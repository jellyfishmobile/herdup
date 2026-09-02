//! Executor behaviour, driven against the scriptable `fake-herdr`.
//!
//! No herdr installed, no real panes — but the same spawn path, and scripted
//! agent responses so the safety-critical decisions are testable.

use launcher_core::execute::{AttentionReason, Event, Executor, PaneState};
use launcher_core::herdr::HerdrCli;
use launcher_core::plan::{plan, LaunchRequest};
use launcher_core::registry::Registry;
use launcher_core::template::Templates;
use serde_json::{json, Value};
use std::path::Path;

const FAKE: &str = env!("CARGO_BIN_EXE_fake-herdr");

fn client(test_name: &str, rules: Value) -> HerdrCli {
    let dir = std::env::temp_dir().join(format!("herdup-x-{}-{}", std::process::id(), test_name));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let script = dir.join("script.json");
    let _ = std::fs::remove_file(dir.join("script.json.state.json"));
    std::fs::write(&script, json!({ "rules": rules }).to_string()).expect("write script");
    HerdrCli::new(FAKE).with_env("FAKE_HERDR_SCRIPT", script.to_string_lossy().into_owned())
}

fn ok(body: Value) -> Value {
    json!({ "stdout": json!({ "id": "t", "result": body }).to_string(), "exit": 0 })
}

/// herdr writes API error envelopes to stderr and exits 1.
fn api_error(code: &str, message: &str) -> Value {
    json!({
        "stderr": json!({ "id": "t", "error": { "code": code, "message": message } }).to_string(),
        "exit": 1
    })
}

fn pane(id: &str, status: &str) -> Value {
    json!({
        "pane_id": id, "tab_id": "w1:t1", "workspace_id": "w1",
        "cwd": "D:\\work\\herdup", "agent_status": status
    })
}

/// Shape captured from herdr 0.8.2 (`tests/fixtures/herdr/agent_*.json`).
fn agent(name: &str, ready: bool) -> Value {
    json!({
        "name": name, "agent": "claude", "pane_id": "w1:p1",
        "agent_status": if ready { "idle" } else { "blocked" },
        "interactive_ready": ready, "launch_pending": !ready,
        "cwd": "D:\\work\\herdup"
    })
}

fn base_rules() -> Vec<Value> {
    vec![
        json!({
            "match": ["workspace", "create"],
            "responses": [ok(json!({
                "workspace": { "workspace_id": "w1", "label": "t" },
                "tab": { "tab_id": "w1:t1", "workspace_id": "w1" },
                "root_pane": pane("w1:p1", "unknown")
            }))]
        }),
        json!({
            "match": ["pane", "split"],
            "responses": [
                ok(json!({ "pane": pane("w1:p2", "unknown") })),
                ok(json!({ "pane": pane("w1:p3", "unknown") })),
                ok(json!({ "pane": pane("w1:p4", "unknown") }))
            ]
        }),
        json!({ "match": ["pane", "rename"], "responses": [ok(json!({ "pane": pane("w1:p1", "unknown") }))] }),
        json!({ "match": ["pane", "run"], "responses": [{ "exit": 0 }] }),
        json!({ "match": ["pane", "read"], "responses": [{ "stdout": "❯ Trust this folder? 1. Yes  2. No", "exit": 0 }] }),
    ]
}

fn rules(extra: Vec<Value>) -> Value {
    let mut all = extra;
    all.extend(base_rules());
    Value::Array(all)
}

/// `agent start` succeeds and reports readiness; `agent prompt` succeeds.
fn healthy_agent_rules() -> Vec<Value> {
    vec![
        json!({ "match": ["agent", "start"], "responses": [ok(json!({ "agent": agent("dev", true) }))] }),
        json!({ "match": ["agent", "prompt"], "responses": [ok(json!({ "agent": agent("dev", true) }))] }),
    ]
}

fn make_plan(template_id: &str, swap: Option<(usize, &str)>) -> launcher_core::plan::LaunchPlan {
    let reg = Registry::builtin();
    let templates = Templates::builtin();
    let t = templates.get(template_id).expect("template");
    let mut req = LaunchRequest::new(Path::new("D:\\work\\herdup"), t);
    if let Some((i, cli)) = swap {
        req = req.override_cli(i, cli);
    }
    plan(&req, &reg).expect("plans")
}

fn run(
    cli: &HerdrCli,
    p: &launcher_core::plan::LaunchPlan,
) -> (launcher_core::execute::Outcome, Vec<Event>) {
    let mut events = Vec::new();
    let outcome = Executor::new(cli).execute(p, &mut |e| events.push(e));
    (outcome, events)
}

// ---------------------------------------------------------------------------

#[test]
fn a_clean_run_creates_every_pane_and_briefs_them_all() {
    let cli = client("happy", rules(healthy_agent_rules()));
    let p = make_plan("duo", None);
    let (outcome, _) = run(&cli, &p);

    assert!(outcome.succeeded(), "failure: {:?}", outcome.failure);
    assert_eq!(outcome.steps_run, outcome.steps_total);
    assert_eq!(outcome.workspace_id.as_deref(), Some("w1"));
    assert_eq!(outcome.briefed(), 2);
    assert!(outcome.needing_attention().is_empty());
    assert_eq!(outcome.panes[0].pane_id.as_deref(), Some("w1:p1"));
    assert_eq!(outcome.panes[0].agent_name.as_deref(), Some("dev"));
    assert_eq!(outcome.panes[1].agent_name.as_deref(), Some("reviewer"));
}

#[test]
fn an_unverified_cli_is_not_briefed_even_when_herdr_says_it_is_ready() {
    // THE Phase 0 regression. Gemini reported ready while a blocking trust modal
    // was on screen. herdr's own guard uses the same detection, so herdup's
    // registry tier is the outer layer that must still refuse.
    let cli = client("unverified", rules(healthy_agent_rules()));
    let p = make_plan("duo", Some((1, "gemini")));
    let (outcome, events) = run(&cli, &p);

    assert!(outcome.succeeded(), "withholding is not a failure");
    assert_eq!(
        outcome.panes[0].state,
        PaneState::Briefed,
        "claude is briefed"
    );
    assert_eq!(
        outcome.panes[1].state,
        PaneState::NeedsAttention(AttentionReason::UnverifiedCli),
        "gemini must be withheld despite herdr reporting it ready"
    );

    let held = outcome.panes[1].pending_briefing.as_deref().expect("held");
    assert!(
        held.contains("review code"),
        "held the real briefing: {held}"
    );
    assert!(
        outcome.panes[1].screen.is_some(),
        "captured the pane for review"
    );

    assert!(events.iter().any(|e| matches!(
        e,
        Event::BriefingWithheld {
            reason: AttentionReason::UnverifiedCli,
            ..
        }
    )));
}

#[test]
fn an_agent_blocked_on_a_startup_prompt_is_flagged_not_failed() {
    // `agent start` returns agent_not_ready for a login or first-run trust
    // prompt. The agent exists and its name stays usable; it just needs a human.
    let cli = client(
        "not_ready",
        rules(vec![json!({
            "match": ["agent", "start"],
            "responses": [api_error("agent_not_ready", "agent dev is blocked during startup")]
        })]),
    );
    let p = make_plan("solo", None);
    let (outcome, events) = run(&cli, &p);

    assert!(
        outcome.succeeded(),
        "a blocked agent is not a launch failure"
    );
    assert_eq!(
        outcome.panes[0].state,
        PaneState::NeedsAttention(AttentionReason::Blocked)
    );
    // The name is retained so the pane can be read and answered.
    assert_eq!(outcome.panes[0].agent_name.as_deref(), Some("dev"));
    assert!(outcome.panes[0].pending_briefing.is_some());
    assert!(outcome.panes[0]
        .screen
        .as_deref()
        .unwrap_or_default()
        .contains("Trust this folder"));
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::PaneNeedsAttention { .. })));
}

#[test]
fn herdrs_own_guard_catches_a_briefing_our_gate_would_have_sent() {
    // Defence in depth: our gate says send (claude is verified, the agent
    // started ready), but herdr refuses because the agent is at a dialog —
    // *before writing any bytes*. That refusal must land as a withheld
    // briefing, not as an error.
    let cli = client(
        "herdr_guard",
        rules(vec![
            json!({ "match": ["agent", "start"], "responses": [ok(json!({ "agent": agent("dev", true) }))] }),
            json!({
                "match": ["agent", "prompt"],
                "responses": [api_error("agent_blocked", "agent dev is at a dialog")]
            }),
        ]),
    );
    let p = make_plan("solo", None);
    let (outcome, events) = run(&cli, &p);

    assert!(outcome.succeeded(), "a refusal is not a launch failure");
    assert_eq!(
        outcome.panes[0].state,
        PaneState::NeedsAttention(AttentionReason::Blocked)
    );
    assert!(
        outcome.panes[0].pending_briefing.is_some(),
        "the briefing is kept, not lost"
    );
    assert_eq!(outcome.briefed(), 0);
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::BriefingWithheld { .. })));
}

#[test]
fn a_pane_whose_shell_is_still_coming_up_is_retried_not_failed() {
    // Observed live: the same launch succeeded once and failed the next time
    // with agent_pane_busy, because herdr needs an *available* shell pane and a
    // just-created pane is not always at its prompt yet.
    let cli = client(
        "pane_busy",
        rules(vec![
            json!({
                "match": ["agent", "start"],
                "responses": [
                    api_error("agent_pane_busy", "pane w1:p1 is not an available shell"),
                    api_error("agent_pane_busy", "pane w1:p1 is not an available shell"),
                    ok(json!({ "agent": agent("dev", true) }))
                ]
            }),
            json!({ "match": ["agent", "prompt"], "responses": [ok(json!({ "agent": agent("dev", true) }))] }),
        ]),
    );
    let p = make_plan("solo", None);
    let (outcome, _) = run(&cli, &p);

    assert!(outcome.succeeded(), "failure: {:?}", outcome.failure);
    assert_eq!(outcome.panes[0].state, PaneState::Briefed);
    assert_eq!(outcome.briefed(), 1);
}

#[test]
fn a_non_transient_start_error_is_not_retried_into_the_ground() {
    let cli = client(
        "hard_start_error",
        rules(vec![json!({
            "match": ["agent", "start"],
            "responses": [api_error("pane_not_found", "no such pane")]
        })]),
    );
    let p = make_plan("solo", None);
    let (outcome, _) = run(&cli, &p);

    assert!(!outcome.succeeded(), "a real error must stop the plan");
    assert!(outcome
        .failure
        .as_ref()
        .expect("failure")
        .message
        .contains("no such pane"));
}

#[test]
fn a_start_that_succeeds_without_asserting_readiness_is_not_trusted() {
    // herdr returning success but interactive_ready=false is not a promise we
    // can type into. Withhold rather than assume.
    let cli = client(
        "not_interactive",
        rules(vec![json!({
            "match": ["agent", "start"],
            "responses": [ok(json!({ "agent": agent("dev", false) }))]
        })]),
    );
    let p = make_plan("solo", None);
    let (outcome, _) = run(&cli, &p);

    assert_eq!(
        outcome.panes[0].state,
        PaneState::NeedsAttention(AttentionReason::Timeout)
    );
    assert_eq!(outcome.briefed(), 0);
}

#[test]
fn a_cli_with_no_agent_kind_runs_raw_and_is_never_briefed() {
    // antigravity has no herdr agent kind, so there is no readiness signal and
    // no agent_blocked guard.
    let cli = client("no_kind", rules(healthy_agent_rules()));
    let p = make_plan("solo", Some((0, "antigravity")));
    let (outcome, _) = run(&cli, &p);

    assert!(outcome.succeeded());
    assert!(outcome.panes[0].agent_name.is_none());
    assert_eq!(
        outcome.panes[0].state,
        PaneState::NeedsAttention(AttentionReason::UnverifiedCli)
    );
    assert_eq!(outcome.briefed(), 0);
}

#[test]
fn a_mid_plan_failure_stops_and_leaves_earlier_panes_standing() {
    // No rollback: a half-built team is still useful, and its panes may already
    // hold running agents (spec §11).
    let mut extra = healthy_agent_rules();
    extra.insert(
        0,
        json!({ "match": ["pane", "split"], "responses": [api_error("pane_not_found", "boom")] }),
    );
    let cli = client("midfail", rules(extra));
    let p = make_plan("duo", None);
    let (outcome, events) = run(&cli, &p);

    assert!(!outcome.succeeded());
    let failure = outcome.failure.as_ref().expect("failure recorded");
    assert!(failure.message.contains("boom"), "{}", failure.message);
    assert!(
        failure.description.contains("split"),
        "{}",
        failure.description
    );
    assert!(outcome.steps_run < outcome.steps_total);

    assert_eq!(outcome.panes[0].pane_id.as_deref(), Some("w1:p1"));
    assert_eq!(outcome.panes[1].state, PaneState::NotCreated);
    assert!(outcome.panes[1].pane_id.is_none());
    assert_eq!(outcome.workspace_id.as_deref(), Some("w1"));
    assert!(events.iter().any(|e| matches!(e, Event::Failed { .. })));
    assert!(events.iter().any(|e| matches!(e, Event::Finished)));
}

#[test]
fn a_withheld_briefing_can_be_released_after_a_human_intervenes() {
    // What the UI's "Send briefing now" button does. It goes through the agent
    // surface too, so if the human has not actually cleared the dialog herdr
    // refuses again rather than typing into it.
    let cli = client(
        "release",
        rules(vec![
            json!({
                "match": ["agent", "start"],
                "responses": [api_error("agent_not_ready", "blocked during startup")]
            }),
            json!({ "match": ["agent", "prompt"], "responses": [ok(json!({ "agent": agent("dev", true) }))] }),
        ]),
    );
    let p = make_plan("solo", None);
    let (mut outcome, _) = run(&cli, &p);

    assert_eq!(
        outcome.panes[0].state,
        PaneState::NeedsAttention(AttentionReason::Blocked)
    );

    Executor::new(&cli)
        .send_pending_briefing(&mut outcome.panes[0])
        .expect("releases");

    assert_eq!(outcome.panes[0].state, PaneState::Briefed);
    assert!(outcome.panes[0].pending_briefing.is_none());
}

#[test]
fn the_coordinator_is_briefed_through_its_agent_name_with_real_pane_ids() {
    let cli = client(
        "roster",
        rules(vec![
            json!({ "match": ["agent", "start"], "responses": [ok(json!({ "agent": agent("x", true) }))] }),
            // Only matches if the coordinator is addressed by its agent name.
            json!({ "match": ["agent", "prompt", "pm"], "responses": [ok(json!({ "agent": agent("pm", true) }))] }),
            json!({ "match": ["agent", "prompt"], "responses": [ok(json!({ "agent": agent("x", true) }))] }),
        ]),
    );
    let p = make_plan("squad", None);
    let (outcome, _) = run(&cli, &p);

    assert!(outcome.succeeded(), "failure: {:?}", outcome.failure);
    assert_eq!(outcome.briefed(), 4);

    let coord = &outcome.panes[0];
    assert_eq!(coord.role, "PM");
    assert_eq!(coord.agent_name.as_deref(), Some("pm"));
    assert_eq!(coord.state, PaneState::Briefed);
}

#[test]
fn events_report_progress_for_every_step() {
    let cli = client("events", rules(healthy_agent_rules()));
    let p = make_plan("duo", None);
    let (outcome, events) = run(&cli, &p);

    let started = events
        .iter()
        .filter(|e| matches!(e, Event::StepStarted { .. }))
        .count();
    assert_eq!(started, outcome.steps_total);
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, Event::PaneCreated { .. }))
            .count(),
        2
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, Event::Briefed { .. }))
            .count(),
        2
    );
    assert!(matches!(events.last(), Some(Event::Finished)));
}

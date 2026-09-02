//! Executor behaviour, driven against the scriptable `fake-herdr`.
//!
//! No herdr installed, no real panes — but the same spawn path, and scripted
//! `agent_status` transitions so the safety-critical decisions are testable.

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

/// Rules common to every scenario: create the workspace, hand out pane ids, and
/// accept the fire-and-forget commands.
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
        json!({ "match": ["pane", "send-text"], "responses": [{ "exit": 0 }] }),
        json!({ "match": ["pane", "send-keys"], "responses": [{ "exit": 0 }] }),
        json!({ "match": ["pane", "read"], "responses": [{ "stdout": "❯ Trust this folder? 1. Yes  2. No", "exit": 0 }] }),
    ]
}

fn rules(extra: Vec<Value>) -> Value {
    // Specific rules first: the fake takes the first match.
    let mut all = extra;
    all.extend(base_rules());
    Value::Array(all)
}

/// Plan `template_id`, optionally swapping one pane's CLI.
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
    let cli = client(
        "happy",
        rules(vec![
            json!({ "match": ["wait", "agent-status"], "responses": [ok(json!({ "type": "ok" }))] }),
            json!({ "match": ["pane", "get"], "responses": [ok(json!({ "pane": pane("w1:p1", "idle") }))] }),
        ]),
    );
    let p = make_plan("duo", None);
    let (outcome, _) = run(&cli, &p);

    assert!(outcome.succeeded(), "failure: {:?}", outcome.failure);
    assert_eq!(outcome.steps_run, outcome.steps_total);
    assert_eq!(outcome.workspace_id.as_deref(), Some("w1"));
    assert_eq!(outcome.briefed(), 2);
    assert!(outcome.needing_attention().is_empty());
    assert_eq!(outcome.panes[0].pane_id.as_deref(), Some("w1:p1"));
    assert_eq!(outcome.panes[1].pane_id.as_deref(), Some("w1:p2"));
}

#[test]
fn an_unverified_cli_reporting_idle_is_still_not_briefed() {
    // THE Phase 0 regression. Gemini reported `idle` while a blocking trust
    // modal was on screen; a briefing sent then was swallowed by the modal and
    // its trailing Enter granted folder trust. Even with herdr insisting the
    // pane is idle, an unverified CLI must not be typed into.
    let cli = client(
        "unverified",
        rules(vec![
            json!({ "match": ["wait", "agent-status"], "responses": [ok(json!({ "type": "ok" }))] }),
            json!({ "match": ["pane", "get"], "responses": [ok(json!({ "pane": pane("w1:p1", "idle") }))] }),
        ]),
    );
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
        "gemini must be withheld despite reporting idle"
    );

    // The briefing is kept for a human to release, not discarded.
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
fn a_pane_sitting_on_a_prompt_is_flagged_and_its_briefing_withheld() {
    // The first-run trust prompt: the wait expires and herdr reports `blocked`.
    let cli = client(
        "blocked",
        rules(vec![
            json!({ "match": ["wait", "agent-status", "w1:p2"], "responses": [{ "exit": 1 }] }),
            json!({ "match": ["pane", "get", "w1:p2"], "responses": [ok(json!({ "pane": pane("w1:p2", "blocked") }))] }),
            json!({ "match": ["wait", "agent-status"], "responses": [ok(json!({ "type": "ok" }))] }),
            json!({ "match": ["pane", "get"], "responses": [ok(json!({ "pane": pane("w1:p1", "idle") }))] }),
        ]),
    );
    let p = make_plan("duo", None);
    let (outcome, events) = run(&cli, &p);

    assert!(
        outcome.succeeded(),
        "a blocked pane is not a launch failure"
    );
    assert_eq!(outcome.panes[0].state, PaneState::Briefed);
    assert_eq!(
        outcome.panes[1].state,
        PaneState::NeedsAttention(AttentionReason::Blocked)
    );
    assert!(outcome.panes[1].pending_briefing.is_some());
    // The UI can show why without the user switching to the terminal.
    assert!(outcome.panes[1]
        .screen
        .as_deref()
        .unwrap_or_default()
        .contains("Trust this folder"));

    assert!(events.iter().any(|e| matches!(
        e,
        Event::PaneNeedsAttention {
            reason: AttentionReason::Blocked,
            ..
        }
    )));
}

#[test]
fn a_pane_that_never_settles_times_out_and_is_withheld() {
    let cli = client(
        "timeout",
        rules(vec![
            json!({ "match": ["wait", "agent-status"], "responses": [{ "exit": 1 }] }),
            json!({ "match": ["pane", "get"], "responses": [ok(json!({ "pane": pane("w1:p1", "working") }))] }),
        ]),
    );
    let p = make_plan("solo", None);
    let (outcome, _) = run(&cli, &p);

    assert!(outcome.succeeded());
    assert_eq!(
        outcome.panes[0].state,
        PaneState::NeedsAttention(AttentionReason::Timeout)
    );
    assert_eq!(outcome.briefed(), 0);
}

#[test]
fn a_wait_that_lands_but_reports_blocked_is_not_treated_as_ready() {
    // Trust the observed status over the wait's exit code. This is the shape of
    // the Phase 0 failure at the herdr layer rather than the registry layer.
    let cli = client(
        "wait_lands_blocked",
        rules(vec![
            json!({ "match": ["wait", "agent-status"], "responses": [ok(json!({ "type": "ok" }))] }),
            json!({ "match": ["pane", "get"], "responses": [ok(json!({ "pane": pane("w1:p1", "blocked") }))] }),
        ]),
    );
    let p = make_plan("solo", None);
    let (outcome, _) = run(&cli, &p);

    assert_eq!(
        outcome.panes[0].state,
        PaneState::NeedsAttention(AttentionReason::Blocked)
    );
    assert_eq!(outcome.briefed(), 0);
}

#[test]
fn a_mid_plan_failure_stops_and_leaves_earlier_panes_standing() {
    // No rollback: a half-built team is still useful, and its panes may already
    // hold running agents (spec §11).
    let cli = client(
        "midfail",
        rules(vec![
            json!({ "match": ["pane", "split"], "responses": [api_error("pane_not_found", "boom")] }),
            json!({ "match": ["wait", "agent-status"], "responses": [ok(json!({ "type": "ok" }))] }),
            json!({ "match": ["pane", "get"], "responses": [ok(json!({ "pane": pane("w1:p1", "idle") }))] }),
        ]),
    );
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

    // Pane 0 survived; pane 1 never came into existence.
    assert_eq!(outcome.panes[0].pane_id.as_deref(), Some("w1:p1"));
    assert_eq!(outcome.panes[1].state, PaneState::NotCreated);
    assert!(outcome.panes[1].pane_id.is_none());

    // The workspace is still reported, so the user can go look at what exists.
    assert_eq!(outcome.workspace_id.as_deref(), Some("w1"));
    assert!(events.iter().any(|e| matches!(e, Event::Failed { .. })));
    assert!(events.iter().any(|e| matches!(e, Event::Finished)));
}

#[test]
fn a_withheld_briefing_can_be_released_after_a_human_intervenes() {
    // What the UI's "Send briefing now" button does.
    let cli = client(
        "release",
        rules(vec![
            json!({ "match": ["wait", "agent-status"], "responses": [{ "exit": 1 }] }),
            json!({ "match": ["pane", "get"], "responses": [ok(json!({ "pane": pane("w1:p1", "blocked") }))] }),
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
fn the_coordinator_briefing_is_sent_with_real_pane_ids() {
    let cli = client(
        "roster",
        rules(vec![
            json!({ "match": ["wait", "agent-status"], "responses": [ok(json!({ "type": "ok" }))] }),
            json!({ "match": ["pane", "get"], "responses": [ok(json!({ "pane": pane("w1:p1", "idle") }))] }),
            // Only matches if the roster resolved PaneRefs to the ids the fake
            // handed out during this very run.
            json!({
                "match": ["pane", "send-text", "w1:p1"],
                "responses": [{ "exit": 0 }]
            }),
        ]),
    );
    let p = make_plan("squad", None);
    let (outcome, _) = run(&cli, &p);

    assert!(outcome.succeeded(), "failure: {:?}", outcome.failure);
    assert_eq!(outcome.briefed(), 4);

    let coord = &outcome.panes[0];
    assert_eq!(coord.role, "PM");
    assert_eq!(coord.state, PaneState::Briefed);
}

#[test]
fn events_report_progress_for_every_step() {
    let cli = client(
        "events",
        rules(vec![
            json!({ "match": ["wait", "agent-status"], "responses": [ok(json!({ "type": "ok" }))] }),
            json!({ "match": ["pane", "get"], "responses": [ok(json!({ "pane": pane("w1:p1", "idle") }))] }),
        ]),
    );
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

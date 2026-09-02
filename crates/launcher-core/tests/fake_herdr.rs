//! Drive `HerdrCli` end to end against the scriptable `fake-herdr` binary.
//!
//! These run with no herdr installed, and go through the real spawn path — so
//! argv construction, envelope handling and error mapping are all exercised.

use launcher_core::herdr::types::{AgentStatus, ReadSource, SplitDirection, WaitOutcome};
use launcher_core::herdr::{HerdrCli, HerdrError};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const FAKE: &str = env!("CARGO_BIN_EXE_fake-herdr");

/// Write a script to a test-unique directory and return a client wired to it.
fn client(test_name: &str, rules: Value) -> HerdrCli {
    let dir = std::env::temp_dir().join(format!("herdup-{}-{}", std::process::id(), test_name));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let script = dir.join("script.json");
    let state = dir.join("script.json.state.json");
    let _ = std::fs::remove_file(&state); // fresh call counters per test
    std::fs::write(&script, json!({ "rules": rules }).to_string()).expect("write script");
    HerdrCli::new(FAKE).with_env("FAKE_HERDR_SCRIPT", script.to_string_lossy().into_owned())
}

fn ok(body: Value) -> Value {
    json!({ "stdout": json!({ "id": "cli:test", "result": body }).to_string(), "exit": 0 })
}

/// Real herdr writes API error envelopes to **stderr** and exits non-zero.
/// Verified against herdr 0.8.2 by capturing the streams to separate files.
fn api_error(code: &str, message: &str) -> Value {
    json!({
        "stderr": json!({ "id": "cli:test", "error": { "code": code, "message": message } })
            .to_string(),
        "exit": 1
    })
}

/// The same payload on stdout. herdr does not currently do this, but the parser
/// accepts either so a future change cannot silently degrade error handling.
fn api_error_on_stdout(code: &str, message: &str) -> Value {
    json!({
        "stdout": json!({ "id": "cli:test", "error": { "code": code, "message": message } })
            .to_string(),
        "exit": 1
    })
}

fn pane(id: &str, status: &str) -> Value {
    json!({
        "pane_id": id, "tab_id": "w1:t1", "workspace_id": "w1",
        "cwd": "D:\\work\\herdup", "focused": false, "agent_status": status
    })
}

// ---------------------------------------------------------------------------

#[test]
fn workspace_create_parses_and_returns_ids() {
    let cli = client(
        "ws_create",
        json!([{
            "match": ["workspace", "create"],
            "responses": [ok(json!({
                "workspace": { "workspace_id": "w1", "label": "demo" },
                "tab": { "tab_id": "w1:t1", "workspace_id": "w1" },
                "root_pane": pane("w1:p1", "unknown")
            }))]
        }]),
    );

    let created = cli
        .workspace_create(Path::new("D:\\work\\herdup"), Some("demo"), false)
        .expect("created");
    assert_eq!(created.workspace.workspace_id, "w1");
    assert_eq!(created.root_pane.pane_id, "w1:p1");
}

#[test]
fn pane_split_then_rename_round_trips() {
    let cli = client(
        "split_rename",
        json!([
            { "match": ["pane", "split"],
              "responses": [ok(json!({ "pane": pane("w1:p2", "unknown") }))] },
            { "match": ["pane", "rename", "w1:p2", "QA"],
              "responses": [ok(json!({ "pane": {
                  "pane_id": "w1:p2", "tab_id": "w1:t1", "workspace_id": "w1",
                  "label": "QA", "agent_status": "unknown" } }))] }
        ]),
    );

    let p = cli
        .pane_split("w1:p1", SplitDirection::Right, Some(0.5), None, false)
        .expect("split");
    assert_eq!(p.pane_id, "w1:p2");

    let renamed = cli.pane_rename("w1:p2", "QA").expect("rename");
    assert_eq!(renamed.label.as_deref(), Some("QA"));
}

// ---- error mapping --------------------------------------------------------

#[test]
fn a_stopped_server_maps_to_a_recoverable_error() {
    let cli = client(
        "no_server",
        json!([{
            "match": ["workspace", "list"],
            "responses": [api_error("server_not_running", "no herdr server is running at ...")]
        }]),
    );

    match cli.workspace_list() {
        Err(e @ HerdrError::ServerUnavailable { .. }) => {
            assert!(e.is_recoverable_by_starting_server());
        }
        other => panic!("expected ServerUnavailable, got {other:?}"),
    }
    assert!(!cli.server_running());
}

#[test]
fn a_protocol_mismatch_is_distinct_and_not_recoverable() {
    // Resolving this means stopping the user's server, which kills their panes.
    // It must never be confused with "no server running".
    let cli = client(
        "proto",
        json!([{
            "match": ["workspace", "list"],
            "responses": [api_error("protocol_mismatch", "client protocol 21 is newer than server protocol 20")]
        }]),
    );

    match cli.workspace_list() {
        Err(e @ HerdrError::ProtocolMismatch { .. }) => {
            assert!(!e.is_recoverable_by_starting_server());
        }
        other => panic!("expected ProtocolMismatch, got {other:?}"),
    }
}

#[test]
fn an_error_envelope_on_stdout_is_also_parsed() {
    // Regression guard for the stream question itself. herdr uses stderr today;
    // if that ever changes, error typing must not silently regress to
    // CommandFailed the way it did before this was fixed.
    let cli = client(
        "err_stdout",
        json!([{
            "match": ["workspace", "list"],
            "responses": [api_error_on_stdout("server_not_running", "gone")]
        }]),
    );
    assert!(matches!(
        cli.workspace_list(),
        Err(HerdrError::ServerUnavailable { .. })
    ));
}

#[test]
fn server_running_reports_false_for_a_dead_server() {
    // The bug this guards: an unparsed error became CommandFailed, which is not
    // ServerUnavailable, so server_running() answered true for a dead server and
    // the caller skipped starting one.
    let cli = client(
        "server_running_false",
        json!([{
            "match": ["workspace", "list"],
            "responses": [api_error("server_not_running", "no herdr server is running at ...")]
        }]),
    );
    assert!(!cli.server_running());
}

#[test]
fn an_unrecognised_api_error_preserves_its_code() {
    let cli = client(
        "api_err",
        json!([{
            "match": ["pane", "get"],
            "responses": [api_error("pane_not_found", "no such pane w1:p9")]
        }]),
    );

    match cli.pane_get("w1:p9") {
        Err(HerdrError::Api { code, message }) => {
            assert_eq!(code, "pane_not_found");
            assert!(message.contains("w1:p9"));
        }
        other => panic!("expected Api, got {other:?}"),
    }
}

#[test]
fn an_unmatched_command_fails_loudly_rather_than_silently() {
    let cli = client("unmatched", json!([]));
    match cli.pane_list() {
        Err(HerdrError::CommandFailed { code, .. }) => assert_eq!(code, Some(97)),
        other => panic!("expected CommandFailed, got {other:?}"),
    }
}

#[test]
fn a_missing_binary_is_reported_as_not_found() {
    let cli = HerdrCli::new(PathBuf::from("herdr-does-not-exist-xyz"));
    assert!(matches!(cli.pane_list(), Err(HerdrError::NotFound)));
}

// ---- waiting --------------------------------------------------------------

#[test]
fn a_wait_timeout_is_an_outcome_not_an_error() {
    // herdr exits 1 with no JSON body when a wait expires. The launcher reacts
    // by withholding a briefing, which is normal operation — not a failure.
    let cli = client(
        "wait_timeout",
        json!([{ "match": ["wait", "agent-status"], "responses": [{ "exit": 1 }] }]),
    );
    let outcome = cli
        .wait_agent_status("w1:p1", AgentStatus::Idle, 100)
        .expect("timeout is not an error");
    assert_eq!(outcome, WaitOutcome::TimedOut);
    assert!(!outcome.reached());
}

#[test]
fn a_wait_that_lands_reports_reached() {
    let cli = client(
        "wait_ok",
        json!([{ "match": ["wait", "agent-status"], "responses": [ok(json!({ "type": "ok" }))] }]),
    );
    assert!(cli
        .wait_agent_status("w1:p1", AgentStatus::Idle, 100)
        .expect("ok")
        .reached());
}

#[test]
fn a_wait_against_a_dead_server_is_still_a_real_error() {
    // Non-zero exit alone must not be read as "timed out" when there is an
    // error body explaining otherwise.
    let cli = client(
        "wait_dead",
        json!([{
            "match": ["wait", "agent-status"],
            "responses": [api_error("server_not_running", "gone")]
        }]),
    );
    assert!(matches!(
        cli.wait_agent_status("w1:p1", AgentStatus::Idle, 100),
        Err(HerdrError::ServerUnavailable { .. })
    ));
}

// ---- the Phase 0 scenario, as a test -------------------------------------

#[test]
fn blocked_then_idle_is_observable_across_successive_polls() {
    // The sign-in / first-run sequence: a CLI sits blocked on a prompt, a human
    // deals with it, and the next poll reports idle. This is what Stage 1 waits
    // for, and it was only manually observable before Phase 0.
    let cli = client(
        "transition",
        json!([{
            "match": ["pane", "get", "w1:p1"],
            "responses": [
                ok(json!({ "pane": pane("w1:p1", "unknown") })),
                ok(json!({ "pane": pane("w1:p1", "blocked") })),
                ok(json!({ "pane": pane("w1:p1", "idle") }))
            ]
        }]),
    );

    let seen: Vec<AgentStatus> = (0..4)
        .map(|_| cli.pane_get("w1:p1").expect("get").agent_status)
        .collect();

    assert_eq!(
        seen,
        vec![
            AgentStatus::Unknown,
            AgentStatus::Blocked,
            AgentStatus::Idle,
            AgentStatus::Idle, // the last scripted response repeats
        ]
    );
    assert!(!seen[1].is_settled());
    assert!(seen[2].is_settled());
}

// ---- argv integrity -------------------------------------------------------

#[test]
fn a_briefing_with_quotes_and_spaces_reaches_herdr_intact() {
    // The rule only matches if the whole briefing arrived as ONE argv element,
    // byte-for-byte. Shelling out would have split or mangled this.
    const BRIEFING: &str =
        r#"You are "QA". Don't write features; run the suite & report failures (all of them)."#;

    let cli = client(
        "argv_quotes",
        json!([{
            "match": ["pane", "send-text", "w1:p1", BRIEFING],
            "responses": [{ "exit": 0 }]
        }]),
    );
    cli.pane_send_text("w1:p1", BRIEFING)
        .expect("briefing survived argv");
}

#[test]
fn a_path_with_spaces_reaches_herdr_intact() {
    let cli = client(
        "argv_path",
        json!([{
            "match": ["workspace", "create", "--cwd", "C:\\Users\\me\\My Projects\\a b"],
            "responses": [ok(json!({
                "workspace": { "workspace_id": "w1" },
                "tab": { "tab_id": "w1:t1", "workspace_id": "w1" },
                "root_pane": pane("w1:p1", "unknown")
            }))]
        }]),
    );
    cli.workspace_create(Path::new("C:\\Users\\me\\My Projects\\a b"), None, false)
        .expect("path survived argv");
}

#[test]
fn the_session_flag_is_passed_before_the_subcommand() {
    // Every automated call must be scoped to a named session so it cannot touch
    // a developer's live panes (ground truth §2).
    let cli = client(
        "session_flag",
        json!([{
            "match": ["--session", "herdup-test", "pane", "list"],
            "responses": [ok(json!({ "panes": [] }))]
        }]),
    )
    .with_session("herdup-test");

    assert_eq!(cli.session(), Some("herdup-test"));
    assert!(cli.pane_list().expect("listed").is_empty());
}

#[test]
fn send_keys_forwards_each_key_as_its_own_argument() {
    let cli = client(
        "send_keys",
        json!([{
            "match": ["pane", "send-keys", "w1:p1", "Down", "Enter"],
            "responses": [{ "exit": 0 }]
        }]),
    );
    cli.pane_send_keys("w1:p1", &["Down", "Enter"])
        .expect("sent");
}

// ---- text output ----------------------------------------------------------

#[test]
fn pane_read_returns_raw_text_not_json() {
    let screen = "❯ Try \"fix lint errors\"\n  auto mode on\n";
    let cli = client(
        "pane_read",
        json!([{
            "match": ["pane", "read", "w1:p1", "--source", "recent"],
            "responses": [{ "stdout": screen, "exit": 0 }]
        }]),
    );
    let got = cli
        .pane_read("w1:p1", ReadSource::Recent, 20)
        .expect("read");
    assert_eq!(got, screen);
}

//! Parse the real herdr 0.8.2 captures from Phase 0.
//!
//! These are recorded fact, not hand-written examples. If herdr's shapes change,
//! re-capture the fixtures and these tests tell you what broke.

use launcher_core::herdr::types::{AgentStatus, Pane, TabCreated, Version, WorkspaceCreated};
use serde::Deserialize;
use std::path::PathBuf;

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/herdr")
        .join(format!("{name}.json"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading fixture {}: {e}", path.display()))
}

/// Mirrors the private envelope handling: pull `result` out, then deserialise.
fn result_of<T: for<'de> Deserialize<'de>>(name: &str) -> T {
    let value: serde_json::Value = serde_json::from_str(&fixture(name)).expect("fixture is JSON");
    assert!(
        value.get("error").is_none(),
        "fixture {name} unexpectedly contains an error payload"
    );
    let result = value.get("result").expect("fixture has a result").clone();
    serde_json::from_value(result).expect("result deserialises")
}

#[test]
fn workspace_create_exposes_the_three_ids_the_spec_depends_on() {
    let created: WorkspaceCreated = result_of("workspace_create");
    assert_eq!(created.workspace.workspace_id, "w1");
    assert_eq!(created.tab.tab_id, "w1:t1");
    assert_eq!(created.root_pane.pane_id, "w1:p1");
    assert_eq!(created.workspace.label.as_deref(), Some("herdup-spike"));
}

#[test]
fn pane_split_exposes_the_new_pane_id() {
    #[derive(Deserialize)]
    struct Info {
        pane: Pane,
    }
    let info: Info = result_of("pane_split");
    assert_eq!(info.pane.pane_id, "w1:p2");
    assert_eq!(info.pane.tab_id, "w1:t1");
}

#[test]
fn tab_create_exposes_tab_and_root_pane() {
    let created: TabCreated = result_of("tab_create");
    assert_eq!(created.tab.tab_id, "w1:t2");
    assert_eq!(created.tab.label.as_deref(), Some("logs"));
    // Pane numbering is workspace-scoped, not tab-scoped (ground truth §3.4).
    assert_eq!(created.root_pane.pane_id, "w1:p3");
}

#[test]
fn pane_list_carries_labels_and_omits_them_when_unset() {
    #[derive(Deserialize)]
    struct List {
        panes: Vec<Pane>,
    }
    let list: List = result_of("pane_list");
    assert_eq!(list.panes.len(), 2);
    assert_eq!(list.panes[0].label, None);
    assert_eq!(list.panes[1].label.as_deref(), Some("QA"));
}

#[test]
fn a_running_claude_reports_its_agent_and_status() {
    #[derive(Deserialize)]
    struct Info {
        pane: Pane,
    }
    let info: Info = result_of("pane_get_claude_running");
    assert_eq!(info.pane.agent.as_deref(), Some("claude"));
    // Captured while Claude Code sat on its trust-this-folder prompt.
    assert_eq!(info.pane.agent_status, AgentStatus::Blocked);
    assert!(!info.pane.agent_status.is_settled());
}

#[test]
fn cwd_trailing_separator_is_normalised() {
    // The SAME pane reports two different cwd strings for the same directory,
    // depending on whether a process is running in it (ground truth §3.5):
    //   idle shell    -> "D:\work\herdr_automation\"
    //   claude running-> "D:\work\herdr_automation"
    // Both captures below are pane w1:p1. They must compare equal.
    #[derive(Deserialize)]
    struct Info {
        pane: Pane,
    }
    let idle: Info = result_of("pane_get_root");
    let running: Info = result_of("pane_get_claude_running");

    assert_eq!(idle.pane.pane_id, running.pane.pane_id, "same pane");
    assert!(
        idle.pane.cwd.ends_with('\\'),
        "idle-shell capture should still carry the raw trailing separator"
    );
    assert!(!running.pane.cwd.ends_with('\\'));
    assert_ne!(idle.pane.cwd, running.pane.cwd, "raw strings differ");
    assert_eq!(
        idle.pane.cwd_path(),
        running.pane.cwd_path(),
        "normalised paths must match"
    );

    // And the creation response agrees with both.
    let created: WorkspaceCreated = result_of("workspace_create");
    assert_eq!(created.root_pane.cwd_path(), running.pane.cwd_path());
}

#[test]
fn unrecognised_agent_status_falls_back_to_unknown() {
    // A future herdr adding a status must not break deserialisation, and must
    // land on the cautious value — nothing is auto-briefed on Unknown.
    #[derive(Deserialize)]
    struct Info {
        pane: Pane,
    }
    let raw = r#"{"pane":{"pane_id":"w1:p9","tab_id":"w1:t1","workspace_id":"w1",
                  "agent_status":"hypnotised"}}"#;
    let info: Info = serde_json::from_str(raw).expect("still parses");
    assert_eq!(info.pane.agent_status, AgentStatus::Unknown);
}

#[test]
fn version_parses_the_windows_preview_string() {
    let v = Version::parse("herdr 0.8.2-preview.2026-08-31-b1ff4582e968").expect("parses");
    assert_eq!((v.major, v.minor, v.patch), (0, 8, 2));
    assert!(v.suffix.starts_with("-preview"));
    // A preview must satisfy its own minimum: Windows builds are preview-only,
    // so treating preview as older than release would reject every install.
    assert!(v.at_least(0, 8, 2));
    assert!(!v.at_least(0, 9, 0));
}

#[test]
fn version_parses_a_plain_release_and_rejects_junk() {
    let v = Version::parse("herdr 0.7.0").expect("parses");
    assert!(v.at_least(0, 7, 0));
    assert!(!v.at_least(0, 8, 2), "0.7.0 must fail the 0.8.2 minimum");
    assert!(Version::parse("herdr").is_none());
    assert!(Version::parse("").is_none());
}

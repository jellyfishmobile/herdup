//! Saving the team that launched back into the project.

use launcher_core::plan::{plan, resolve_team, LaunchRequest};
use launcher_core::registry::Registry;
use launcher_core::template::{PaneSpec, Templates, REPO_TEMPLATE_ID};
use std::path::Path;

fn project() -> &'static Path {
    if cfg!(windows) {
        Path::new("D:\\work\\herdup")
    } else {
        Path::new("/work/herdup")
    }
}

fn added(role: &str, cli: &str) -> PaneSpec {
    PaneSpec {
        role: role.to_string(),
        cli: cli.to_string(),
        flags: String::new(),
        briefing: format!("You are {role}."),
        coordinator: false,
        split: None,
    }
}

#[test]
fn the_resolved_team_matches_what_the_plan_launched() {
    let registry = Registry::builtin();
    let templates = Templates::builtin();
    let squad = templates.get("squad").expect("squad");

    let request = LaunchRequest::new(project(), squad)
        .skip_pane(1)
        .override_cli(2, "hermes")
        .add_pane(added("Scribe", "claude"));

    let planned = plan(&request, &registry).expect("plans");
    let team = resolve_team(&request, &registry).expect("resolves");

    assert_eq!(team.id, REPO_TEMPLATE_ID);
    assert_eq!(team.panes.len(), planned.panes.len());
    for (pane, planned) in team.panes.iter().zip(planned.panes.iter()) {
        assert_eq!(pane.role, planned.role, "role");
        assert_eq!(pane.cli, planned.cli, "cli for {}", pane.role);
        assert_eq!(pane.coordinator, planned.coordinator, "coordinator");
        // The command the plan built is the binary plus exactly these flags.
        let entry = registry.get(&pane.cli).expect("registry entry");
        assert_eq!(
            planned.command,
            launcher_core::template::command_line(&entry.binary, &pane.flags),
            "flags for {}",
            pane.role
        );
    }
}

#[test]
fn flags_dropped_by_a_swap_are_not_saved() {
    // squad's panes carry Claude Code's permission flags. Swapping a pane to a
    // CLI with no verified preset for them drops the flags; the saved team must
    // record what ran, not what the template wished for.
    let registry = Registry::builtin();
    let templates = Templates::builtin();
    let squad = templates.get("squad").expect("squad");
    let request = LaunchRequest::new(project(), squad).override_cli(1, "codex");
    let team = resolve_team(&request, &registry).expect("resolves");
    let swapped = &team.panes[1];
    assert_eq!(swapped.cli, "codex");
    assert!(swapped.flags.is_empty(), "{:?}", swapped.flags);
}

#[test]
fn dropping_the_pane_others_split_from_leaves_a_valid_layout() {
    let registry = Registry::builtin();
    let templates = Templates::builtin();
    let full = templates
        .get("full")
        .or_else(|| templates.get("squad"))
        .expect("a big template");
    let request = LaunchRequest::new(project(), full).skip_pane(0);
    let team = resolve_team(&request, &registry).expect("resolves");
    assert!(team.panes[0].split.is_none(), "the new root has no split");
    for (i, pane) in team.panes.iter().enumerate().skip(1) {
        let split = pane
            .split
            .expect("every non-root pane splits from something");
        assert!(split.from < i, "pane {i} splits from {}", split.from);
    }
}

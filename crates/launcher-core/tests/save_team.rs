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

use launcher_core::template::{
    parse_repo_team, save_repo_team, to_repo_toml, SaveOutcome, Template, REPO_TEAM_FILE,
};
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("herdup-save-team-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn team_with(briefing: &str) -> Template {
    Template {
        id: "repo".to_string(),
        display_name: "Saved team".to_string(),
        description: "from a launch".to_string(),
        panes: vec![
            PaneSpec {
                role: "PM".to_string(),
                cli: "claude".to_string(),
                flags: "--permission-mode bypassPermissions".to_string(),
                briefing: briefing.to_string(),
                coordinator: true,
                split: None,
            },
            added("Dev", "claude"),
        ],
    }
}

/// The second pane needs a split to be a valid team; `added` leaves it None.
fn writable(mut team: Template) -> Template {
    team.panes[1].split = Some(launcher_core::template::Split {
        direction: launcher_core::herdr::types::SplitDirection::Right,
        ratio: Some(0.5),
        from: 0,
    });
    team
}

#[test]
fn a_written_team_reads_back_identically() {
    let registry = Registry::builtin();
    for briefing in [
        "One line.",
        "Two\nlines, with a \"quote\".",
        "A backslash \\ and a triple \"\"\" quote.",
    ] {
        let team = writable(team_with(briefing));
        let text = to_repo_toml(&team);
        let back = parse_repo_team(&text, "team.toml", project(), &registry)
            .unwrap_or_else(|e| panic!("{briefing:?} did not round-trip: {e}\n{text}"));
        assert_eq!(back.panes, team.panes, "{briefing:?}\n{text}");
        assert_eq!(back.display_name, team.display_name);
    }
}

#[test]
fn a_simple_briefing_is_written_readably() {
    let team = writable(team_with("Do the work.\nThen stop."));
    let text = to_repo_toml(&team);
    assert!(text.contains("briefing = \"\"\""), "{text}");
    assert!(text.contains("Do the work.\nThen stop."), "{text}");
}

#[test]
fn saving_creates_the_herdr_folder() {
    let project = scratch("fresh");
    let team = writable(team_with("Go."));
    match save_repo_team(&project, &team, false).expect("saves") {
        SaveOutcome::Written(path) => {
            assert_eq!(path, project.join(REPO_TEAM_FILE));
            assert!(path.is_file());
        }
        other => panic!("{other:?}"),
    }
    std::fs::remove_dir_all(&project).unwrap();
}

#[test]
fn an_existing_file_is_reported_not_replaced() {
    let project = scratch("exists");
    let file = project.join(REPO_TEAM_FILE);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "# hand written\n").unwrap();
    let team = writable(team_with("Go."));

    match save_repo_team(&project, &team, false).expect("reports") {
        SaveOutcome::Exists(path) => assert_eq!(path, file),
        other => panic!("{other:?}"),
    }
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "# hand written\n");

    match save_repo_team(&project, &team, true).expect("overwrites") {
        SaveOutcome::Written(_) => {}
        other => panic!("{other:?}"),
    }
    assert!(std::fs::read_to_string(&file).unwrap().contains("[[pane]]"));
    std::fs::remove_dir_all(&project).unwrap();
}

//! A repository's own team, from `.herdr/team.toml`.

use launcher_core::config::ConfigError;
use launcher_core::registry::Registry;
use launcher_core::template::{
    load_repo_team, parse_repo_team, Templates, REPO_TEAM_FILE, REPO_TEMPLATE_ID,
};
use std::path::{Path, PathBuf};

const VALID: &str = r#"
display_name = "Repo squad"
description  = "two panes"

[[pane]]
role        = "PM"
cli         = "claude"
coordinator = true
briefing    = "Coordinate."

[[pane]]
role     = "Dev"
cli      = "claude"
split    = { direction = "right", from = 0 }
briefing = "Build."
"#;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("herdup-repo-team-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn write_team(project: &Path, text: &str) {
    let file = project.join(REPO_TEAM_FILE);
    std::fs::create_dir_all(file.parent().unwrap()).expect(".herdr");
    std::fs::write(file, text).expect("team.toml");
}

#[test]
fn a_valid_team_loads_under_the_repo_id() {
    let project = scratch("valid");
    write_team(&project, VALID);
    let team = load_repo_team(&project, &Registry::builtin())
        .expect("file exists")
        .expect("valid");
    assert_eq!(team.id, REPO_TEMPLATE_ID);
    assert_eq!(team.display_name, "Repo squad");
    assert_eq!(team.description, "two panes");
    assert_eq!(team.panes.len(), 2);
    assert_eq!(team.coordinator(), Some(0));
    std::fs::remove_dir_all(&project).unwrap();
}

#[test]
fn the_display_name_defaults_to_the_folder_name() {
    let project = scratch("named");
    let text = VALID.replacen("display_name = \"Repo squad\"\n", "", 1);
    write_team(&project, &text);
    let team = load_repo_team(&project, &Registry::builtin())
        .unwrap()
        .unwrap();
    assert_eq!(
        team.display_name,
        project.file_name().unwrap().to_string_lossy()
    );
    std::fs::remove_dir_all(&project).unwrap();
}

#[test]
fn no_file_means_none() {
    let project = scratch("absent");
    assert!(load_repo_team(&project, &Registry::builtin()).is_none());
    std::fs::remove_dir_all(&project).unwrap();
}

#[test]
fn a_wrapping_key_is_rejected_and_named() {
    let text = format!("[squad]\n{}", VALID.replace("[[pane]]", "[[squad.pane]]"));
    let err = parse_repo_team(
        &text,
        "team.toml",
        Path::new("/p/demo"),
        &Registry::builtin(),
    )
    .expect_err("a wrapping key is not the bare shape");
    let msg = err.to_string();
    assert!(msg.contains("squad"), "{msg}");
    assert!(matches!(err, ConfigError::Toml { .. }), "{err:?}");
}

#[test]
fn an_unknown_cli_is_rejected_by_role() {
    let text = VALID.replace("cli      = \"claude\"", "cli      = \"nope\"");
    let err = parse_repo_team(
        &text,
        "team.toml",
        Path::new("/p/demo"),
        &Registry::builtin(),
    )
    .expect_err("unknown cli");
    match err {
        ConfigError::UnknownCli {
            template,
            role,
            cli,
        } => {
            assert_eq!(template, REPO_TEMPLATE_ID);
            assert_eq!(role, "Dev");
            assert_eq!(cli, "nope");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_root_pane_may_not_split() {
    let text = VALID.replacen(
        "coordinator = true\n",
        "coordinator = true\nsplit = { direction = \"right\", from = 0 }\n",
        1,
    );
    let err = parse_repo_team(
        &text,
        "team.toml",
        Path::new("/p/demo"),
        &Registry::builtin(),
    )
    .expect_err("root split");
    assert!(
        matches!(err, ConfigError::RootPaneHasSplit { .. }),
        "{err:?}"
    );
}

#[test]
fn the_coordinator_must_be_first() {
    let text = VALID.replacen("coordinator = true\n", "", 1).replacen(
        "role     = \"Dev\"\n",
        "role     = \"Dev\"\ncoordinator = true\n",
        1,
    );
    let err = parse_repo_team(
        &text,
        "team.toml",
        Path::new("/p/demo"),
        &Registry::builtin(),
    )
    .expect_err("coordinator at 1");
    assert!(
        matches!(err, ConfigError::CoordinatorNotFirst { .. }),
        "{err:?}"
    );
}

#[test]
fn an_unreadable_file_is_an_error_not_none() {
    let project = scratch("unreadable");
    // A directory where the file should be: exists, cannot be read as text.
    std::fs::create_dir_all(project.join(REPO_TEAM_FILE)).unwrap();
    let outcome = load_repo_team(&project, &Registry::builtin()).expect("something is there");
    assert!(
        matches!(outcome, Err(ConfigError::Io { .. })),
        "{outcome:?}"
    );
    std::fs::remove_dir_all(&project).unwrap();
}

#[test]
fn with_repo_team_offers_it_under_repo_and_replaces_an_earlier_one() {
    let registry = Registry::builtin();
    let first = parse_repo_team(VALID, "team.toml", Path::new("/p/one"), &registry).unwrap();
    let second = parse_repo_team(
        &VALID.replace("Repo squad", "Second"),
        "team.toml",
        Path::new("/p/two"),
        &registry,
    )
    .unwrap();
    let templates = Templates::builtin()
        .with_repo_team(first)
        .with_repo_team(second);
    assert_eq!(
        templates.get(REPO_TEMPLATE_ID).unwrap().display_name,
        "Second"
    );
    assert_eq!(templates.len(), Templates::builtin().len() + 1);
}

use launcher_core::config::load_templates_for;
use launcher_core::plan::{plan, LaunchRequest};

#[test]
fn load_templates_for_merges_a_valid_team() {
    let project = scratch("merge");
    write_team(&project, VALID);
    let registry = Registry::builtin();
    let (templates, error) = load_templates_for(&project, &registry).expect("loads");
    assert!(error.is_none());
    assert!(templates.get(REPO_TEMPLATE_ID).is_some());
    assert!(templates.get("squad").is_some(), "built-ins are kept");
    std::fs::remove_dir_all(&project).unwrap();
}

#[test]
fn load_templates_for_reports_a_bad_team_and_keeps_the_builtins() {
    let project = scratch("bad");
    write_team(&project, "display_name = 1\n");
    let registry = Registry::builtin();
    let (templates, error) = load_templates_for(&project, &registry).expect("loads");
    assert!(templates.get(REPO_TEMPLATE_ID).is_none());
    assert!(templates.get("squad").is_some());
    let error = error.expect("the bad file is reported");
    assert!(error.to_string().contains("team.toml"), "{error}");
    std::fs::remove_dir_all(&project).unwrap();
}

#[test]
fn load_templates_for_without_a_file_is_just_the_templates() {
    let project = scratch("plain");
    let registry = Registry::builtin();
    let (templates, error) = load_templates_for(&project, &registry).expect("loads");
    assert!(error.is_none());
    assert!(templates.get(REPO_TEMPLATE_ID).is_none());
    std::fs::remove_dir_all(&project).unwrap();
}

#[test]
fn a_repo_team_plans_like_the_same_builtin() {
    // The duo shape, written as a repo team, must plan identically to duo.
    let registry = Registry::builtin();
    let builtin = Templates::builtin();
    let duo = builtin.get("duo").expect("duo exists");
    let mut text = String::from("display_name = \"Duo\"\ndescription = \"d\"\n");
    for pane in &duo.panes {
        text.push_str("\n[[pane]]\n");
        text.push_str(&format!(
            "role = {:?}\ncli = {:?}\nflags = {:?}\nbriefing = {:?}\n",
            pane.role, pane.cli, pane.flags, pane.briefing
        ));
        if pane.coordinator {
            text.push_str("coordinator = true\n");
        }
        if let Some(split) = &pane.split {
            // Serialise the split the way templates.toml writes it.
            let direction = format!("{:?}", split.direction).to_lowercase();
            match split.ratio {
                Some(r) => text.push_str(&format!(
                    "split = {{ direction = \"{direction}\", ratio = {r}, from = {} }}\n",
                    split.from
                )),
                None => text.push_str(&format!(
                    "split = {{ direction = \"{direction}\", from = {} }}\n",
                    split.from
                )),
            }
        }
    }
    let repo = parse_repo_team(&text, "team.toml", Path::new("/work/demo"), &registry).unwrap();
    let project = Path::new("/work/demo");
    let a = plan(&LaunchRequest::new(project, duo), &registry).unwrap();
    let b = plan(&LaunchRequest::new(project, &repo), &registry).unwrap();
    let shape = |p: &launcher_core::plan::LaunchPlan| {
        p.panes
            .iter()
            .map(|x| (x.role.clone(), x.cli.clone(), x.command.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(shape(&a), shape(&b));
}

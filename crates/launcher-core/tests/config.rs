//! Registry and template loading, merging and validation.
//!
//! All pure — no herdr, no processes.

use launcher_core::config::ConfigError;
use launcher_core::registry::{BriefingTrust, Registry};
use launcher_core::template::{flatten, Templates};

// ---------------------------------------------------------------------------
// built-ins
// ---------------------------------------------------------------------------

/// herdr ships detection manifests for exactly these agents
/// (`src/detect/manifests/*.toml`). Registry keys must match them, or herdr will
/// attribute a launched pane to the wrong agent — or to none.
const HERDR_MANIFEST_IDS: [&str; 18] = [
    "amp",
    "antigravity",
    "claude",
    "cline",
    "codex",
    "cursor",
    "devin",
    "droid",
    "gemini",
    "github-copilot",
    "grok",
    "hermes",
    "kilo",
    "kimi",
    "kiro",
    "opencode",
    "pi",
    "qodercli",
];

/// Agent kinds accepted by `herdr agent start --kind` on 0.8.2, from the
/// installed binary's own help output. A kind outside this list is rejected at
/// runtime, so shipping one would break launches for that CLI.
const HERDR_AGENT_KINDS: [&str; 23] = [
    "pi",
    "claude",
    "codex",
    "gemini",
    "cursor",
    "devin",
    "agy",
    "cline",
    "omp",
    "mastracode",
    "opencode",
    "copilot",
    "kimi",
    "kiro",
    "droid",
    "amp",
    "grok",
    "hermes",
    "kilo",
    "qodercli",
    "qwen",
    "maki",
    "muse",
];

#[test]
fn the_builtin_registry_covers_every_herdr_manifest_id() {
    // A superset: the registry also carries agent kinds that have no detection
    // manifest, so herdr's sidebar may not label them but `agent start` works.
    let reg = Registry::builtin();
    for id in HERDR_MANIFEST_IDS {
        assert!(reg.contains(id), "registry is missing herdr agent '{id}'");
    }
    assert_eq!(
        reg.len(),
        25,
        "a CLI was added or lost without updating this"
    );
}

#[test]
fn every_declared_kind_is_one_herdr_actually_accepts() {
    // `agent start --kind` rejects anything outside its list, so an invented
    // kind would fail every launch for that CLI.
    let reg = Registry::builtin();
    for entry in reg.iter() {
        if let Some(kind) = &entry.kind {
            assert!(
                HERDR_AGENT_KINDS.contains(&kind.as_str()),
                "{} declares kind '{kind}', which herdr does not accept",
                entry.id
            );
        }
    }
}

#[test]
fn every_herdr_agent_kind_is_reachable_from_some_registry_entry() {
    // Otherwise a CLI herdr can drive would be unavailable in herdup for no
    // reason. `agy` in particular was missing until the agent API was adopted.
    let reg = Registry::builtin();
    for kind in HERDR_AGENT_KINDS {
        assert!(
            reg.iter().any(|e| e.kind.as_deref() == Some(kind)),
            "no registry entry offers herdr kind '{kind}'"
        );
    }
}

#[test]
fn a_cli_herdr_cannot_manage_as_an_agent_has_no_kind() {
    // antigravity has a detection manifest but is not an `agent start` kind.
    let reg = Registry::builtin();
    let entry = reg.get("antigravity").expect("present");
    assert!(entry.kind.is_none());
    assert!(!entry.has_agent_kind());
}

#[test]
fn only_verified_clis_may_be_auto_briefed() {
    // The Phase 0 guarantee, encoded. Promoting a CLI to `verified` means
    // someone reproduced its blocked -> idle transition by hand. Anything else
    // must require a human to look at the pane before herdup types into it.
    let reg = Registry::builtin();
    let auto: Vec<&str> = reg.auto_briefable().map(|e| e.id.as_str()).collect();
    assert_eq!(
        auto,
        vec!["claude"],
        "only hand-verified CLIs may auto-brief; adding one here needs a real test first"
    );

    // Gemini specifically: it reported `idle` behind a blocking modal.
    let gemini = reg.get("gemini").expect("gemini present");
    assert_eq!(gemini.briefing_trust, BriefingTrust::Manual);
    assert!(!gemini.briefing_trust.may_auto_brief());
}

/// Presets verified by the agreed bar: the flag in `--help`, then a real launch
/// reaching a ready prompt in an isolated herdr 0.8.2 session.
///
/// claude: 2026-09-02. hermes and agy: 2026-09-03, macOS; neither has an
/// edits-only flag. gemini, cursor, kilo and pi ship nothing: gemini's
/// `--approval-mode` exists but the launch was blocked by trust and auth
/// dialogs, cursor needs a login, and kilo and pi have no such flag.
const VERIFIED_PRESETS: &[(&str, &[&str])] = &[
    (
        "claude",
        &[
            "--permission-mode bypassPermissions",
            "--permission-mode acceptEdits",
            "",
        ],
    ),
    ("hermes", &["--yolo", ""]),
    ("agy", &["--dangerously-skip-permissions", ""]),
];

#[test]
fn unverified_clis_ship_no_invented_flag_presets() {
    // A wrong permission flag fails silently and could disable a sandbox, so
    // presets are only shipped where verified. A wrong *binary* name, by
    // contrast, fails loudly at preflight and is safe to ship best-effort.
    let reg = Registry::builtin();
    for entry in reg.iter() {
        assert!(!entry.binary.is_empty(), "{} has no binary", entry.id);
        let got: Vec<&str> = entry.flag_presets.iter().map(String::as_str).collect();
        match VERIFIED_PRESETS.iter().find(|(id, _)| *id == entry.id) {
            Some((_, expected)) => assert_eq!(
                got, *expected,
                "{} ships presets other than the verified ones",
                entry.id
            ),
            None => assert_eq!(got, [""], "{} ships flag presets nobody verified", entry.id),
        }
    }
    for (id, _) in VERIFIED_PRESETS {
        assert!(reg.get(id).is_some(), "{id} is verified but not registered");
    }
}

#[test]
fn builtin_templates_load_and_reference_only_known_clis() {
    let reg = Registry::builtin();
    let templates = Templates::builtin();
    templates
        .validate_against(&reg)
        .expect("every built-in template names a registered cli");

    for id in ["solo", "duo", "squad", "full-team"] {
        assert!(templates.get(id).is_some(), "missing template '{id}'");
    }
    assert_eq!(templates.len(), 4);
}

#[test]
fn builtin_template_shapes_match_the_spec() {
    let t = Templates::builtin();
    assert_eq!(t.get("solo").unwrap().panes.len(), 1);
    assert_eq!(t.get("duo").unwrap().panes.len(), 2);
    assert_eq!(t.get("squad").unwrap().panes.len(), 4);
    assert_eq!(t.get("full-team").unwrap().panes.len(), 6);

    // Coordinator holds the root pane, so it is created first even though it is
    // briefed last.
    assert_eq!(t.get("squad").unwrap().coordinator(), Some(0));
    assert_eq!(t.get("full-team").unwrap().coordinator(), Some(0));
    // Small templates have no coordinator to coordinate.
    assert_eq!(t.get("solo").unwrap().coordinator(), None);
    assert_eq!(t.get("duo").unwrap().coordinator(), None);

    let full = t.get("full-team").unwrap();
    let roles: Vec<&str> = full.panes.iter().map(|p| p.role.as_str()).collect();
    assert_eq!(
        roles,
        vec!["PM", "Coder 1", "Coder 2", "QA", "Builds", "Research"]
    );
}

#[test]
fn distinct_clis_deduplicates_because_sign_in_is_per_cli() {
    // Four panes, all claude -> one login and one trust answer, not four.
    let squad = Templates::builtin();
    let squad = squad.get("squad").unwrap();
    assert_eq!(squad.panes.len(), 4);
    assert_eq!(squad.distinct_clis(), vec!["claude"]);
}

// ---------------------------------------------------------------------------
// briefing handling
// ---------------------------------------------------------------------------

#[test]
fn briefings_flatten_to_exactly_one_line() {
    // Most agent CLIs submit on newline, so a multi-line briefing would fire as
    // several truncated prompts.
    let templates = Templates::builtin();
    for template in templates.iter() {
        for pane in &template.panes {
            let flat = pane.flattened_briefing();
            assert!(
                !flat.contains('\n') && !flat.contains('\r'),
                "{}/{} briefing still contains a newline",
                template.id,
                pane.role
            );
            assert!(
                !flat.is_empty(),
                "{}/{} has an empty briefing",
                template.id,
                pane.role
            );
            assert!(
                !flat.contains("  "),
                "{}/{} has a double space",
                template.id,
                pane.role
            );
        }
    }
}

#[test]
fn flatten_collapses_all_whitespace_runs() {
    assert_eq!(
        flatten("one\ntwo\r\n\tthree   four\n\n"),
        "one two three four"
    );
    assert_eq!(flatten("   "), "");
}

#[test]
fn command_joins_binary_and_flags_and_omits_empty_flags() {
    let t = Templates::builtin();
    let squad = t.get("squad").unwrap();
    let pm = &squad.panes[0];
    assert_eq!(
        pm.command("claude"),
        "claude --permission-mode bypassPermissions"
    );

    let solo = t.get("solo").unwrap();
    let mut bare = solo.panes[0].clone();
    bare.flags = String::new();
    assert_eq!(bare.command("claude"), "claude");
    bare.flags = "   ".into();
    assert_eq!(
        bare.command("claude"),
        "claude",
        "whitespace-only flags are no flags"
    );
}

// ---------------------------------------------------------------------------
// user overrides
// ---------------------------------------------------------------------------

#[test]
fn a_user_override_replaces_only_the_fields_it_names() {
    let reg = Registry::builtin()
        .with_user_overrides(
            r#"
            [claude]
            flag_presets = ["--permission-mode plan"]
            "#,
            "user registry.toml",
        )
        .expect("merges");

    let claude = reg.get("claude").unwrap();
    assert_eq!(claude.flag_presets, vec!["--permission-mode plan"]);
    // Untouched fields survive, so upgrading the built-ins does not clobber edits.
    assert_eq!(claude.display_name, "Claude Code");
    assert_eq!(claude.binary, "claude");
    assert_eq!(claude.briefing_trust, BriefingTrust::Verified);
    assert!(claude.install_command().is_some());
}

#[test]
fn a_user_may_fix_a_wrong_binary_name_without_restating_the_entry() {
    // The intended remedy when a best-effort base name is wrong on some machine.
    let reg = Registry::builtin()
        .with_user_overrides("[codex]\nbinary = \"codex-cli\"\n", "user")
        .expect("merges");
    assert_eq!(reg.get("codex").unwrap().binary, "codex-cli");
    assert_eq!(reg.get("codex").unwrap().display_name, "Codex");
}

#[test]
fn a_user_can_add_a_cli_the_builtins_do_not_know() {
    let reg = Registry::builtin()
        .with_user_overrides(
            r#"
            [mytool]
            display_name = "My Tool"
            binary = "mytool"
            "#,
            "user",
        )
        .expect("merges");

    let added = reg.get("mytool").expect("added");
    assert_eq!(added.display_name, "My Tool");
    // An untested CLI defaults to the cautious tier.
    assert_eq!(added.briefing_trust, BriefingTrust::Manual);
    assert_eq!(reg.len(), Registry::builtin().len() + 1);
}

#[test]
fn a_new_cli_missing_required_fields_is_rejected_by_name() {
    let err = Registry::builtin()
        .with_user_overrides("[mytool]\nflag_presets = [\"-x\"]\n", "user")
        .expect_err("should reject");
    match err {
        ConfigError::NewEntryIncomplete { ref id, .. } => assert_eq!(id, "mytool"),
        other => panic!("expected NewEntryIncomplete, got {other:?}"),
    }
    assert!(err.to_string().contains("mytool"));
}

#[test]
fn a_misspelled_key_is_rejected_rather_than_silently_ignored() {
    // Silently dropping `briefing_trusted` would leave the user believing they
    // had granted auto-briefing when they had not.
    let err = Registry::builtin()
        .with_user_overrides(
            "[claude]\nbriefing_trusted = \"verified\"\n",
            "user registry.toml",
        )
        .expect_err("should reject");
    let msg = err.to_string();
    assert!(matches!(err, ConfigError::Toml { .. }), "got {err:?}");
    assert!(
        msg.contains("user registry.toml"),
        "error names the file: {msg}"
    );
    assert!(
        msg.contains("briefing_trusted"),
        "error names the bad key: {msg}"
    );
}

#[test]
fn a_user_template_replaces_a_builtin_wholesale() {
    let templates = Templates::builtin()
        .with_user_overrides(
            r#"
            [solo]
            display_name = "My Solo"
            description = "Just me."
            [[solo.pane]]
            role = "Dev"
            cli = "codex"
            briefing = "Do the thing."
            "#,
            "user templates.toml",
        )
        .expect("merges");

    let solo = templates.get("solo").unwrap();
    assert_eq!(solo.display_name, "My Solo");
    assert_eq!(solo.panes[0].cli, "codex");
    // Other templates are untouched.
    assert_eq!(templates.get("squad").unwrap().panes.len(), 4);
    assert_eq!(templates.len(), 4);
}

#[test]
fn a_template_naming_an_unknown_cli_is_rejected_by_role() {
    let templates = Templates::from_toml(
        r#"
        [t]
        display_name = "T"
        description = "d"
        [[t.pane]]
        role = "Dev"
        cli = "not-a-real-cli"
        briefing = "hi"
        "#,
        "user",
    )
    .expect("parses structurally");

    match templates.validate_against(&Registry::builtin()) {
        Err(ConfigError::UnknownCli { role, cli, .. }) => {
            assert_eq!(role, "Dev");
            assert_eq!(cli, "not-a-real-cli");
        }
        other => panic!("expected UnknownCli, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// layout invariants
// ---------------------------------------------------------------------------

/// Build a one-off template body so each invariant can be violated in isolation.
fn parse(panes: &str) -> Result<Templates, ConfigError> {
    Templates::from_toml(
        &format!("[t]\ndisplay_name = \"T\"\ndescription = \"d\"\n{panes}"),
        "user templates.toml",
    )
}

#[test]
fn the_root_pane_must_not_declare_a_split() {
    // Pane 0 is created by `workspace create`, not by splitting something else.
    let err = parse(
        r#"
        [[t.pane]]
        role = "Dev"
        cli = "claude"
        briefing = "b"
        split = { direction = "right", from = 0 }
        "#,
    )
    .expect_err("should reject");
    assert!(
        matches!(err, ConfigError::RootPaneHasSplit { .. }),
        "got {err:?}"
    );
    assert!(err.to_string().contains("Dev"));
}

#[test]
fn every_pane_after_the_first_must_declare_a_split() {
    let err = parse(
        r#"
        [[t.pane]]
        role = "Dev"
        cli = "claude"
        briefing = "b"

        [[t.pane]]
        role = "QA"
        cli = "claude"
        briefing = "b"
        "#,
    )
    .expect_err("should reject");
    match err {
        ConfigError::MissingSplit { role, index, .. } => {
            assert_eq!(role, "QA");
            assert_eq!(index, 1);
        }
        other => panic!("expected MissingSplit, got {other:?}"),
    }
}

#[test]
fn a_split_must_reference_an_earlier_pane() {
    // Splitting from a later pane would reference something that does not exist
    // yet when the plan executes.
    let err = parse(
        r#"
        [[t.pane]]
        role = "Dev"
        cli = "claude"
        briefing = "b"

        [[t.pane]]
        role = "QA"
        cli = "claude"
        briefing = "b"
        split = { direction = "down", from = 2 }
        "#,
    )
    .expect_err("should reject");
    match err {
        ConfigError::SplitFromNotEarlier { index, from, .. } => {
            assert_eq!((index, from), (1, 2));
        }
        other => panic!("expected SplitFromNotEarlier, got {other:?}"),
    }
}

#[test]
fn a_pane_may_not_split_from_itself() {
    let err = parse(
        r#"
        [[t.pane]]
        role = "Dev"
        cli = "claude"
        briefing = "b"

        [[t.pane]]
        role = "QA"
        cli = "claude"
        briefing = "b"
        split = { direction = "down", from = 1 }
        "#,
    )
    .expect_err("should reject");
    assert!(
        matches!(err, ConfigError::SplitFromNotEarlier { from: 1, .. }),
        "got {err:?}"
    );
}

#[test]
fn the_coordinator_must_be_pane_zero() {
    let err = parse(
        r#"
        [[t.pane]]
        role = "Dev"
        cli = "claude"
        briefing = "b"

        [[t.pane]]
        role = "PM"
        cli = "claude"
        briefing = "b"
        coordinator = true
        split = { direction = "right", from = 0 }
        "#,
    )
    .expect_err("should reject");
    match err {
        ConfigError::CoordinatorNotFirst { role, index, .. } => {
            assert_eq!(role, "PM");
            assert_eq!(index, 1);
        }
        other => panic!("expected CoordinatorNotFirst, got {other:?}"),
    }
}

#[test]
fn two_coordinators_are_rejected() {
    let err = parse(
        r#"
        [[t.pane]]
        role = "PM"
        cli = "claude"
        briefing = "b"
        coordinator = true

        [[t.pane]]
        role = "PM2"
        cli = "claude"
        briefing = "b"
        coordinator = true
        split = { direction = "right", from = 0 }
        "#,
    )
    .expect_err("should reject");
    // The second one trips CoordinatorNotFirst before the duplicate check,
    // which is still a correct rejection naming the offending pane.
    let msg = err.to_string();
    assert!(
        msg.contains("PM2"),
        "error should name the offending pane: {msg}"
    );
}

#[test]
fn an_empty_template_is_rejected() {
    let err = Templates::from_toml(
        "[t]\ndisplay_name = \"T\"\ndescription = \"d\"\npane = []\n",
        "user",
    )
    .expect_err("should reject");
    assert!(
        matches!(err, ConfigError::EmptyTemplate { .. }),
        "got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// on-disk loading
// ---------------------------------------------------------------------------

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("herdup-cfg-{}-{}", std::process::id(), name));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

#[test]
fn a_missing_user_config_directory_falls_back_to_builtins() {
    // The normal first-run case must not be an error.
    let reg = launcher_core::config::load_registry_from(None).expect("loads");
    assert_eq!(reg.len(), Registry::builtin().len());
    let t = launcher_core::config::load_templates_from(None, &reg).expect("loads");
    assert_eq!(t.len(), 4);
}

#[test]
fn an_empty_config_directory_falls_back_to_builtins() {
    let dir = temp_dir("empty");
    let reg = launcher_core::config::load_registry_from(Some(&dir)).expect("loads");
    assert_eq!(reg.len(), Registry::builtin().len());
}

#[test]
fn user_files_on_disk_are_merged_and_validated_together() {
    let dir = temp_dir("merged");
    std::fs::write(
        dir.join("registry.toml"),
        "[mytool]\ndisplay_name = \"My Tool\"\nbinary = \"mytool\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("templates.toml"),
        "[mine]\ndisplay_name = \"Mine\"\ndescription = \"d\"\n\
         [[mine.pane]]\nrole = \"Dev\"\ncli = \"mytool\"\nbriefing = \"go\"\n",
    )
    .unwrap();

    let reg = launcher_core::config::load_registry_from(Some(&dir)).expect("registry loads");
    assert_eq!(reg.len(), Registry::builtin().len() + 1);

    // The user's template names the user's CLI: validation must see both.
    let t = launcher_core::config::load_templates_from(Some(&dir), &reg).expect("templates load");
    assert_eq!(t.len(), 5);
    assert_eq!(t.get("mine").unwrap().panes[0].cli, "mytool");
}

#[test]
fn a_user_template_naming_a_cli_they_did_not_define_is_rejected_at_load() {
    let dir = temp_dir("badref");
    std::fs::write(
        dir.join("templates.toml"),
        "[mine]\ndisplay_name = \"Mine\"\ndescription = \"d\"\n\
         [[mine.pane]]\nrole = \"Dev\"\ncli = \"nope\"\nbriefing = \"go\"\n",
    )
    .unwrap();

    let reg = launcher_core::config::load_registry_from(Some(&dir)).expect("registry loads");
    let err = launcher_core::config::load_templates_from(Some(&dir), &reg).expect_err("rejects");
    assert!(matches!(err, ConfigError::UnknownCli { .. }), "got {err:?}");
}

#[test]
fn config_dir_is_platform_appropriate() {
    if let Some(dir) = launcher_core::config::config_dir() {
        assert!(dir.ends_with("herdup"), "got {}", dir.display());
        if cfg!(windows) {
            assert!(dir.to_string_lossy().contains("AppData"));
        } else {
            assert!(dir.to_string_lossy().contains("Application Support"));
        }
    }
}

#[test]
fn a_valid_multi_level_layout_is_accepted() {
    parse(
        r#"
        [[t.pane]]
        role = "PM"
        cli = "claude"
        briefing = "b"
        coordinator = true

        [[t.pane]]
        role = "A"
        cli = "claude"
        briefing = "b"
        split = { direction = "right", ratio = 0.5, from = 0 }

        [[t.pane]]
        role = "B"
        cli = "claude"
        briefing = "b"
        split = { direction = "down", ratio = 0.5, from = 1 }
        "#,
    )
    .expect("a well-formed tree is accepted");
}

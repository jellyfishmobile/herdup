//! Plan generation. Pure — no herdr, no processes, no clock.

use launcher_core::plan::{
    plan, BriefingGate, BriefingText, LaunchRequest, PaneRef, PlanError, Step,
};
use launcher_core::registry::Registry;
use launcher_core::template::Templates;
use std::path::Path;

fn project() -> &'static Path {
    Path::new("D:\\work\\herdup")
}

/// Compact one-line rendering of a step, for sequence assertions.
fn render(step: &Step) -> String {
    match step {
        Step::CreateWorkspace { .. } => "create-workspace".into(),
        Step::SplitPane {
            from,
            creates,
            direction,
            ..
        } => format!("split {}->{} {}", from.0, creates.0, direction.as_str()),
        Step::RenamePane { pane, label } => format!("rename {} {label}", pane.0),
        Step::StartAgent { pane, name, .. } => format!("start {} {}", pane.0, name),
        Step::RunCommand { pane, .. } => format!("run {}", pane.0),
        Step::SendBriefing { pane, .. } => format!("brief {}", pane.0),
    }
}

fn steps_of(id: &str) -> Vec<String> {
    let reg = Registry::builtin();
    let templates = Templates::builtin();
    let t = templates.get(id).expect("template exists");
    let p = plan(&LaunchRequest::new(project(), t), &reg).expect("plans");
    p.steps.iter().map(render).collect()
}

// ---------------------------------------------------------------------------
// step sequences
// ---------------------------------------------------------------------------

#[test]
fn the_squad_plan_is_exactly_this_sequence() {
    assert_eq!(
        steps_of("squad"),
        vec![
            "create-workspace",
            // Create, rename and start every pane in template order. There is
            // no separate readiness step: `agent start` returns only once herdr
            // sees the agent ready for input.
            "rename 0 PM",
            "start 0 pm",
            "split 0->1 right",
            "rename 1 Coder 1",
            "start 1 coder-1",
            "split 1->2 down",
            "rename 2 Coder 2",
            "start 2 coder-2",
            "split 0->3 down",
            "rename 3 QA",
            "start 3 qa",
            // Then brief everyone but the coordinator...
            "brief 1",
            "brief 2",
            "brief 3",
            // ...and the coordinator last, once the roster is known.
            "brief 0",
        ]
    );
}

#[test]
fn agent_names_are_herdr_legal_and_unique() {
    // herdr requires [a-z][a-z0-9_-]{0,31} and uniqueness among live agents.
    let reg = Registry::builtin();
    let templates = Templates::builtin();
    let p = plan(
        &LaunchRequest::new(project(), templates.get("full-team").unwrap()),
        &reg,
    )
    .expect("plans");

    let names: Vec<&str> = p
        .panes
        .iter()
        .map(|x| x.agent_name.as_deref().expect("claude has an agent kind"))
        .collect();
    assert_eq!(
        names,
        vec!["pm", "coder-1", "coder-2", "qa", "builds", "research"]
    );

    for name in &names {
        let mut chars = name.chars();
        assert!(
            chars.next().is_some_and(|c| c.is_ascii_lowercase()),
            "{name} must start with a lowercase letter"
        );
        assert!(
            name.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_'),
            "{name} has an illegal character"
        );
        assert!(name.len() <= 32, "{name} is too long");
    }

    let mut sorted = names.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), names.len(), "names must be unique");
}

#[test]
fn a_cli_without_a_herdr_agent_kind_falls_back_and_is_never_auto_briefed() {
    // antigravity is detected by herdr but is not an `agent start` kind, so
    // there is no readiness signal and no `agent_blocked` guard.
    let reg = Registry::builtin();
    assert!(
        reg.get("antigravity").unwrap().kind.is_none(),
        "fixture assumption: antigravity has no agent kind"
    );
    let templates = Templates::builtin();
    let p = plan(
        &LaunchRequest::new(project(), templates.get("solo").unwrap())
            .override_cli(0, "antigravity"),
        &reg,
    )
    .expect("plans");

    assert!(p.panes[0].agent_name.is_none());
    assert_eq!(p.panes[0].gate, BriefingGate::RequiresHuman);
    assert!(p.steps.iter().any(|s| matches!(s, Step::RunCommand { .. })));
    assert!(!p.steps.iter().any(|s| matches!(s, Step::StartAgent { .. })));
}

#[test]
fn template_flags_become_agent_args_after_the_separator() {
    let reg = Registry::builtin();
    let templates = Templates::builtin();
    let p = plan(
        &LaunchRequest::new(project(), templates.get("solo").unwrap()),
        &reg,
    )
    .expect("plans");

    let args = p
        .steps
        .iter()
        .find_map(|s| match s {
            Step::StartAgent { args, .. } => Some(args.clone()),
            _ => None,
        })
        .expect("solo starts an agent");
    assert_eq!(args, vec!["--permission-mode", "bypassPermissions"]);
}

#[test]
fn solo_has_no_split_and_no_coordinator_step_ordering_to_worry_about() {
    assert_eq!(
        steps_of("solo"),
        vec!["create-workspace", "rename 0 Dev", "start 0 dev", "brief 0",]
    );
}

#[test]
fn every_builtin_template_plans_without_error() {
    let reg = Registry::builtin();
    let templates = Templates::builtin();
    for t in templates.iter() {
        let p = plan(&LaunchRequest::new(project(), t), &reg)
            .unwrap_or_else(|e| panic!("template '{}' failed to plan: {e}", t.id));
        assert_eq!(p.panes.len(), t.panes.len());
        // Every pane gets renamed and started, and every pane gets briefed.
        let renames = p
            .steps
            .iter()
            .filter(|s| matches!(s, Step::RenamePane { .. }))
            .count();
        let briefs = p
            .steps
            .iter()
            .filter(|s| matches!(s, Step::SendBriefing { .. }))
            .count();
        assert_eq!(renames, t.panes.len(), "{}", t.id);
        assert_eq!(briefs, t.panes.len(), "{}", t.id);
    }
}

// ---------------------------------------------------------------------------
// coordinator ordering
// ---------------------------------------------------------------------------

#[test]
fn the_coordinator_pane_is_created_first_but_briefed_last() {
    // The whole point of the ordering: it holds the root pane everything else
    // splits from, yet its briefing names the finished team.
    let steps = steps_of("full-team");

    let first_start = steps.iter().position(|s| s.starts_with("start ")).unwrap();
    assert_eq!(steps[first_start], "start 0 pm", "coordinator starts first");

    let last_brief = steps.iter().rposition(|s| s.starts_with("brief ")).unwrap();
    assert_eq!(steps[last_brief], "brief 0", "coordinator is briefed last");
    assert_eq!(last_brief, steps.len() - 1);

    // No other pane is briefed after it.
    assert_eq!(steps.iter().filter(|s| *s == "brief 0").count(), 1);
}

#[test]
fn the_coordinator_briefing_names_every_teammate_with_role_and_cli() {
    let reg = Registry::builtin();
    let templates = Templates::builtin();
    let t = templates.get("full-team").unwrap();
    let p = plan(&LaunchRequest::new(project(), t), &reg).expect("plans");

    let coord = p.coordinator().expect("full-team has a coordinator");
    assert_eq!(coord.role, "PM");

    let briefing = p
        .steps
        .iter()
        .find_map(|s| match s {
            Step::SendBriefing { pane, text, .. } if *pane == coord.pane => Some(text),
            _ => None,
        })
        .expect("coordinator is briefed");

    let BriefingText::Coordinator { roster, .. } = briefing else {
        panic!("coordinator should get a roster briefing, not a literal one");
    };
    // Five teammates, and the coordinator is not on its own roster.
    assert_eq!(roster.len(), 5);
    assert!(!roster.iter().any(|r| r.role == "PM"));

    // Rendered with real ids, it names each teammate, their pane and their CLI.
    let rendered = briefing.render(&|r: PaneRef| format!("w1:p{}", r.0 + 1));
    for role in ["Coder 1", "Coder 2", "QA", "Builds", "Research"] {
        assert!(rendered.contains(role), "briefing omits {role}: {rendered}");
    }
    assert!(
        rendered.contains("w1:p2"),
        "briefing omits a resolved pane id"
    );
    assert!(
        rendered.contains("Claude Code"),
        "briefing omits the CLI name"
    );
    // Teammates are addressed by agent name, which herdr resolves to whatever
    // pane the agent currently occupies.
    for name in ["coder-1", "coder-2", "qa", "builds", "research"] {
        assert!(rendered.contains(name), "briefing omits agent name {name}");
    }
    assert!(rendered.contains("AGENT NAME"));

    // Every command it names must exist on herdr 0.8.2. An earlier version of
    // this briefing told the coordinator to run `herdr wait agent-status`,
    // which is not a command — the same mistake the executor was making.
    for command in [
        "herdr agent read",
        "herdr agent prompt",
        "herdr agent get",
        "herdr agent list",
    ] {
        assert!(rendered.contains(command), "briefing omits {command}");
    }
    assert!(
        !rendered.contains("wait agent-status"),
        "briefing names a command herdr does not have"
    );
    // And it must tell the coordinator what to do when herdr refuses.
    assert!(rendered.contains("agent_blocked"));
}

#[test]
fn every_briefing_renders_to_exactly_one_line() {
    // Most agent CLIs submit on newline, so any newline would truncate.
    let reg = Registry::builtin();
    let templates = Templates::builtin();
    for t in templates.iter() {
        let p = plan(&LaunchRequest::new(project(), t), &reg).expect("plans");
        for step in &p.steps {
            if let Step::SendBriefing { text, pane, .. } = step {
                let rendered = text.render(&|r: PaneRef| format!("w1:p{}", r.0));
                assert!(
                    !rendered.contains('\n') && !rendered.contains('\r'),
                    "{}/{} briefing has a newline",
                    t.id,
                    pane.0
                );
                assert!(
                    !rendered.contains("  "),
                    "{}/{} has a double space",
                    t.id,
                    pane.0
                );
                assert!(!rendered.is_empty());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// briefing gates — the Phase 0 finding, enforced in the plan
// ---------------------------------------------------------------------------

#[test]
fn a_verified_cli_is_gated_auto_and_an_unverified_one_is_not() {
    let reg = Registry::builtin();
    let templates = Templates::builtin();
    let t = templates.get("duo").unwrap();

    // Built-in duo is all claude, which is the one verified CLI.
    let p = plan(&LaunchRequest::new(project(), t), &reg).expect("plans");
    assert!(p.panes.iter().all(|pane| pane.gate.is_auto()));
    assert!(p.requires_human_briefing().is_empty());

    // Swap the reviewer to gemini — the CLI that reported idle behind a modal.
    let p = plan(
        &LaunchRequest::new(project(), t).override_cli(1, "gemini"),
        &reg,
    )
    .expect("plans");
    assert!(p.panes[0].gate.is_auto(), "claude still auto");
    assert_eq!(
        p.panes[1].gate,
        BriefingGate::RequiresHuman,
        "gemini must never be auto-briefed"
    );

    let manual = p.requires_human_briefing();
    assert_eq!(manual.len(), 1);
    assert_eq!(manual[0].role, "Reviewer");

    // And the gate travels on the step the executor will read.
    let gates: Vec<BriefingGate> = p
        .steps
        .iter()
        .filter_map(|s| match s {
            Step::SendBriefing { gate, .. } => Some(*gate),
            _ => None,
        })
        .collect();
    assert!(gates.contains(&BriefingGate::RequiresHuman));
}

#[test]
fn describe_marks_which_briefings_wait_for_a_human() {
    let reg = Registry::builtin();
    let templates = Templates::builtin();
    let t = templates.get("duo").unwrap();
    let p = plan(
        &LaunchRequest::new(project(), t).override_cli(1, "gemini"),
        &reg,
    )
    .expect("plans");

    let text = p.describe();
    assert!(
        text.contains("WAITS FOR YOU"),
        "dry run must be honest: {text}"
    );
    assert!(text.contains("brief #0 [automatic]") || text.contains("automatic"));
}

// ---------------------------------------------------------------------------
// preflight remediations: dropping panes and swapping CLIs
// ---------------------------------------------------------------------------

#[test]
fn dropping_a_middle_pane_repoints_its_children_to_its_parent() {
    // squad layout: PM(0) <- Coder 1(1) <- Coder 2(2), and PM(0) <- QA(3).
    // Dropping Coder 1 orphans Coder 2, which must re-attach to PM.
    let reg = Registry::builtin();
    let templates = Templates::builtin();
    let t = templates.get("squad").unwrap();

    let p = plan(&LaunchRequest::new(project(), t).skip_pane(1), &reg).expect("plans");

    let roles: Vec<&str> = p.panes.iter().map(|x| x.role.as_str()).collect();
    assert_eq!(roles, vec!["PM", "Coder 2", "QA"]);

    let splits: Vec<(usize, usize)> = p
        .steps
        .iter()
        .filter_map(|s| match s {
            Step::SplitPane { from, creates, .. } => Some((from.0, creates.0)),
            _ => None,
        })
        .collect();
    // Coder 2 (now #1) re-attached to PM (#0) instead of the dropped pane.
    assert_eq!(splits, vec![(0, 1), (0, 2)]);
}

#[test]
fn dropping_the_root_pane_promotes_the_next_survivor_to_root() {
    let reg = Registry::builtin();
    let templates = Templates::builtin();
    let t = templates.get("squad").unwrap();

    let p = plan(&LaunchRequest::new(project(), t).skip_pane(0), &reg).expect("plans");

    let roles: Vec<&str> = p.panes.iter().map(|x| x.role.as_str()).collect();
    assert_eq!(roles, vec!["Coder 1", "Coder 2", "QA"]);
    // The new root is created by `workspace create`, so it is never split.
    let splits: Vec<(usize, usize)> = p
        .steps
        .iter()
        .filter_map(|s| match s {
            Step::SplitPane { from, creates, .. } => Some((from.0, creates.0)),
            _ => None,
        })
        .collect();
    assert_eq!(splits, vec![(0, 1), (0, 2)]);
    assert!(
        !splits.iter().any(|(_, c)| *c == 0),
        "root is never split into"
    );

    // With the coordinator gone, nothing is briefed last for roster reasons.
    assert!(p.coordinator().is_none());
    let last = p.steps.last().unwrap();
    assert!(matches!(last, Step::SendBriefing { .. }));
}

#[test]
fn dropping_every_pane_is_an_error_not_an_empty_plan() {
    let reg = Registry::builtin();
    let templates = Templates::builtin();
    let t = templates.get("duo").unwrap();
    let err = plan(
        &LaunchRequest::new(project(), t).skip_pane(0).skip_pane(1),
        &reg,
    )
    .expect_err("should refuse");
    assert!(matches!(err, PlanError::NothingToLaunch), "got {err:?}");
}

#[test]
fn skipping_a_pane_that_does_not_exist_is_rejected() {
    let reg = Registry::builtin();
    let templates = Templates::builtin();
    let t = templates.get("solo").unwrap();
    let err = plan(&LaunchRequest::new(project(), t).skip_pane(7), &reg).expect_err("rejects");
    match err {
        PlanError::SkipOutOfRange { index, count } => assert_eq!((index, count), (7, 1)),
        other => panic!("expected SkipOutOfRange, got {other:?}"),
    }
}

#[test]
fn overriding_a_cli_changes_the_command_that_will_be_run() {
    let reg = Registry::builtin();
    let templates = Templates::builtin();
    let t = templates.get("solo").unwrap();

    let p = plan(
        &LaunchRequest::new(project(), t).override_cli(0, "codex"),
        &reg,
    )
    .expect("plans");
    assert_eq!(p.panes[0].cli, "codex");
    assert_eq!(p.panes[0].binary, "codex");
    assert!(p.panes[0].command.starts_with("codex"));
}

#[test]
fn swapping_a_cli_discards_flags_the_new_cli_is_not_known_to_accept() {
    // `--permission-mode acceptEdits` is a Claude Code flag. Handing it to
    // Gemini would at best fail to start. Same rule as the registry: never use
    // a flag nobody verified for that CLI.
    let reg = Registry::builtin();
    let templates = Templates::builtin();
    let t = templates.get("duo").unwrap();
    assert_eq!(t.panes[1].flags, "--permission-mode acceptEdits");

    let p = plan(
        &LaunchRequest::new(project(), t).override_cli(1, "gemini"),
        &reg,
    )
    .expect("plans");

    assert_eq!(p.panes[1].command, "gemini", "no claude flags leak across");
    assert_eq!(
        p.panes[1].dropped_flags.as_deref(),
        Some("--permission-mode acceptEdits"),
        "the drop is recorded, not silent"
    );
}

#[test]
fn flags_survive_when_the_new_cli_lists_them_as_a_preset() {
    // Swapping between two CLIs that genuinely share a flag keeps it.
    let reg = Registry::builtin()
        .with_user_overrides(
            "[codex]\nflag_presets = [\"--permission-mode acceptEdits\", \"\"]\n",
            "test",
        )
        .expect("merges");
    let templates = Templates::builtin();
    let t = templates.get("duo").unwrap();

    let p = plan(
        &LaunchRequest::new(project(), t).override_cli(1, "codex"),
        &reg,
    )
    .expect("plans");
    assert_eq!(p.panes[1].command, "codex --permission-mode acceptEdits");
    assert_eq!(p.panes[1].dropped_flags, None);
}

#[test]
fn flags_are_untouched_when_the_cli_is_not_swapped() {
    let reg = Registry::builtin();
    let templates = Templates::builtin();
    let t = templates.get("duo").unwrap();
    let p = plan(&LaunchRequest::new(project(), t), &reg).expect("plans");
    assert_eq!(p.panes[1].command, "claude --permission-mode acceptEdits");
    assert!(p.panes.iter().all(|x| x.dropped_flags.is_none()));
}

#[test]
fn overriding_to_an_unknown_cli_fails_the_whole_plan() {
    // Better a rejected plan than a half-usable one.
    let reg = Registry::builtin();
    let templates = Templates::builtin();
    let t = templates.get("solo").unwrap();
    let err = plan(
        &LaunchRequest::new(project(), t).override_cli(0, "nope"),
        &reg,
    )
    .expect_err("rejects");
    match err {
        PlanError::UnknownCli { role, cli } => {
            assert_eq!(role, "Dev");
            assert_eq!(cli, "nope");
        }
        other => panic!("expected UnknownCli, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// misc
// ---------------------------------------------------------------------------

#[test]
fn the_workspace_label_defaults_to_folder_and_template() {
    let reg = Registry::builtin();
    let templates = Templates::builtin();
    let t = templates.get("squad").unwrap();
    let p = plan(&LaunchRequest::new(project(), t), &reg).expect("plans");
    assert_eq!(p.workspace_label, "herdup — Squad");

    let p = plan(&LaunchRequest::new(project(), t).label("custom"), &reg).expect("plans");
    assert_eq!(p.workspace_label, "custom");
}

#[test]
fn distinct_clis_dedupes_so_first_run_work_happens_once_per_cli() {
    let reg = Registry::builtin();
    let templates = Templates::builtin();
    let t = templates.get("squad").unwrap();

    let p = plan(&LaunchRequest::new(project(), t), &reg).expect("plans");
    assert_eq!(p.distinct_clis(), vec!["claude"]);

    let p = plan(
        &LaunchRequest::new(project(), t).override_cli(3, "codex"),
        &reg,
    )
    .expect("plans");
    assert_eq!(p.distinct_clis(), vec!["claude", "codex"]);
}

#[test]
fn planning_is_deterministic() {
    let reg = Registry::builtin();
    let templates = Templates::builtin();
    let t = templates.get("full-team").unwrap();
    let a = plan(&LaunchRequest::new(project(), t), &reg).expect("plans");
    let b = plan(&LaunchRequest::new(project(), t), &reg).expect("plans");
    assert_eq!(a, b);
}

#[test]
fn every_split_references_a_pane_created_earlier() {
    // The executor resolves PaneRefs as it walks the list, so a forward
    // reference would be unresolvable at run time.
    let reg = Registry::builtin();
    let templates = Templates::builtin();
    for t in templates.iter() {
        let p = plan(&LaunchRequest::new(project(), t), &reg).expect("plans");
        let mut created = vec![PaneRef(0)]; // the workspace root pane
        for step in &p.steps {
            match step {
                Step::SplitPane { from, creates, .. } => {
                    assert!(
                        created.contains(from),
                        "{}: split from unknown {from}",
                        t.id
                    );
                    created.push(*creates);
                }
                Step::RenamePane { pane, .. }
                | Step::StartAgent { pane, .. }
                | Step::RunCommand { pane, .. }
                | Step::SendBriefing { pane, .. } => {
                    assert!(
                        created.contains(pane),
                        "{}: step targets unknown {pane}",
                        t.id
                    );
                }
                Step::CreateWorkspace { .. } => {}
            }
        }
    }
}

// ---------------------------------------------------------------------------
// added panes
//
// The launcher lets people build a line-up rather than only pick a preset, so
// the plan builder has to append panes the template never named.
// ---------------------------------------------------------------------------

fn addable(id: &str) -> launcher_core::template::PaneSpec {
    launcher_core::template::addable_roles()
        .into_iter()
        .find(|r| r.id == id)
        .unwrap_or_else(|| panic!("addable role {id:?} exists"))
        .spec
}

#[test]
fn every_addable_role_names_a_cli_in_the_registry() {
    let reg = Registry::builtin();
    for role in launcher_core::template::addable_roles() {
        assert!(
            reg.contains(&role.spec.cli),
            "addable role {:?} names unknown cli {:?}",
            role.id,
            role.spec.cli
        );
        assert!(
            !role.spec.coordinator,
            "addable role {:?} must never be the coordinator",
            role.id
        );
        assert!(
            role.spec.split.is_none(),
            "addable role {:?} must carry no split; plan() attaches it to the root",
            role.id
        );
        assert!(
            !role.summary.trim().is_empty(),
            "{:?} needs a summary",
            role.id
        );
        assert!(
            !role.spec.briefing.trim().is_empty(),
            "{:?} needs a briefing — the UI must never invent one",
            role.id
        );
    }
}

#[test]
fn an_added_pane_is_appended_and_hangs_off_the_root() {
    let reg = Registry::builtin();
    let templates = Templates::builtin();
    let t = templates.get("solo").expect("template exists");

    let p = plan(
        &LaunchRequest::new(project(), t).add_pane(addable("tester")),
        &reg,
    )
    .expect("plans");

    assert_eq!(p.panes.len(), 2);
    assert_eq!(p.panes[1].role, "Tester");
    assert!(
        p.steps.iter().any(
            |s| matches!(s, Step::SplitPane { from, creates, .. } if from.0 == 0 && creates.0 == 1)
        ),
        "the added pane must split from the root"
    );
}

#[test]
fn an_added_pane_gets_its_own_briefing_and_is_never_the_coordinator() {
    let reg = Registry::builtin();
    let templates = Templates::builtin();
    let t = templates.get("squad").expect("template exists");

    let p = plan(
        &LaunchRequest::new(project(), t).add_pane(addable("research")),
        &reg,
    )
    .expect("plans");

    let added = p.panes.last().expect("has panes");
    assert_eq!(added.role, "Research");
    assert!(!added.coordinator, "added panes are never the coordinator");
    assert_eq!(
        p.panes.iter().filter(|x| x.coordinator).count(),
        1,
        "the template's coordinator is still the only one"
    );

    let briefed_added = p.steps.iter().any(|s| match s {
        Step::SendBriefing { pane, text, .. } => {
            pane.0 == added.pane.0
                && matches!(text, BriefingText::Literal(t) if t.contains("research"))
        }
        _ => false,
    });
    assert!(briefed_added, "the added pane must get its own briefing");
}

#[test]
fn cli_overrides_never_apply_to_added_panes() {
    let reg = Registry::builtin();
    let templates = Templates::builtin();
    let t = templates.get("solo").expect("template exists");

    // Index 1 exists only because a pane was added; the override targets the
    // TEMPLATE index space and must not reach it.
    let p = plan(
        &LaunchRequest::new(project(), t)
            .add_pane(addable("coder"))
            .override_cli(1, "gemini"),
        &reg,
    )
    .expect("plans");

    assert_eq!(p.panes[1].cli, "claude", "the added pane keeps its own cli");
}

#[test]
fn adding_survives_dropping_every_template_pane() {
    let reg = Registry::builtin();
    let templates = Templates::builtin();
    let t = templates.get("solo").expect("template exists");

    let p = plan(
        &LaunchRequest::new(project(), t)
            .skip_pane(0)
            .add_pane(addable("coder")),
        &reg,
    )
    .expect("plans");

    assert_eq!(p.panes.len(), 1);
    assert_eq!(p.panes[0].role, "Coder");
    assert!(
        !p.steps.iter().any(|s| matches!(s, Step::SplitPane { .. })),
        "a lone added pane is the root and is never split into existence"
    );
}

#[test]
fn dropping_everything_with_nothing_added_still_fails() {
    let reg = Registry::builtin();
    let templates = Templates::builtin();
    let t = templates.get("solo").expect("template exists");
    let err = plan(&LaunchRequest::new(project(), t).skip_pane(0), &reg).unwrap_err();
    assert!(matches!(err, PlanError::NothingToLaunch), "got {err:?}");
}

#[test]
fn two_coordinators_are_rejected_rather_than_silently_briefed() {
    let reg = Registry::builtin();
    let templates = Templates::builtin();
    let t = templates.get("squad").expect("template exists");

    // Force the invariant the toml normally guarantees.
    let mut rogue = addable("lead");
    rogue.coordinator = true;

    let err = plan(&LaunchRequest::new(project(), t).add_pane(rogue), &reg).unwrap_err();
    assert!(
        matches!(err, PlanError::MultipleCoordinators { count: 2 }),
        "got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// template index vs compacted index
// ---------------------------------------------------------------------------

/// Regression: `PaneRef` shifts when a pane is dropped, but `skip` is keyed on
/// TEMPLATE indices. A UI that fed the displayed index back into `skip` would
/// drop the wrong teammate on the second removal.
#[test]
fn dropping_twice_removes_the_two_panes_the_user_actually_pointed_at() {
    let reg = Registry::builtin();
    let templates = Templates::builtin();
    let t = templates.get("squad").expect("template exists");

    let first = plan(&LaunchRequest::new(project(), t), &reg).expect("plans");
    let coder1 = first
        .panes
        .iter()
        .find(|p| p.role == "Coder 1")
        .expect("squad has Coder 1");
    let qa_origin = first
        .panes
        .iter()
        .find(|p| p.role == "QA")
        .and_then(|p| p.origin)
        .expect("squad has QA");

    // Drop Coder 1, then drop QA — using origins, as the UI must.
    let after = plan(
        &LaunchRequest::new(project(), t)
            .skip_pane(coder1.origin.expect("template pane"))
            .skip_pane(qa_origin),
        &reg,
    )
    .expect("plans");

    let roles: Vec<&str> = after.panes.iter().map(|p| p.role.as_str()).collect();
    assert_eq!(roles, vec!["PM", "Coder 2"], "got {roles:?}");
}

#[test]
fn origin_is_the_template_index_and_is_none_for_added_panes() {
    let reg = Registry::builtin();
    let templates = Templates::builtin();
    let t = templates.get("squad").expect("template exists");

    let p = plan(
        &LaunchRequest::new(project(), t)
            .skip_pane(1)
            .add_pane(addable("tester")),
        &reg,
    )
    .expect("plans");

    // Compacted index 1 is template index 2 once pane 1 is dropped.
    assert_eq!(p.panes[1].origin, Some(2));
    assert_eq!(p.panes[0].origin, Some(0));
    assert_eq!(
        p.panes.last().expect("has panes").origin,
        None,
        "an added pane has no template index"
    );
}

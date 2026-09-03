//! herdup desktop app: the Tauri command layer.
//!
//! Deliberately thin. Every decision — planning, gating, remediation — lives in
//! `launcher-core` and is already tested without a GUI (Phases 1–6). This module
//! only converts those types into something serialisable and streams progress
//! events. **No business logic belongs here**, because nothing here is covered
//! by the test suite that matters.

use launcher_core::execute::{Event, Executor, LaunchedPane, Outcome, PaneState};
use launcher_core::firstrun::{FirstRun, FirstRunSession, FirstRunState, FirstRunTarget};
use launcher_core::herdr::HerdrCli;
use launcher_core::plan::{LaunchPlan, LaunchRequest};
use launcher_core::preflight::{self, Preflight, SystemResolver};
use launcher_core::registry::Registry;
use launcher_core::settings::Settings;
use launcher_core::template::Templates;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{Emitter, Manager, State};

const DEFAULT_SESSION: &str = "herdup";

/// The herdr session herdup owns.
///
/// Overridable via `HERDUP_SESSION` so the test harness and the screenshot
/// capture can run against an isolated session. A named session gets its own
/// socket *and* its own state directory, so an override cannot see — or be
/// seen by — the teams a real user is running. That isolation is also why the
/// published screenshots never contain somebody's actual project.
fn session() -> String {
    std::env::var("HERDUP_SESSION")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_SESSION.to_string())
}

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct TemplateDto {
    id: String,
    display_name: String,
    description: String,
    panes: Vec<TemplatePaneDto>,
}

#[derive(Serialize)]
pub struct TemplatePaneDto {
    role: String,
    cli: String,
    flags: String,
    coordinator: bool,
}

#[derive(Serialize)]
pub struct CliDto {
    id: String,
    display_name: String,
    binary: String,
    /// `null` when herdr cannot manage this CLI as an agent.
    kind: Option<String>,
    flag_presets: Vec<String>,
    /// Whether herdup may brief it without a human looking first.
    auto_briefable: bool,
    install_command: Option<String>,
    docs_url: Option<String>,
}

#[derive(Serialize)]
pub struct WorkspaceDto {
    workspace_id: String,
    label: String,
    pane_count: u32,
    agent_status: String,
    /// The folder its panes are in, so the workspace is identifiable and can be
    /// reused as a project. Taken from the panes themselves rather than the
    /// label, which is only a display name.
    path: Option<String>,
    /// At least one pane is waiting on a human.
    blocked: bool,
}

#[derive(Serialize)]
pub struct PlannedPaneDto {
    index: usize,
    /// Template index, or `null` for a pane the user added. Anything feeding
    /// back into `skip`/`overrides` must use this, never `index`.
    origin: Option<usize>,
    role: String,
    cli: String,
    cli_display: String,
    command: String,
    agent_name: Option<String>,
    coordinator: bool,
    /// False means a human must release the briefing.
    auto_brief: bool,
    dropped_flags: Option<String>,
}

#[derive(Serialize)]
pub struct PlanDto {
    workspace_label: String,
    panes: Vec<PlannedPaneDto>,
    steps: Vec<String>,
    distinct_clis: Vec<String>,
    manual_briefings: usize,
}

#[derive(Serialize)]
pub struct CliStatusDto {
    id: String,
    display_name: String,
    binary: String,
    resolved: Option<String>,
    installed: bool,
    first_run_done: bool,
    install_command: Option<String>,
    docs_url: Option<String>,
    alternatives: Vec<String>,
}

#[derive(Serialize)]
pub struct PreflightDto {
    herdr: String,
    herdr_ok: bool,
    /// Present when a herdr server on another protocol is running: herdup will
    /// not restart it, because that would exit the user's panes.
    herdr_note: Option<String>,
    gh_ready: bool,
    gh_account: Option<String>,
    gh_blocker: Option<String>,
    clis: Vec<CliStatusDto>,
    needs_first_run: Vec<String>,
    blocking: Vec<String>,
    /// Not blockers, but things a human must acknowledge before agents start
    /// editing — an unversioned folder, or uncommitted work already in the tree.
    warnings: Vec<String>,
    /// A standing platform caveat, not a per-launch problem.
    platform_note: Option<String>,
    project: String,
    git_branch: Option<String>,
    can_launch: bool,
}

#[derive(Serialize, Clone)]
pub struct LaunchedPaneDto {
    index: usize,
    role: String,
    cli_display: String,
    pane_id: Option<String>,
    agent_name: Option<String>,
    /// `briefed` | `needs_attention` | `ready` | `starting` | `not_created`
    state: String,
    reason: Option<String>,
    screen: Option<String>,
    has_pending_briefing: bool,
}

#[derive(Serialize, Clone)]
pub struct OutcomeDto {
    workspace_id: Option<String>,
    panes: Vec<LaunchedPaneDto>,
    briefed: usize,
    failure: Option<String>,
    failed_step: Option<String>,
    session: String,
}

#[derive(Serialize, Clone)]
pub struct FirstRunPaneDto {
    cli: String,
    display_name: String,
    pane_id: String,
    /// `waiting` | `needs_you` | `verified`
    state: String,
    screen: String,
    hints: Vec<HintDto>,
}

#[derive(Serialize, Clone)]
pub struct HintDto {
    kind: String,
    value: String,
}

#[derive(Serialize)]
pub struct ProjectStatusDto {
    exists: bool,
    name: String,
    versioned: bool,
    branch: Option<String>,
    uncommitted: usize,
}

#[derive(Serialize)]
pub struct AddableRoleDto {
    id: String,
    display_name: String,
    summary: String,
    cli: String,
}

#[derive(Deserialize, Default)]
pub struct LaunchOptions {
    project: String,
    template: String,
    #[serde(default)]
    skip: Vec<usize>,
    /// `[[index, cli_id], …]`
    #[serde(default)]
    overrides: Vec<(usize, String)>,
    /// Ids of roles added beyond the template, in the order they were added.
    ///
    /// Ids only: the briefing text for each role lives in the core crate, so the
    /// front end never supplies a prompt.
    #[serde(default)]
    extra: Vec<String>,
}

// ---------------------------------------------------------------------------
// state
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct AppState {
    /// The most recent launch, so a held briefing can be released later.
    outcome: Mutex<Option<Outcome>>,
    first_run: Mutex<Option<FirstRunSession>>,
}

fn client() -> Result<HerdrCli, String> {
    HerdrCli::discover()
        .map(|c| c.with_session(session()))
        .map_err(|e| e.to_string())
}

fn load() -> Result<(Registry, Templates, Settings), String> {
    let registry = launcher_core::config::load_registry().map_err(|e| e.to_string())?;
    let templates = launcher_core::config::load_templates(&registry).map_err(|e| e.to_string())?;
    Ok((registry, templates, Settings::load()))
}

/// Plan for display: no herdr calls at all.
///
/// `preview_plan` runs this on every keystroke and every team change, so it must
/// stay cheap. Agent names are not shown in the UI, so a preview does not need
/// to know which are taken.
fn build_plan(options: &LaunchOptions) -> Result<(LaunchPlan, Registry, Settings), String> {
    build_plan_inner(options, Vec::new())
}

/// Plan for a real launch, avoiding agent names the session already uses.
///
/// Agent names are unique per herdr *session*, so starting a second team while a
/// first is running would otherwise ask for `pm` again and be rejected with
/// `agent_name_taken` partway through. Only the launch path pays for the extra
/// herdr call; best-effort, since a server that cannot be reached has no agents
/// to collide with and preflight reports that separately.
fn build_launch_plan(options: &LaunchOptions) -> Result<(LaunchPlan, Registry, Settings), String> {
    let taken: Vec<String> = client()
        .and_then(|c| c.agent_list().map_err(|e| e.to_string()))
        .map(|agents| agents.into_iter().map(|a| a.name).collect())
        .unwrap_or_default();
    build_plan_inner(options, taken)
}

fn build_plan_inner(
    options: &LaunchOptions,
    reserved: Vec<String>,
) -> Result<(LaunchPlan, Registry, Settings), String> {
    let (registry, templates, settings) = load()?;
    let template = templates
        .get(&options.template)
        .ok_or_else(|| format!("no template '{}'", options.template))?;

    let project = PathBuf::from(&options.project);
    let mut request = LaunchRequest::new(&project, template).reserving(reserved);
    for index in &options.skip {
        request = request.skip_pane(*index);
    }
    for (index, cli) in &options.overrides {
        request = request.override_cli(*index, cli);
    }
    let addable = launcher_core::template::addable_roles();
    for id in &options.extra {
        let role = addable
            .iter()
            .find(|r| &r.id == id)
            .ok_or_else(|| format!("no role '{id}' to add"))?;
        request = request.add_pane(role.spec.clone());
    }
    let plan = launcher_core::plan::plan(&request, &registry).map_err(|e| e.to_string())?;
    Ok((plan, registry, settings))
}

// ---------------------------------------------------------------------------
// commands
// ---------------------------------------------------------------------------

#[tauri::command]
fn list_templates() -> Result<Vec<TemplateDto>, String> {
    let (_, templates, _) = load()?;
    Ok(templates
        .iter()
        .map(|t| TemplateDto {
            id: t.id.clone(),
            display_name: t.display_name.clone(),
            description: t.description.clone(),
            panes: t
                .panes
                .iter()
                .map(|p| TemplatePaneDto {
                    role: p.role.clone(),
                    cli: p.cli.clone(),
                    flags: p.flags.clone(),
                    coordinator: p.coordinator,
                })
                .collect(),
        })
        .collect())
}

/// Cheap, read-only look at a folder, for the moment a project is chosen.
///
/// Deliberately much lighter than [`run_preflight`]: it touches git and the
/// filesystem only, so the launcher can warn about an un-undoable folder the
/// instant it is picked rather than three screens later.
#[tauri::command]
async fn project_status(project: String) -> Result<ProjectStatusDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = PathBuf::from(&project);
        let exists = path.is_dir();
        let git = launcher_core::preflight::git_status(&path);
        ProjectStatusDto {
            exists,
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| project.clone()),
            versioned: git.is_repo,
            branch: git.branch,
            uncommitted: git.dirty_files,
        }
    })
    .await
    .map_err(|e| e.to_string())
}

/// The roles the user may add on top of a template.
///
/// The UI renders these as the "add someone" controls and sends back ids only.
#[tauri::command]
fn list_addable_roles() -> Vec<AddableRoleDto> {
    launcher_core::template::addable_roles()
        .into_iter()
        .map(|r| AddableRoleDto {
            id: r.id,
            display_name: r.display_name,
            summary: r.summary,
            cli: r.spec.cli,
        })
        .collect()
}

#[tauri::command]
fn list_clis() -> Result<Vec<CliDto>, String> {
    let (registry, _, _) = load()?;
    Ok(registry
        .iter()
        .map(|e| CliDto {
            id: e.id.clone(),
            display_name: e.display_name.clone(),
            binary: e.binary.clone(),
            kind: e.kind.clone(),
            flag_presets: e.flag_presets.clone(),
            auto_briefable: e.briefing_trust.may_auto_brief() && e.has_agent_kind(),
            install_command: e.install_command().map(str::to_string),
            docs_url: e.docs_url.clone(),
        })
        .collect())
}

/// Live workspaces on herdup's own session.
///
/// Never the user's default session: a protocol mismatch there is reported, not
/// resolved, because resolving it means stopping their server and exiting their
/// panes.
#[tauri::command]
async fn list_workspaces() -> Result<Vec<WorkspaceDto>, String> {
    tauri::async_runtime::spawn_blocking(list_workspaces_blocking)
        .await
        .map_err(|e| e.to_string())?
}

fn list_workspaces_blocking() -> Result<Vec<WorkspaceDto>, String> {
    let cli = client()?;
    match cli.workspace_list() {
        Ok(list) => {
            // One pane listing for all workspaces, rather than one call each.
            let panes = cli.pane_list().unwrap_or_default();
            Ok(list
                .into_iter()
                .map(|w| {
                    let mine: Vec<_> = panes
                        .iter()
                        .filter(|p| p.workspace_id == w.workspace_id)
                        .collect();
                    WorkspaceDto {
                        label: w.label.clone().unwrap_or_else(|| w.workspace_id.clone()),
                        pane_count: w.pane_count,
                        agent_status: w.agent_status.as_str().to_string(),
                        path: mine.first().map(|p| p.cwd_path().display().to_string()),
                        blocked: mine
                            .iter()
                            .any(|p| p.agent_status == launcher_core::herdr::AgentStatus::Blocked),
                        workspace_id: w.workspace_id,
                    }
                })
                .collect())
        }
        // No server yet is the normal first-run state, not an error to show.
        Err(e) if e.is_recoverable_by_starting_server() => Ok(Vec::new()),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
fn preview_plan(options: LaunchOptions) -> Result<PlanDto, String> {
    let (plan, _, _) = build_plan(&options)?;
    Ok(plan_dto(&plan))
}

fn plan_dto(plan: &LaunchPlan) -> PlanDto {
    PlanDto {
        workspace_label: plan.workspace_label.clone(),
        panes: plan
            .panes
            .iter()
            .map(|p| PlannedPaneDto {
                index: p.pane.0,
                origin: p.origin,
                role: p.role.clone(),
                cli: p.cli.clone(),
                cli_display: p.cli_display.clone(),
                command: p.command.clone(),
                agent_name: p.agent_name.clone(),
                coordinator: p.coordinator,
                auto_brief: p.gate.is_auto(),
                dropped_flags: p.dropped_flags.clone(),
            })
            .collect(),
        steps: plan.describe().lines().map(str::to_string).collect(),
        distinct_clis: plan.distinct_clis().iter().map(|s| s.to_string()).collect(),
        manual_briefings: plan.requires_human_briefing().len(),
    }
}

/// Inspect the environment.
///
/// **Async on purpose.** This shells out to `herdr`, `git` and `gh`; as a
/// synchronous command it blocked Tauri's main thread, and the webview froze
/// with no error — the UI simply never left the previous screen. Every command
/// that spawns a process must go through `spawn_blocking`.
#[tauri::command]
async fn run_preflight(options: LaunchOptions) -> Result<PreflightDto, String> {
    tauri::async_runtime::spawn_blocking(move || run_preflight_blocking(options))
        .await
        .map_err(|e| e.to_string())?
}

fn run_preflight_blocking(options: LaunchOptions) -> Result<PreflightDto, String> {
    let (plan, registry, settings) = build_plan(&options)?;
    let cli = client()?;
    let pf = Preflight::run(&cli, &plan, &registry, &settings, &SystemResolver);
    let gh = preflight::check_gh();

    use launcher_core::preflight::HerdrStatus;
    let (herdr, herdr_ok, herdr_note) = match &pf.herdr {
        HerdrStatus::Ready { version } => (format!("herdr {version}"), true, None),
        HerdrStatus::ServerDown { version } => (
            format!("herdr {version}"),
            true,
            Some("No server for herdup's session yet — one starts at launch.".into()),
        ),
        HerdrStatus::TooOld { found, required } => (
            format!("herdr {found}"),
            false,
            Some(format!("Too old: herdup needs {required} or newer.")),
        ),
        HerdrStatus::ProtocolMismatch { version, .. } => (
            format!("herdr {version}"),
            false,
            Some(
                "A herdr server on a different protocol is running. Restarting it would \
                 exit its panes, so herdup will not do it for you."
                    .into(),
            ),
        ),
        HerdrStatus::Missing => ("herdr not found".into(), false, None),
    };

    Ok(PreflightDto {
        herdr,
        herdr_ok,
        herdr_note,
        gh_ready: gh.usable(),
        gh_account: gh.account.clone(),
        gh_blocker: gh.blocker().map(str::to_string),
        clis: pf
            .clis
            .iter()
            .map(|c| CliStatusDto {
                id: c.id.clone(),
                display_name: c.display_name.clone(),
                binary: c.binary.clone(),
                resolved: c.resolved.as_ref().map(|p| p.display().to_string()),
                installed: c.installed(),
                first_run_done: c.first_run_done,
                install_command: c.install_command.clone(),
                docs_url: c.docs_url.clone(),
                alternatives: pf
                    .alternatives_for(&c.id)
                    .iter()
                    .map(|a| a.id.clone())
                    .collect(),
            })
            .collect(),
        needs_first_run: pf.needs_first_run().iter().map(|c| c.id.clone()).collect(),
        blocking: pf.blocking_issues().iter().map(|i| i.explain()).collect(),
        warnings: pf.warnings().iter().map(|w| w.explain()).collect(),
        platform_note: preflight::platform_note().map(str::to_string),
        project: pf.project.display().to_string(),
        git_branch: pf.git.branch.clone(),
        can_launch: pf.can_launch(),
    })
}

// ---- first run -------------------------------------------------------------

#[tauri::command]
async fn start_first_run(
    app: tauri::AppHandle,
    options: LaunchOptions,
) -> Result<Vec<FirstRunPaneDto>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        start_first_run_blocking(options, state)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn start_first_run_blocking(
    options: LaunchOptions,
    state: State<'_, AppState>,
) -> Result<Vec<FirstRunPaneDto>, String> {
    let (plan, registry, settings) = build_plan(&options)?;
    let cli = client()?;
    preflight::ensure_server(&cli, Duration::from_secs(20)).map_err(|e| e.to_string())?;

    let pf = Preflight::run(&cli, &plan, &registry, &settings, &SystemResolver);
    let targets: Vec<FirstRunTarget> = pf
        .needs_first_run()
        .iter()
        .map(|c| FirstRunTarget {
            cli: c.id.clone(),
            display_name: c.display_name.clone(),
            binary: c.binary.clone(),
        })
        .collect();
    if targets.is_empty() {
        return Ok(Vec::new());
    }

    let session = FirstRun::new(&cli)
        .start(&plan.project, &targets, &mut |_| {})
        .map_err(|e| e.to_string())?;
    let dto = first_run_dto(&session);
    *state.first_run.lock().unwrap() = Some(session);
    Ok(dto)
}

/// One polling round. The UI owns the interval, which keeps the backend free of
/// timers and matches how `FirstRun` is tested.
/// Polled every two seconds by the UI, so blocking here would stutter the whole
/// window.
#[tauri::command]
async fn poll_first_run(app: tauri::AppHandle) -> Result<Vec<FirstRunPaneDto>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        poll_first_run_blocking(state)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn poll_first_run_blocking(state: State<'_, AppState>) -> Result<Vec<FirstRunPaneDto>, String> {
    let cli = client()?;
    let mut guard = state.first_run.lock().unwrap();
    let Some(session) = guard.as_mut() else {
        return Ok(Vec::new());
    };
    FirstRun::new(&cli).poll_once(session, &mut |_| {});
    Ok(first_run_dto(session))
}

/// Finish the pass: record what actually verified, then tear the panes down.
///
/// Only CLIs that reached their prompt are cached, so abandoning caches nothing.
#[tauri::command]
async fn finish_first_run(app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        finish_first_run_blocking(state)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn finish_first_run_blocking(state: State<'_, AppState>) -> Result<(), String> {
    let cli = client()?;
    let Some(session) = state.first_run.lock().unwrap().take() else {
        return Ok(());
    };
    let mut settings = Settings::load();
    session.apply_to(&mut settings);
    let _ = settings.save();
    let _ = FirstRun::new(&cli).teardown(&session);
    Ok(())
}

fn first_run_dto(session: &FirstRunSession) -> Vec<FirstRunPaneDto> {
    session
        .panes
        .iter()
        .map(|p| FirstRunPaneDto {
            cli: p.cli.clone(),
            display_name: p.display_name.clone(),
            pane_id: p.pane_id.clone(),
            state: match p.state {
                FirstRunState::Waiting => "waiting",
                FirstRunState::NeedsYou => "needs_you",
                FirstRunState::Verified => "verified",
            }
            .into(),
            screen: p.screen.clone(),
            hints: p
                .hints
                .iter()
                .map(|h| HintDto {
                    kind: format!("{:?}", h.kind).to_lowercase(),
                    value: h.value.clone(),
                })
                .collect(),
        })
        .collect()
}

// ---- launching -------------------------------------------------------------

/// Run the launch on a worker thread, streaming progress as `launch-progress`
/// events and resolving with the finished outcome.
#[tauri::command]
async fn launch(app: tauri::AppHandle, options: LaunchOptions) -> Result<OutcomeDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        // The only path that actually starts agents, so the only one that needs
        // to know which agent names the session already owns.
        let (plan, _, _) = build_launch_plan(&options)?;
        let cli = client()?;
        preflight::ensure_server(&cli, Duration::from_secs(20)).map_err(|e| e.to_string())?;

        let outcome = Executor::new(&cli).execute(&plan, &mut |event| {
            let _ = app.emit("launch-progress", progress_dto(&event));
        });

        let dto = outcome_dto(&outcome);
        if let Some(state) = app.try_state::<AppState>() {
            *state.outcome.lock().unwrap() = Some(outcome);
        }
        Ok(dto)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize, Clone)]
struct ProgressDto {
    kind: String,
    index: Option<usize>,
    total: Option<usize>,
    role: Option<String>,
    detail: Option<String>,
}

fn progress_dto(event: &Event) -> ProgressDto {
    let base = |kind: &str| ProgressDto {
        kind: kind.into(),
        index: None,
        total: None,
        role: None,
        detail: None,
    };
    match event {
        Event::StepStarted {
            index,
            total,
            description,
        } => ProgressDto {
            index: Some(*index),
            total: Some(*total),
            detail: Some(description.clone()),
            ..base("step")
        },
        Event::PaneCreated { role, pane_id, .. } => ProgressDto {
            role: Some(role.clone()),
            detail: Some(pane_id.clone()),
            ..base("pane_created")
        },
        Event::PaneReady { role, .. } => ProgressDto {
            role: Some(role.clone()),
            ..base("pane_ready")
        },
        Event::PaneNeedsAttention { role, reason, .. } => ProgressDto {
            role: Some(role.clone()),
            detail: Some(reason.explain().into()),
            ..base("needs_attention")
        },
        Event::Briefed { role, .. } => ProgressDto {
            role: Some(role.clone()),
            ..base("briefed")
        },
        Event::BriefingWithheld { role, reason, .. } => ProgressDto {
            role: Some(role.clone()),
            detail: Some(reason.explain().into()),
            ..base("briefing_withheld")
        },
        Event::Failed { message, .. } => ProgressDto {
            detail: Some(message.clone()),
            ..base("failed")
        },
        Event::Finished => base("finished"),
    }
}

fn pane_dto(index: usize, pane: &LaunchedPane) -> LaunchedPaneDto {
    let (state, reason) = match &pane.state {
        PaneState::Briefed => ("briefed", None),
        PaneState::Ready => ("ready", None),
        PaneState::Starting => ("starting", None),
        PaneState::NotCreated => ("not_created", None),
        PaneState::NeedsAttention(r) => ("needs_attention", Some(r.explain().to_string())),
    };
    LaunchedPaneDto {
        index,
        role: pane.role.clone(),
        cli_display: pane.cli_display.clone(),
        pane_id: pane.pane_id.clone(),
        agent_name: pane.agent_name.clone(),
        state: state.into(),
        reason,
        screen: pane.screen.clone(),
        has_pending_briefing: pane.pending_briefing.is_some(),
    }
}

fn outcome_dto(outcome: &Outcome) -> OutcomeDto {
    OutcomeDto {
        workspace_id: outcome.workspace_id.clone(),
        panes: outcome
            .panes
            .iter()
            .enumerate()
            .map(|(i, p)| pane_dto(i, p))
            .collect(),
        briefed: outcome.briefed(),
        failure: outcome.failure.as_ref().map(|f| f.message.clone()),
        failed_step: outcome.failure.as_ref().map(|f| f.description.clone()),
        session: session(),
    }
}

/// Release a briefing a human has now cleared.
///
/// Goes through the agent surface, so if the dialog is still up herdr refuses
/// again rather than typing the briefing into it.
#[tauri::command]
async fn send_briefing_now(app: tauri::AppHandle, index: usize) -> Result<OutcomeDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        send_briefing_now_blocking(index, state)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn send_briefing_now_blocking(
    index: usize,
    state: State<'_, AppState>,
) -> Result<OutcomeDto, String> {
    let cli = client()?;
    let mut guard = state.outcome.lock().unwrap();
    let outcome = guard.as_mut().ok_or("no launch in progress")?;
    let pane = outcome
        .panes
        .get_mut(index)
        .ok_or_else(|| format!("no pane {index}"))?;
    Executor::new(&cli)
        .send_pending_briefing(pane)
        .map_err(|e| e.to_string())?;
    Ok(outcome_dto(outcome))
}

#[derive(Serialize, Clone)]
pub struct CreatedRepoDto {
    url: Option<String>,
    path: String,
}

/// GitHub accounts and organisations the user could create under.
#[tauri::command]
async fn gh_owners() -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        launcher_core::github::Gh::discover()
            .map(|gh| gh.owners())
            .unwrap_or_default()
    })
    .await
    .map_err(|e| e.to_string())
}

/// Create a GitHub repository and clone it.
///
/// **The one outward-facing action herdup takes.** The UI form is the
/// confirmation; everything checkable is validated before the call, and the
/// destination is never overwritten.
#[tauri::command]
async fn create_repo(
    name: String,
    owner: Option<String>,
    public: bool,
    into: String,
    description: Option<String>,
) -> Result<CreatedRepoDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        use launcher_core::github::{Gh, NewRepo, Visibility};
        let gh = Gh::discover().map_err(|e| e.to_string())?;
        let mut repo = NewRepo::private(name, PathBuf::from(into));
        repo.owner = owner.filter(|o| !o.trim().is_empty());
        repo.description = description.filter(|d| !d.trim().is_empty());
        if public {
            repo.visibility = Visibility::Public;
        }
        let created = gh.create(&repo).map_err(|e| e.to_string())?;
        Ok(CreatedRepoDto {
            url: created.url,
            path: created.path.display().to_string(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Attach to a workspace that is already running.
///
/// Focuses it first so the terminal opens on that workspace rather than
/// whichever one herdr happened to have focused.
#[tauri::command]
async fn attach_workspace(workspace_id: String, path: Option<String>) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let cli = client()?;
        cli.workspace_focus(&workspace_id)
            .map_err(|e| e.to_string())?;
        let settings = Settings::load();
        let dir = path
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        match launcher_core::terminal::open_with_fallback(
            &dir,
            Some(&session()),
            settings.terminal.as_deref(),
        ) {
            Ok(h) => Ok(h.display()),
            Err(h) => Err(format!("could not open a terminal. Run: {}", h.display())),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn open_terminal(project: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || open_terminal_blocking(project))
        .await
        .map_err(|e| e.to_string())?
}

fn open_terminal_blocking(project: String) -> Result<String, String> {
    let settings = Settings::load();
    let path = PathBuf::from(project);
    match launcher_core::terminal::open_with_fallback(
        &path,
        Some(&session()),
        settings.terminal.as_deref(),
    ) {
        Ok(h) => Ok(h.display()),
        Err(h) => Err(format!(
            "could not open a terminal. Run this yourself: {}",
            h.display()
        )),
    }
}

#[tauri::command]
fn default_projects_root() -> Option<String> {
    Settings::load()
        .projects_root_path()
        .map(|p| p.display().to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // A GUI app on macOS starts with launchd's four-directory PATH, which has
    // none of the user's CLIs on it. Adopt the login shell's PATH before
    // anything probes for herdr, gh or an agent. Done before the builder so no
    // thread exists yet when the environment is mutated.
    if cfg!(target_os = "macos") {
        if let Some(path) = launcher_core::login_shell_path() {
            std::env::set_var("PATH", path);
        }
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            list_templates,
            list_addable_roles,
            project_status,
            list_clis,
            list_workspaces,
            preview_plan,
            run_preflight,
            start_first_run,
            poll_first_run,
            finish_first_run,
            launch,
            send_briefing_now,
            gh_owners,
            create_repo,
            attach_workspace,
            open_terminal,
            default_projects_root,
        ])
        .run(tauri::generate_context!())
        .expect("error while running herdup");
}

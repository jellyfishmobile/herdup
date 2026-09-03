//! herdup command-line harness.
//!
//! Phases 1–6 build the launcher here, with no GUI, so the Tauri layer in
//! Phase 7 is a thin shell over logic that is already proven.
//!
//! Phase 1 provides:
//!   probe  — find herdr, report its version, check the minimum
//!   smoke  — drive a real herdr end to end in a disposable named session

use launcher_core::config::ConfigError;
use launcher_core::herdr::types::SplitDirection;
use launcher_core::herdr::{HerdrCli, HerdrError, MIN_HERDR};
use launcher_core::terminal::reap_in_background;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const DEFAULT_SMOKE_SESSION: &str = "herdup-smoke";

/// Either failure mode a subcommand can hit.
#[derive(Debug)]
enum AppError {
    Herdr(HerdrError),
    Config(ConfigError),
    Plan(launcher_core::plan::PlanError),
    Gh(launcher_core::github::GhError),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::Herdr(e) => write!(f, "{e}"),
            AppError::Config(e) => write!(f, "{e}"),
            AppError::Plan(e) => write!(f, "{e}"),
            AppError::Gh(e) => write!(f, "{e}"),
        }
    }
}

impl From<HerdrError> for AppError {
    fn from(e: HerdrError) -> Self {
        AppError::Herdr(e)
    }
}

impl From<ConfigError> for AppError {
    fn from(e: ConfigError) -> Self {
        AppError::Config(e)
    }
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("probe") => probe(),
        Some("config") => config(),
        Some("plan") => show_plan(&args[1..]),
        Some("preflight") => show_preflight(&args[1..]),
        Some("launch") => launch(&args[1..]),
        Some("new-repo") => new_repo(&args[1..]),
        Some("smoke") => smoke(&args[1..]),
        Some("help") | Some("--help") | Some("-h") | None => {
            usage();
            return std::process::ExitCode::SUCCESS;
        }
        Some(other) => {
            eprintln!("unknown command: {other}\n");
            usage();
            return std::process::ExitCode::from(2);
        }
    };

    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("\nerror: {e}");
            if let AppError::Herdr(HerdrError::ProtocolMismatch { .. }) = e {
                eprintln!(
                    "\nThis usually means a herdr server from an older binary is still running.\n\
                     Restarting it exits its pane processes, so herdup will not do it for you."
                );
            }
            std::process::ExitCode::FAILURE
        }
    }
}

fn usage() {
    println!(
        "herdup harness\n\n\
         usage:\n  \
         launcher-cli probe\n  \
         launcher-cli config\n  \
         launcher-cli plan --template ID [--cwd PATH] [--skip N]... [--cli N=CLI]...\n  \
         launcher-cli preflight --template ID [--cwd PATH] [--session NAME]\n  \
         launcher-cli launch --template ID [--cwd PATH] [--session NAME]\n                       \
         [--skip N]... [--cli N=CLI] [--no-terminal] [--skip-first-run] [--yes]\n      \
         `--template repo` uses the project's own .herdr/team.toml\n  \
         launcher-cli new-repo --name NAME [--owner OWNER] [--public]\n                          \
         [--into PATH] [--description TEXT] [--template ID] [--yes]\n  \
         launcher-cli smoke [--session NAME] [--cwd PATH]\n\n\
         `plan` is a dry run: it prints what a launch would do and changes nothing.\n\
         `smoke` creates and destroys an isolated named session. It never touches\n\
         your default herdr session, whose panes may be real work."
    );
}

fn probe() -> Result<(), AppError> {
    let cli = HerdrCli::discover()?;
    println!("herdr binary : {}", cli.exe().display());

    let version = cli.version()?;
    let min = format!("{}.{}.{}", MIN_HERDR.0, MIN_HERDR.1, MIN_HERDR.2);
    let meets = version.at_least(MIN_HERDR.0, MIN_HERDR.1, MIN_HERDR.2);
    println!("version      : {version}");
    println!(
        "minimum      : {min}  [{}]",
        if meets { "ok" } else { "TOO OLD" }
    );

    if !meets {
        return Err(HerdrError::VersionTooOld {
            found: version.to_string(),
            required: min,
        }
        .into());
    }

    match cli.workspace_list() {
        Ok(ws) => println!("default sess : running, {} workspace(s)", ws.len()),
        Err(HerdrError::ServerUnavailable { .. }) => {
            println!("default sess : not running")
        }
        Err(HerdrError::ProtocolMismatch { .. }) => {
            println!("default sess : running an older protocol (left alone)")
        }
        Err(e) => println!("default sess : unreadable ({e})"),
    }
    Ok(())
}

fn config() -> Result<(), AppError> {
    let dir = launcher_core::config::config_dir();
    match &dir {
        Some(d) => {
            println!("config dir : {}", d.display());
            for name in ["registry.toml", "templates.toml"] {
                let p = d.join(name);
                println!(
                    "  {name:<15} {}",
                    if p.is_file() {
                        "user overrides found"
                    } else {
                        "(none — using built-ins)"
                    }
                );
            }
        }
        None => println!("config dir : unavailable; built-ins only"),
    }

    let registry = launcher_core::config::load_registry()?;
    let templates = launcher_core::config::load_templates(&registry)?;

    println!("\nCLIs ({}):", registry.len());
    println!("  ID               NAME                   BINARY         BRIEFING");
    for e in registry.iter() {
        println!(
            "  {:<16} {:<22} {:<14} {}",
            e.id,
            e.display_name,
            e.binary,
            if e.briefing_trust.may_auto_brief() {
                "verified — auto"
            } else {
                "manual"
            }
        );
    }
    println!(
        "\n  Only 'verified' CLIs are briefed automatically. Everything else waits\n  \
         for you to look at the pane first (spec §5.1)."
    );

    println!("\nTemplates ({}):", templates.len());
    for t in templates.iter() {
        let roles: Vec<String> = t
            .panes
            .iter()
            .map(|p| {
                if p.coordinator {
                    format!("{}*", p.role)
                } else {
                    p.role.clone()
                }
            })
            .collect();
        println!(
            "  {:<12} {} pane(s): {}",
            t.id,
            t.panes.len(),
            roles.join(", ")
        );
        println!("               {}", t.description);
    }
    println!("\n  * coordinator — created first, briefed last with the finished roster.");
    Ok(())
}

fn show_plan(args: &[String]) -> Result<(), AppError> {
    let Some(id) = flag(args, "--template") else {
        eprintln!("plan: --template ID is required (see `launcher-cli config` for ids)");
        return Ok(());
    };
    let cwd = flag(args, "--cwd")
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let registry = launcher_core::config::load_registry()?;
    let template = resolve_template("plan", &id, &cwd, &registry)?;

    let mut request = launcher_core::plan::LaunchRequest::new(&cwd, &template);
    for raw in flags(args, "--skip") {
        match raw.parse::<usize>() {
            Ok(i) => request = request.skip_pane(i),
            Err(_) => eprintln!("plan: ignoring non-numeric --skip {raw:?}"),
        }
    }
    for raw in flags(args, "--cli") {
        match raw.split_once('=') {
            Some((i, cli)) => match i.parse::<usize>() {
                Ok(i) => request = request.override_cli(i, cli),
                Err(_) => eprintln!("plan: ignoring --cli {raw:?} (index must be a number)"),
            },
            None => eprintln!("plan: ignoring --cli {raw:?} (expected N=CLI)"),
        }
    }

    let p = launcher_core::plan::plan(&request, &registry).map_err(AppError::Plan)?;

    println!("project   : {}", p.project.display());
    println!("workspace : {}", p.workspace_label);
    println!("CLIs      : {}", p.distinct_clis().join(", "));
    println!("\npanes:");
    for pane in &p.panes {
        println!(
            "  #{} {:<12} {:<16} {}{}",
            pane.pane.0,
            pane.role,
            pane.cli_display,
            pane.command,
            if pane.coordinator {
                "   [coordinator]"
            } else {
                ""
            }
        );
        if let Some(dropped) = &pane.dropped_flags {
            println!(
                "     dropped {dropped:?} — {} is not known to accept it",
                pane.cli_display
            );
        }
    }

    let manual = p.requires_human_briefing();
    if manual.is_empty() {
        println!("\nAll briefings send automatically once each pane reports idle.");
    } else {
        println!(
            "\n{} pane(s) will NOT be briefed automatically:",
            manual.len()
        );
        for pane in &manual {
            println!("  #{} {} ({})", pane.pane.0, pane.role, pane.cli_display);
        }
        println!(
            "  These CLIs have unverified blocked-detection, so herdup will not type\n  \
             into them unattended. You confirm each one after looking at the pane."
        );
    }

    println!("\nsteps ({} total, nothing has run):", p.steps.len());
    print!("{}", p.describe());
    Ok(())
}

/// Load the templates for `cwd` and pick `id`, or explain why not.
///
/// `repo` is the project's own `.herdr/team.toml`; its absence or its load
/// error is the message. Any other unknown id lists what exists. An unknown
/// or unloadable template exits 2 here rather than returning, the same code
/// `main` uses for an unknown command.
fn resolve_template(
    verb: &str,
    id: &str,
    cwd: &std::path::Path,
    registry: &launcher_core::registry::Registry,
) -> Result<launcher_core::template::Template, AppError> {
    use launcher_core::template::{REPO_TEAM_FILE, REPO_TEMPLATE_ID};
    let (templates, repo_error) = launcher_core::config::load_templates_for(cwd, registry)?;
    if let Some(t) = templates.get(id) {
        return Ok(t.clone());
    }
    if id == REPO_TEMPLATE_ID {
        match repo_error {
            Some(e) => eprintln!("{verb}: {e}"),
            None => eprintln!("{verb}: no {REPO_TEAM_FILE} in {}", cwd.display()),
        }
    } else {
        eprintln!(
            "{verb}: no template '{id}'. Known: {}",
            template_ids(&templates)
        );
    }
    std::process::exit(2);
}

fn template_ids(t: &launcher_core::template::Templates) -> String {
    t.iter()
        .map(|x| x.id.clone())
        .collect::<Vec<_>>()
        .join(", ")
}

fn show_preflight(args: &[String]) -> Result<(), AppError> {
    use launcher_core::preflight::{
        check_gh, summarise, HerdrStatus, Issue, Preflight, SystemResolver,
    };

    let Some(id) = flag(args, "--template") else {
        eprintln!("preflight: --template ID is required");
        return Ok(());
    };
    let cwd = flag(args, "--cwd")
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let session = flag(args, "--session").unwrap_or_else(|| "herdup".to_string());

    let registry = launcher_core::config::load_registry()?;
    let template = resolve_template("preflight", &id, &cwd, &registry)?;
    let settings = launcher_core::Settings::load();
    let p = launcher_core::plan::plan(
        &launcher_core::plan::LaunchRequest::new(&cwd, &template),
        &registry,
    )
    .map_err(AppError::Plan)?;

    let herdr = HerdrCli::discover()?.with_session(&session);
    let pf = Preflight::run(&herdr, &p, &registry, &settings, &SystemResolver);

    println!("project : {}", cwd.display());
    println!("session : {session}");
    match &pf.herdr {
        HerdrStatus::Ready { version } => println!("herdr   : {version}, server up"),
        HerdrStatus::ServerDown { version } => {
            println!("herdr   : {version}, no server for this session (herdup will start one)")
        }
        HerdrStatus::TooOld { found, required } => {
            println!("herdr   : {found} — TOO OLD, need {required}")
        }
        HerdrStatus::ProtocolMismatch { version, .. } => println!(
            "herdr   : {version}, but a server on a different protocol is running \
             (left alone — restarting it would kill its panes)"
        ),
        HerdrStatus::Missing => println!("herdr   : NOT FOUND"),
    }

    if let Some(note) = launcher_core::preflight::platform_note() {
        println!("note    : {note}");
    }

    let gh = check_gh();
    match gh.blocker() {
        None => println!(
            "gh      : ready{}",
            gh.account
                .as_deref()
                .map(|a| format!(" ({a})"))
                .unwrap_or_default()
        ),
        Some(why) => println!("gh      : {why} — only the new-repo flow is affected"),
    }

    println!("\nCLIs needed by '{id}':");
    for cli in &pf.clis {
        println!("  {}", summarise(cli));
    }

    let first_run = pf.needs_first_run();
    if first_run.is_empty() {
        println!("\nFirst-run: nothing to do (all cached for this project).");
    } else {
        println!(
            "\nFirst-run needed for: {}",
            first_run
                .iter()
                .map(|c| c.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        println!(
            "  Stage 1 opens one bare pane per CLI in this project so logins and\n  \
             first-run 'trust this folder' prompts are cleared before the team is built."
        );
    }

    let warnings = pf.warnings();
    if !warnings.is_empty() {
        println!("\nBefore launching agents into this folder:");
        for warning in &warnings {
            println!("  ! {}", warning.explain());
        }
    }

    for issue in pf.auto_resolvable_issues() {
        if issue.is_server_startable() {
            println!("\nNote: no server for this session yet — herdup starts one at launch.");
        }
    }

    let issues = pf.blocking_issues();
    if issues.is_empty() {
        println!("\nREADY TO LAUNCH");
        return Ok(());
    }
    println!("\n{} issue(s) you need to resolve:", issues.len());
    for issue in &issues {
        match issue {
            Issue::ProjectMissing { path } => {
                println!("  - the project folder does not exist: {path}")
            }
            Issue::ProjectNotADirectory { path } => println!("  - not a folder: {path}"),
            Issue::HerdrMissing => println!("  - herdr is not installed"),
            Issue::HerdrTooOld { found, required } => {
                println!("  - herdr {found} is older than {required}")
            }
            Issue::HerdrProtocolMismatch { .. } => println!(
                "  - a herdr server on an older protocol is running. Restarting it exits\n    \
                 its panes, so herdup will not do it for you."
            ),
            Issue::ServerDown => println!("  - no server yet (herdup starts one at launch)"),
            Issue::CliMissing {
                display_name,
                install_command,
                docs_url,
                panes,
                cli,
            } => {
                println!(
                    "  - {display_name} not found, needed by {} pane(s)",
                    panes.len()
                );
                if let Some(cmd) = install_command {
                    println!("      install: {cmd}");
                } else if let Some(url) = docs_url {
                    println!("      docs:    {url}");
                }
                let alts = pf.alternatives_for(cli);
                if !alts.is_empty() {
                    println!(
                        "      or switch those panes to: {}",
                        alts.iter()
                            .map(|a| a.id.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
            }
        }
    }
    Ok(())
}

/// The Phase 6 milestone: preflight, first-run, build the team, hand off.
fn launch(args: &[String]) -> Result<(), AppError> {
    use launcher_core::execute::{Event, Executor, PaneState};
    use launcher_core::firstrun::{FirstRun, FirstRunState, FirstRunTarget};
    use launcher_core::preflight::{ensure_server, Issue, Preflight, SystemResolver};

    let Some(id) = flag(args, "--template") else {
        eprintln!("launch: --template ID is required");
        return Ok(());
    };
    let cwd = flag(args, "--cwd")
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let session = flag(args, "--session").unwrap_or_else(|| "herdup".to_string());
    let want_terminal = !args.iter().any(|a| a == "--no-terminal");
    let skip_first_run = args.iter().any(|a| a == "--skip-first-run");
    // One terminal per launch. First-run needs one so the user can answer
    // prompts, and that same window stays attached for the finished team.
    let mut terminal_opened = false;

    let registry = launcher_core::config::load_registry()?;
    let template = resolve_template("launch", &id, &cwd, &registry)?;
    let mut settings = launcher_core::Settings::load();

    let mut request = launcher_core::plan::LaunchRequest::new(&cwd, &template);
    for raw in flags(args, "--skip") {
        if let Ok(i) = raw.parse::<usize>() {
            request = request.skip_pane(i);
        }
    }
    for raw in flags(args, "--cli") {
        if let Some((i, cli)) = raw.split_once('=') {
            if let Ok(i) = i.parse::<usize>() {
                request = request.override_cli(i, cli);
            }
        }
    }
    let p = launcher_core::plan::plan(&request, &registry).map_err(AppError::Plan)?;

    println!("herdup launch");
    println!("  project  : {}", cwd.display());
    println!(
        "  template : {} ({} panes)",
        template.display_name,
        p.panes.len()
    );
    println!("  session  : {session}");

    // ---- Stage 0 --------------------------------------------------------
    let herdr = HerdrCli::discover()?.with_session(&session);
    let pf = Preflight::run(&herdr, &p, &registry, &settings, &SystemResolver);
    let blocking = pf.blocking_issues();
    if !blocking.is_empty() {
        println!("\nCannot launch — {} issue(s):", blocking.len());
        for issue in &blocking {
            println!("  - {}", issue.explain());
            if let Issue::CliMissing {
                install_command,
                cli,
                ..
            } = issue
            {
                if let Some(cmd) = install_command {
                    println!("      {cmd}");
                }
                let alts = pf.alternatives_for(cli);
                if !alts.is_empty() {
                    println!("      or re-run with --cli N={}", alts[0].id);
                }
            }
        }
        return Err(AppError::Herdr(HerdrError::Api {
            code: "preflight_blocked".into(),
            message: format!("{} unresolved issue(s)", blocking.len()),
        }));
    }

    // Warnings are not blockers, but a launch puts agents with file-editing
    // permissions into this folder. That must be a decision, never an accident.
    let warnings = pf.warnings();
    if !warnings.is_empty() && !args.iter().any(|a| a == "--yes") {
        println!(
            "\nLaunching {} agent(s) into {}",
            p.panes.len(),
            cwd.display()
        );
        for warning in &warnings {
            println!("  ! {}", warning.explain());
        }
        println!("\nNothing has been created. Re-run with --yes to proceed anyway.");
        return Ok(());
    }

    step("ensure server");
    if ensure_server(&herdr, Duration::from_secs(15))? {
        println!("   started a headless server for session '{session}'");
    } else {
        println!("   already running");
    }

    // ---- Stage 1 --------------------------------------------------------
    let pending: Vec<FirstRunTarget> = pf
        .needs_first_run()
        .iter()
        .map(|c| FirstRunTarget {
            cli: c.id.clone(),
            display_name: c.display_name.clone(),
            binary: c.binary.clone(),
        })
        .collect();

    if !pending.is_empty() && !skip_first_run {
        step("first run");
        println!(
            "   {} CLI(s) have not completed first run in this project.",
            pending.len()
        );
        println!("   Opening one bare pane each so logins and 'trust this folder'");
        println!("   prompts are cleared before the team is built.\n");

        let fr = FirstRun::new(&herdr);
        let mut fr_session = fr.start(&cwd, &pending, &mut |_| {})?;

        // The user has to interact with these panes, so show them a terminal.
        // The same window stays attached through the launch, so Stage 3 must not
        // open a second one on top of it.
        if want_terminal {
            match launcher_core::terminal::open_with_fallback(
                &cwd,
                Some(&session),
                settings.terminal.as_deref(),
            ) {
                Ok(h) => {
                    terminal_opened = true;
                    println!("   opened a terminal: {}", h.display());
                }
                Err(h) => println!("   could not open a terminal; run: {}", h.display()),
            }
        }

        let deadline = Instant::now() + Duration::from_secs(300);
        while !fr_session.all_verified() && Instant::now() < deadline {
            // Print on state *change* only. Printing every poll produced dozens
            // of identical "waiting for you" lines and buried the hints.
            fr.poll_once(&mut fr_session, &mut |event| match &event {
                launcher_core::firstrun::FirstRunEvent::HintFound { cli, hint } => {
                    println!("   [{cli}] {:?}: {}", hint.kind, hint.value)
                }
                launcher_core::firstrun::FirstRunEvent::StateChanged { cli, state } => {
                    match state {
                        FirstRunState::NeedsYou => {
                            println!("   [{cli}] waiting for you — answer it in the terminal")
                        }
                        FirstRunState::Verified => println!("   [{cli}] ready"),
                        FirstRunState::Waiting => {}
                    }
                }
                _ => {}
            });
            if fr_session.all_verified() {
                break;
            }
            std::thread::sleep(Duration::from_secs(2));
        }

        let done = fr_session.all_verified();
        fr_session.apply_to(&mut settings);
        let _ = settings.save();
        let _ = fr.teardown(&fr_session);

        if done {
            println!("   first run complete for all CLIs");
        } else {
            println!(
                "   first run did not finish; the team will still launch, but\n   \
                 briefings may be withheld until you answer each pane."
            );
        }
    }

    // ---- Stage 2 --------------------------------------------------------
    step("build team");
    let outcome = Executor::new(&herdr).execute(&p, &mut |event| match event {
        Event::PaneCreated { role, pane_id, .. } => println!("   + {role:<12} {pane_id}"),
        Event::Briefed { role, .. } => println!("   briefed {role}"),
        Event::BriefingWithheld { role, reason, .. } => {
            println!("   held briefing for {role}: {}", reason.explain())
        }
        Event::Failed { message, .. } => println!("   ! {message}"),
        _ => {}
    });

    // ---- Report ---------------------------------------------------------
    println!("\nresult:");
    for pane in &outcome.panes {
        let state = match &pane.state {
            PaneState::Briefed => "briefed".to_string(),
            PaneState::Ready => "ready, not briefed".to_string(),
            PaneState::Starting => "starting".to_string(),
            PaneState::NotCreated => "not created".to_string(),
            PaneState::NeedsAttention(r) => format!("NEEDS YOU — {}", r.explain()),
        };
        println!(
            "  {:<12} {:<10} {}",
            pane.role,
            pane.pane_id.as_deref().unwrap_or("-"),
            state
        );
    }

    if let Some(failure) = &outcome.failure {
        println!(
            "\nStopped at step {}: {}\n  {}",
            failure.step_index + 1,
            failure.description,
            failure.message
        );
        println!("  Earlier panes were left running on purpose — they may hold real work.");
    }

    let attention = outcome.needing_attention();
    if !attention.is_empty() {
        println!(
            "\n{} pane(s) need you before they are briefed:",
            attention.len()
        );
        for pane in &attention {
            println!(
                "  {} ({})",
                pane.role,
                pane.pane_id.as_deref().unwrap_or("-")
            );
            if let Some(screen) = &pane.screen {
                for line in screen
                    .lines()
                    .rev()
                    .take(3)
                    .collect::<Vec<_>>()
                    .iter()
                    .rev()
                {
                    if !line.trim().is_empty() {
                        println!("      | {}", line.trim_end());
                    }
                }
            }
        }
    }

    // ---- Stage 3 --------------------------------------------------------
    if terminal_opened {
        println!("\nYour terminal is already attached to session '{session}'.");
    } else if want_terminal {
        step("hand off");
        match launcher_core::terminal::open_with_fallback(
            &cwd,
            Some(&session),
            settings.terminal.as_deref(),
        ) {
            Ok(h) => println!("   {}", h.display()),
            Err(h) => println!(
                "   could not open a terminal. Run this yourself:\n   {}",
                h.display()
            ),
        }
    } else {
        println!("\nAttach with:\n  herdr --session {session}");
    }

    println!(
        "\n{} of {} pane(s) briefed.",
        outcome.briefed(),
        outcome.panes.len()
    );
    Ok(())
}

/// Create a GitHub repository, clone it, and optionally launch a team into it.
fn new_repo(args: &[String]) -> Result<(), AppError> {
    use launcher_core::github::{Gh, NewRepo, Visibility};

    let Some(name) = flag(args, "--name") else {
        eprintln!("new-repo: --name NAME is required");
        return Ok(());
    };
    let into = flag(args, "--into")
        .map(PathBuf::from)
        .or_else(|| launcher_core::Settings::load().projects_root_path())
        .unwrap_or(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let gh = Gh::discover().map_err(AppError::Gh)?;
    let mut repo = NewRepo::private(&name, &into);
    repo.owner = flag(args, "--owner");
    repo.description = flag(args, "--description");
    if args.iter().any(|a| a == "--public") {
        repo.visibility = Visibility::Public;
    }

    // Everything checkable, checked before anything is created.
    repo.validate().map_err(AppError::Gh)?;
    if !gh.is_authenticated() {
        return Err(AppError::Gh(
            launcher_core::github::GhError::NotAuthenticated,
        ));
    }

    let owner_label = repo.owner.clone().unwrap_or_else(|| {
        gh.owners()
            .first()
            .cloned()
            .unwrap_or_else(|| "your account".into())
    });
    println!(
        "Create a {} repository:",
        match repo.visibility {
            Visibility::Private => "PRIVATE",
            Visibility::Public => "PUBLIC",
        }
    );
    println!("  {owner_label}/{name}");
    println!("  clone into {}", repo.destination().display());
    if repo.visibility == Visibility::Public {
        println!("\n  A public repository is visible to anyone.");
    }

    // Creating a repository is outward-facing and cannot be quietly undone, so
    // it never happens without an explicit yes.
    if !args.iter().any(|a| a == "--yes") {
        println!("\nNothing has been created. Re-run with --yes to proceed.");
        return Ok(());
    }

    println!("\n-> {}", repo.display_command());
    let created = gh.create(&repo).map_err(AppError::Gh)?;
    println!(
        "   created {}",
        created.url.as_deref().unwrap_or("(url unknown)")
    );
    println!("   cloned to {}", created.path.display());

    match flag(args, "--template") {
        Some(template) => {
            println!("\nLaunching '{template}' into the new repository…\n");
            let mut launch_args = vec![
                "--template".to_string(),
                template,
                "--cwd".to_string(),
                created.path.display().to_string(),
                // A brand-new clone is a clean repo, so no warning would fire —
                // but pass --yes so the flow does not stop on a technicality.
                "--yes".to_string(),
            ];
            launch_args.extend(args.iter().filter(|a| *a == "--no-terminal").cloned());
            launch(&launch_args)
        }
        None => {
            println!("\nLaunch a team into it with:");
            println!(
                "  launcher-cli launch --template squad --cwd {}",
                created.path.display()
            );
            Ok(())
        }
    }
}

fn smoke(args: &[String]) -> Result<(), AppError> {
    let session = flag(args, "--session").unwrap_or_else(|| DEFAULT_SMOKE_SESSION.to_string());
    let cwd = flag(args, "--cwd")
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let cli = HerdrCli::discover()?.with_session(&session);
    let version = cli.check_version()?;
    println!("herdr {version} in session '{session}'");
    println!("project: {}", cwd.display());

    // Isolated session, so anything that goes wrong here cannot reach the
    // developer's live panes.
    let outcome = run_smoke(&cli, &cwd);

    print!("\ncleanup: ");
    match cli
        .session_stop(&session)
        .and_then(|_| cli.session_delete(&session))
    {
        Ok(()) => println!("session '{session}' stopped and deleted"),
        Err(e) => println!("could not fully clean up session '{session}': {e}"),
    }

    outcome?;
    println!("\nSMOKE PASSED");
    Ok(())
}

fn run_smoke(cli: &HerdrCli, cwd: &Path) -> Result<(), HerdrError> {
    step("ensure server");
    if !cli.server_running() {
        reap_in_background(cli.start_server()?, Duration::ZERO);
        wait_for_server(cli, Duration::from_secs(15))?;
        println!("   started a headless server");
    } else {
        println!("   already running");
    }

    step("workspace create");
    let created = cli.workspace_create(cwd, Some("herdup smoke"), false)?;
    let ws = created.workspace.workspace_id.clone();
    let root = created.root_pane.pane_id.clone();
    println!("   workspace={ws} tab={} root={root}", created.tab.tab_id);

    step("pane split");
    let pane = cli.pane_split(&root, SplitDirection::Right, Some(0.5), Some(cwd), false)?;
    println!("   new pane={}", pane.pane_id);
    assert_that(pane.pane_id != root, "split returned a distinct pane id")?;

    step("pane rename");
    let renamed = cli.pane_rename(&pane.pane_id, "QA")?;
    assert_that(renamed.label.as_deref() == Some("QA"), "label applied")?;
    println!("   label={:?}", renamed.label);

    step("pane list");
    let panes = cli.pane_list()?;
    println!("   {} pane(s): {:?}", panes.len(), ids(&panes));
    assert_that(panes.len() == 2, "two panes exist")?;

    step("cwd normalisation");
    let got = cli.pane_get(&pane.pane_id)?;
    println!(
        "   raw={:?} normalised={}",
        got.cwd,
        got.cwd_path().display()
    );
    assert_that(
        got.cwd_is(cwd),
        "pane cwd matches the project after normalising",
    )?;

    step("id stability after close");
    // 0.8.x allocates monotonically; a reused id here would invalidate the
    // planning assumptions in the spec.
    cli.pane_close(&pane.pane_id)?;
    let reborn = cli.pane_split(&root, SplitDirection::Down, None, Some(cwd), false)?;
    println!("   closed {} then created {}", pane.pane_id, reborn.pane_id);
    assert_that(
        reborn.pane_id != pane.pane_id,
        "pane ids are monotonic, not reused",
    )?;

    step("workspace close");
    cli.workspace_close(&ws)?;
    println!("   closed {ws}");
    Ok(())
}

fn wait_for_server(cli: &HerdrCli, timeout: Duration) -> Result<(), HerdrError> {
    let start = Instant::now();
    loop {
        match cli.workspace_list() {
            Ok(_) => return Ok(()),
            Err(e) if !e.is_recoverable_by_starting_server() => return Err(e),
            Err(e) => {
                if start.elapsed() > timeout {
                    return Err(e);
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        }
    }
}

fn ids(panes: &[launcher_core::herdr::Pane]) -> Vec<&str> {
    panes.iter().map(|p| p.pane_id.as_str()).collect()
}

fn step(name: &str) {
    println!("\n-> {name}");
}

fn assert_that(cond: bool, what: &str) -> Result<(), HerdrError> {
    if cond {
        println!("   ok: {what}");
        Ok(())
    } else {
        Err(HerdrError::Api {
            code: "smoke_assertion".into(),
            message: format!("expected: {what}"),
        })
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    let i = args.iter().position(|a| a == name)?;
    args.get(i + 1).cloned()
}

/// All values of a repeatable flag, e.g. `--skip 1 --skip 3`.
fn flags(args: &[String], name: &str) -> Vec<String> {
    args.iter()
        .enumerate()
        .filter(|(_, a)| *a == name)
        .filter_map(|(i, _)| args.get(i + 1).cloned())
        .collect()
}

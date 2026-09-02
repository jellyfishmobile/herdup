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
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const DEFAULT_SMOKE_SESSION: &str = "herdup-smoke";

/// Either failure mode a subcommand can hit.
#[derive(Debug)]
enum AppError {
    Herdr(HerdrError),
    Config(ConfigError),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::Herdr(e) => write!(f, "{e}"),
            AppError::Config(e) => write!(f, "{e}"),
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
         launcher-cli smoke [--session NAME] [--cwd PATH]\n\n\
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
        cli.start_server()?;
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

//! Stage 0: what is missing before anything is created.
//!
//! Reports rather than decides. Remediations are returned as data so the UI can
//! offer them; nothing here installs, launches or changes state, with the single
//! explicit exception of [`ensure_server`].

use crate::herdr::{HerdrCli, HerdrError};
use crate::plan::{LaunchPlan, PaneRef};
use crate::registry::Registry;
use crate::settings::Settings;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Finds an executable by base name.
///
/// Behind a trait so tests can stub PATH. Base name, never a filename: Phase 0
/// found four CLIs installed in three different shapes on one machine.
pub trait BinaryResolver {
    fn resolve(&self, base_name: &str) -> Option<PathBuf>;
}

/// Uses `where.exe` on Windows and `which` elsewhere.
pub struct SystemResolver;

impl BinaryResolver for SystemResolver {
    fn resolve(&self, base_name: &str) -> Option<PathBuf> {
        crate::herdr::which(base_name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HerdrStatus {
    Missing,
    TooOld {
        found: String,
        required: String,
    },
    /// No server for this session. Recoverable — see [`ensure_server`].
    ServerDown {
        version: String,
    },
    /// A server from a different binary is running. **Not** recoverable by us:
    /// fixing it means stopping the user's server, which kills their panes.
    ProtocolMismatch {
        version: String,
        message: String,
    },
    Ready {
        version: String,
    },
}

impl HerdrStatus {
    pub fn is_ready(&self) -> bool {
        matches!(self, HerdrStatus::Ready { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliStatus {
    pub id: String,
    pub display_name: String,
    pub binary: String,
    /// Absolute path if found on PATH.
    pub resolved: Option<PathBuf>,
    pub install_command: Option<String>,
    pub docs_url: Option<String>,
    /// Cached first-run completion for this project. A hint, not a guarantee.
    pub first_run_done: bool,
    /// Panes in the plan that use this CLI.
    pub panes: Vec<PaneRef>,
}

impl CliStatus {
    pub fn installed(&self) -> bool {
        self.resolved.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhStatus {
    pub installed: bool,
    pub authenticated: bool,
    pub account: Option<String>,
}

impl GhStatus {
    pub fn usable(&self) -> bool {
        self.installed && self.authenticated
    }

    /// Why the new-repo flow is unavailable, if it is.
    pub fn blocker(&self) -> Option<&'static str> {
        if !self.installed {
            Some("the GitHub CLI (`gh`) is not installed")
        } else if !self.authenticated {
            Some("`gh` is installed but not signed in — run `gh auth login`")
        } else {
            None
        }
    }
}

/// Something that must be resolved before launching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Issue {
    HerdrMissing,
    HerdrTooOld {
        found: String,
        required: String,
    },
    HerdrProtocolMismatch {
        message: String,
    },
    ServerDown,
    CliMissing {
        cli: String,
        display_name: String,
        install_command: Option<String>,
        docs_url: Option<String>,
        panes: Vec<PaneRef>,
    },
}

impl Issue {
    /// Whether starting a server would clear this.
    pub fn is_server_startable(&self) -> bool {
        matches!(self, Issue::ServerDown)
    }
}

#[derive(Debug, Clone)]
pub struct Preflight {
    pub herdr: HerdrStatus,
    pub clis: Vec<CliStatus>,
    pub project: PathBuf,
}

impl Preflight {
    /// Inspect the environment against a plan. Read-only.
    pub fn run(
        herdr: &HerdrCli,
        plan: &LaunchPlan,
        registry: &Registry,
        settings: &Settings,
        resolver: &dyn BinaryResolver,
    ) -> Preflight {
        Preflight {
            herdr: check_herdr(herdr),
            clis: check_clis(plan, registry, settings, resolver),
            project: plan.project.clone(),
        }
    }

    /// Everything blocking a launch, most fundamental first.
    pub fn issues(&self) -> Vec<Issue> {
        let mut issues = Vec::new();
        match &self.herdr {
            HerdrStatus::Missing => issues.push(Issue::HerdrMissing),
            HerdrStatus::TooOld { found, required } => issues.push(Issue::HerdrTooOld {
                found: found.clone(),
                required: required.clone(),
            }),
            HerdrStatus::ProtocolMismatch { message, .. } => {
                issues.push(Issue::HerdrProtocolMismatch {
                    message: message.clone(),
                })
            }
            HerdrStatus::ServerDown { .. } => issues.push(Issue::ServerDown),
            HerdrStatus::Ready { .. } => {}
        }
        for cli in &self.clis {
            if !cli.installed() {
                issues.push(Issue::CliMissing {
                    cli: cli.id.clone(),
                    display_name: cli.display_name.clone(),
                    install_command: cli.install_command.clone(),
                    docs_url: cli.docs_url.clone(),
                    panes: cli.panes.clone(),
                });
            }
        }
        issues
    }

    /// Issues herdup cannot resolve on its own.
    ///
    /// A stopped server is *not* one of them: herdup starts one for its own
    /// session at launch. Listing it as blocking would tell the user to go fix
    /// something that needs no fixing.
    pub fn blocking_issues(&self) -> Vec<Issue> {
        self.issues()
            .into_iter()
            .filter(|i| !i.is_server_startable())
            .collect()
    }

    /// Issues herdup will clear by itself, worth showing but not acting on.
    pub fn auto_resolvable_issues(&self) -> Vec<Issue> {
        self.issues()
            .into_iter()
            .filter(Issue::is_server_startable)
            .collect()
    }

    pub fn can_launch(&self) -> bool {
        self.blocking_issues().is_empty()
    }

    /// CLIs that still need a first-run pass in this project.
    ///
    /// Installed but unverified — a missing CLI is a blocking issue, not a
    /// first-run candidate.
    pub fn needs_first_run(&self) -> Vec<&CliStatus> {
        self.clis
            .iter()
            .filter(|c| c.installed() && !c.first_run_done)
            .collect()
    }

    /// Installed CLIs that could stand in for a missing one.
    ///
    /// Offered as the "switch this role's CLI" remediation.
    pub fn alternatives_for(&self, missing: &str) -> Vec<&CliStatus> {
        self.clis
            .iter()
            .filter(|c| c.id != missing && c.installed())
            .collect()
    }
}

/// The one side-effecting helper: start a headless server and wait for it.
///
/// Only ever called for the session herdup owns. It never touches the user's
/// default session, whose panes may be real work.
pub fn ensure_server(herdr: &HerdrCli, timeout: Duration) -> Result<bool, HerdrError> {
    if herdr.server_running() {
        return Ok(false);
    }
    herdr.start_server()?;
    let start = Instant::now();
    loop {
        match herdr.workspace_list() {
            Ok(_) => return Ok(true),
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

fn check_herdr(herdr: &HerdrCli) -> HerdrStatus {
    let version = match herdr.check_version() {
        Ok(v) => v.to_string(),
        Err(HerdrError::NotFound) => return HerdrStatus::Missing,
        Err(HerdrError::VersionTooOld { found, required }) => {
            return HerdrStatus::TooOld { found, required }
        }
        // Any other failure to even read a version means we cannot use it.
        Err(_) => return HerdrStatus::Missing,
    };

    match herdr.workspace_list() {
        Ok(_) => HerdrStatus::Ready { version },
        Err(HerdrError::ServerUnavailable { .. }) => HerdrStatus::ServerDown { version },
        Err(HerdrError::ProtocolMismatch { message }) => {
            HerdrStatus::ProtocolMismatch { version, message }
        }
        // An unexpected error is not a reason to claim the server is fine.
        Err(e) => HerdrStatus::ProtocolMismatch {
            version,
            message: e.to_string(),
        },
    }
}

fn check_clis(
    plan: &LaunchPlan,
    registry: &Registry,
    settings: &Settings,
    resolver: &dyn BinaryResolver,
) -> Vec<CliStatus> {
    // Per distinct CLI, not per pane: three claude panes need one install check
    // and one first-run pass.
    plan.distinct_clis()
        .into_iter()
        .map(|id| {
            let entry = registry.get(id);
            let binary = entry
                .map(|e| e.binary.clone())
                .unwrap_or_else(|| id.to_string());
            CliStatus {
                id: id.to_string(),
                display_name: entry
                    .map(|e| e.display_name.clone())
                    .unwrap_or_else(|| id.to_string()),
                resolved: resolver.resolve(&binary),
                binary,
                install_command: entry.and_then(|e| e.install_command().map(str::to_string)),
                docs_url: entry.and_then(|e| e.docs_url.clone()),
                first_run_done: settings.is_verified(id, &plan.project),
                panes: plan
                    .panes
                    .iter()
                    .filter(|p| p.cli == id)
                    .map(|p| p.pane)
                    .collect(),
            }
        })
        .collect()
}

/// Check the GitHub CLI. Only needed for the new-repo flow.
///
/// `gh auth status` is the one auth probe that is documented and stable — it
/// exits non-zero when logged out. Everything else herdup checks by observation.
pub fn check_gh() -> GhStatus {
    let Some(path) = crate::herdr::which("gh") else {
        return GhStatus {
            installed: false,
            authenticated: false,
            account: None,
        };
    };
    let out = Command::new(path)
        .args(["auth", "status"])
        .stdin(Stdio::null())
        .output();
    match out {
        Ok(out) => {
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            GhStatus {
                installed: true,
                authenticated: out.status.success(),
                account: parse_gh_account(&text),
            }
        }
        Err(_) => GhStatus {
            installed: true,
            authenticated: false,
            account: None,
        },
    }
}

/// Pull the account out of `Logged in to github.com account NAME (keyring)`.
fn parse_gh_account(text: &str) -> Option<String> {
    let line = text.lines().find(|l| l.contains("account "))?;
    let after = line.split("account ").nth(1)?;
    let name = after.split_whitespace().next()?;
    if name.is_empty() {
        None
    } else {
        Some(name.trim_end_matches([',', '.']).to_string())
    }
}

/// Convenience for a UI that wants one line per CLI.
pub fn summarise(status: &CliStatus) -> String {
    match &status.resolved {
        Some(path) => format!(
            "{:<18} found  {}{}",
            status.id,
            path.display(),
            if status.first_run_done {
                "  (first-run done)"
            } else {
                "  (first-run needed)"
            }
        ),
        None => format!(
            "{:<18} NOT FOUND  (looked for `{}`)",
            status.id, status.binary
        ),
    }
}

/// Where a project's own path sits, for display.
pub fn project_name(project: &Path) -> String {
    project
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| project.display().to_string())
}

//! Stage 0: what is missing before anything is created.
//!
//! Reports rather than decides. Remediations are returned as data so the UI can
//! offer them; nothing here installs, launches or changes state, with the single
//! explicit exception of [`ensure_server`].

use crate::herdr::{HerdrCli, HerdrError};
use crate::plan::{LaunchPlan, PaneRef};
use crate::registry::Registry;
use crate::settings::Settings;
use crate::terminal::reap_in_background;
use std::path::{Path, PathBuf};
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

/// What version control can tell us about the project.
///
/// Agents launched here run with permission flags that let them edit files. Git
/// state is the difference between "a change I can undo" and "a change I cannot".
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GitStatus {
    pub is_repo: bool,
    pub branch: Option<String>,
    /// Count of modified, staged and untracked entries.
    pub dirty_files: usize,
}

impl GitStatus {
    pub fn is_dirty(&self) -> bool {
        self.is_repo && self.dirty_files > 0
    }
}

/// Something worth a human's explicit acknowledgement, but not a blocker.
///
/// Kept distinct from [`Issue`]: an issue means herdup cannot launch, a warning
/// means it should not launch *silently*.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Warning {
    /// Uncommitted work is already in the tree. Anything an agent changes will
    /// be mixed into it, and `git checkout` is no longer a clean undo.
    UncommittedChanges {
        count: usize,
        branch: Option<String>,
    },
    /// No version control at all: nothing an agent does here can be undone.
    NotAGitRepo,
}

impl Warning {
    pub fn explain(&self) -> String {
        match self {
            Warning::UncommittedChanges { count, branch } => format!(
                "{count} uncommitted change(s) on {}. Anything the agents edit will be mixed \
                 into work you have not committed, so `git checkout` is no longer a clean undo. \
                 Commit or stash first if you want a way back.",
                branch.as_deref().unwrap_or("this branch")
            ),
            Warning::NotAGitRepo => "This folder is not a git repository, so there is no way to \
                undo anything the agents change. Consider `git init` first."
                .into(),
        }
    }
}

/// Something that must be resolved before launching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Issue {
    /// The project folder does not exist.
    ProjectMissing {
        path: String,
    },
    /// The path exists but is a file, not a directory.
    ProjectNotADirectory {
        path: String,
    },
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

    /// A sentence for a human.
    ///
    /// Lives here so the CLI, the GUI and any future surface say the same thing.
    /// It previously did not, and a `{:?}` fallback leaked
    /// `ProjectMissing { path: "..." }` into the terminal.
    pub fn explain(&self) -> String {
        match self {
            Issue::ProjectMissing { path } => {
                format!("the project folder does not exist: {path}")
            }
            Issue::ProjectNotADirectory { path } => {
                format!("that path is a file, not a folder: {path}")
            }
            Issue::HerdrMissing => "herdr is not installed".into(),
            Issue::HerdrTooOld { found, required } => {
                format!("herdr {found} is older than the required {required}")
            }
            Issue::HerdrProtocolMismatch { .. } => {
                "a herdr server on a different protocol is running; restarting it would exit \
                 its panes, so herdup will not do it for you"
                    .into()
            }
            Issue::ServerDown => "no herdr server yet (herdup starts one at launch)".into(),
            Issue::CliMissing { display_name, .. } => {
                format!("{display_name} is not installed")
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct Preflight {
    pub herdr: HerdrStatus,
    pub clis: Vec<CliStatus>,
    pub project: PathBuf,
    pub project_exists: bool,
    pub project_is_dir: bool,
    pub git: GitStatus,
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
        let project = plan.project.clone();
        Preflight {
            herdr: check_herdr(herdr),
            clis: check_clis(plan, registry, settings, resolver),
            project_exists: project.exists(),
            project_is_dir: project.is_dir(),
            git: check_git(&project),
            project,
        }
    }

    /// Things a human should acknowledge before agents start editing.
    ///
    /// Never blocking: the user may genuinely want to launch into a dirty tree.
    /// But it must be a decision, not an accident.
    pub fn warnings(&self) -> Vec<Warning> {
        if !self.project_is_dir {
            return Vec::new(); // the missing-project issue covers it
        }
        let mut out = Vec::new();
        if !self.git.is_repo {
            out.push(Warning::NotAGitRepo);
        } else if self.git.is_dirty() {
            out.push(Warning::UncommittedChanges {
                count: self.git.dirty_files,
                branch: self.git.branch.clone(),
            });
        }
        out
    }

    /// Everything blocking a launch, most fundamental first.
    pub fn issues(&self) -> Vec<Issue> {
        let mut issues = Vec::new();

        // Checked first: launching agents into a path that is not a project is
        // never what someone meant, and a typo'd path should never reach herdr.
        if !self.project_exists {
            issues.push(Issue::ProjectMissing {
                path: self.project.display().to_string(),
            });
        } else if !self.project_is_dir {
            issues.push(Issue::ProjectNotADirectory {
                path: self.project.display().to_string(),
            });
        }

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
    // The server is meant to outlive this call, so its reaping goes straight
    // to a thread rather than leaving a zombie whenever it does stop.
    reap_in_background(herdr.start_server()?, Duration::ZERO);
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

/// Read version-control state for the project.
///
/// Best-effort and read-only: `git` may be absent, and a failure here must never
/// stop a launch — it only means we cannot warn.
///
/// Public because the launcher shows the "nothing can be undone" warning at the
/// moment a project is chosen, which is long before the full preflight runs.
pub fn git_status(project: &Path) -> GitStatus {
    check_git(project)
}

fn check_git(project: &Path) -> GitStatus {
    if !project.is_dir() {
        return GitStatus::default();
    }
    let run = |args: &[&str]| -> Option<String> {
        let out = crate::proc::hidden_command("git")
            .args(args)
            .current_dir(project)
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    };

    // `--is-inside-work-tree` is false for a plain directory and errors outside
    // a repo, so its success is the repo test.
    let is_repo = run(&["rev-parse", "--is-inside-work-tree"])
        .map(|s| s.trim() == "true")
        .unwrap_or(false);
    if !is_repo {
        return GitStatus::default();
    }

    GitStatus {
        is_repo: true,
        branch: run(&["rev-parse", "--abbrev-ref", "HEAD"])
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        // --porcelain counts modified, staged and untracked alike, which is the
        // right notion of "work that is not safely committed".
        dirty_files: run(&["status", "--porcelain"])
            .map(|s| s.lines().filter(|l| !l.trim().is_empty()).count())
            .unwrap_or(0),
    }
}

/// Platform caveats worth stating plainly, if any.
///
/// herdr's Windows builds are preview-only beta and default to the preview
/// update channel — its own README says so. Someone trusting herdup with a real
/// repository should know that the thing underneath it is beta, rather than
/// discover it when something misbehaves.
pub fn platform_note() -> Option<&'static str> {
    if cfg!(windows) {
        Some(
            "herdr's Windows builds are preview-only beta and track the preview update \
             channel. Linux and macOS have stable releases; Windows does not yet.",
        )
    } else {
        None
    }
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
    let out = crate::proc::hidden_command(path)
        .args(["auth", "status"])
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

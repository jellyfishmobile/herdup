//! Typed wrapper over the `herdr` binary.
//!
//! This is the **only** module that knows herdr's command shapes (spec §6.1).
//! It makes no policy decisions: it maps Rust calls onto herdr commands and
//! parses what comes back.
//!
//! Every invocation spawns `herdr` directly with an argv vector and **never
//! through a shell** (spec §6.3). Briefing text, repo paths and labels are
//! user-controlled data; routing them through `cmd.exe` or `sh -c` would be a
//! command-injection vector and would break on paths containing spaces.

pub mod error;
pub mod types;

pub use error::{HerdrError, Result};
pub use types::{
    AgentInfo, AgentStatus, Pane, ReadSource, SplitDirection, Tab, TabCreated, Version,
    WaitOutcome, Workspace, WorkspaceCreated,
};

use serde::de::DeserializeOwned;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use types::{AgentEnvelope, AgentList, PaneInfo, PaneList, WorkspaceList};

/// Minimum herdr this design supports.
///
/// Below 0.8.x pane IDs compact when panes close, which the launcher's planning
/// assumes they do not (ground truth §3.2).
pub const MIN_HERDR: (u32, u32, u32) = (0, 8, 2);

struct Raw {
    stdout: String,
    stderr: String,
    code: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct HerdrCli {
    exe: PathBuf,
    session: Option<String>,
    env: Vec<(String, String)>,
}

impl HerdrCli {
    pub fn new(exe: impl Into<PathBuf>) -> Self {
        HerdrCli {
            exe: exe.into(),
            session: None,
            env: Vec::new(),
        }
    }

    /// Set an environment variable on every spawned command.
    ///
    /// Scoped to this client rather than the process, so parallel callers (and
    /// parallel tests) cannot clobber each other the way `std::env::set_var`
    /// would.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// Scope every command to a named herdr session.
    ///
    /// Strongly recommended for anything automated. A named session gets its own
    /// socket *and* state directory, so it cannot disturb a developer's live
    /// panes — `HERDR_SOCKET_PATH` alone does not achieve this (ground truth §2).
    pub fn with_session(mut self, name: impl Into<String>) -> Self {
        self.session = Some(name.into());
        self
    }

    pub fn session(&self) -> Option<&str> {
        self.session.as_deref()
    }

    pub fn exe(&self) -> &Path {
        &self.exe
    }

    /// Locate `herdr` on PATH, falling back to its documented install location.
    pub fn discover() -> Result<Self> {
        if let Some(path) = which("herdr") {
            return Ok(HerdrCli::new(path));
        }
        for candidate in default_install_paths() {
            if is_runnable(&candidate) {
                return Ok(HerdrCli::new(candidate));
            }
        }
        Err(HerdrError::NotFound)
    }

    // ---- version -------------------------------------------------------

    pub fn version(&self) -> Result<Version> {
        let raw = self.invoke(&["--version".into()])?;
        let text = if raw.stdout.trim().is_empty() {
            raw.stderr
        } else {
            raw.stdout
        };
        Version::parse(&text).ok_or(HerdrError::UnparsableVersion(text))
    }

    /// Verify the installed herdr meets [`MIN_HERDR`].
    pub fn check_version(&self) -> Result<Version> {
        let found = self.version()?;
        if found.at_least(MIN_HERDR.0, MIN_HERDR.1, MIN_HERDR.2) {
            Ok(found)
        } else {
            Err(HerdrError::VersionTooOld {
                found: found.to_string(),
                required: format!("{}.{}.{}", MIN_HERDR.0, MIN_HERDR.1, MIN_HERDR.2),
            })
        }
    }

    // ---- workspaces ----------------------------------------------------

    pub fn workspace_list(&self) -> Result<Vec<Workspace>> {
        let list: WorkspaceList = self.run_json("workspace list", &args(["workspace", "list"]))?;
        Ok(list.workspaces)
    }

    pub fn workspace_create(
        &self,
        cwd: &Path,
        label: Option<&str>,
        focus: bool,
    ) -> Result<WorkspaceCreated> {
        let mut a = args(["workspace", "create", "--cwd"]);
        a.push(cwd.as_os_str().to_string_lossy().into_owned());
        if let Some(label) = label {
            a.push("--label".into());
            a.push(label.to_string());
        }
        a.push(if focus {
            "--focus".into()
        } else {
            "--no-focus".into()
        });
        self.run_json("workspace create", &a)
    }

    /// Make a workspace the focused one, so an attaching terminal opens on it.
    pub fn workspace_focus(&self, workspace_id: &str) -> Result<()> {
        self.run_unit(
            "workspace focus",
            &args(["workspace", "focus", workspace_id]),
        )
    }

    pub fn workspace_close(&self, workspace_id: &str) -> Result<()> {
        self.run_unit(
            "workspace close",
            &args(["workspace", "close", workspace_id]),
        )
    }

    // ---- tabs ----------------------------------------------------------

    pub fn tab_create(&self, workspace_id: &str, label: Option<&str>) -> Result<TabCreated> {
        let mut a = args(["tab", "create", "--workspace", workspace_id]);
        if let Some(label) = label {
            a.push("--label".into());
            a.push(label.to_string());
        }
        self.run_json("tab create", &a)
    }

    pub fn tab_close(&self, tab_id: &str) -> Result<()> {
        self.run_unit("tab close", &args(["tab", "close", tab_id]))
    }

    // ---- panes ---------------------------------------------------------

    pub fn pane_list(&self) -> Result<Vec<Pane>> {
        let list: PaneList = self.run_json("pane list", &args(["pane", "list"]))?;
        Ok(list.panes)
    }

    pub fn pane_get(&self, pane_id: &str) -> Result<Pane> {
        let info: PaneInfo = self.run_json("pane get", &args(["pane", "get", pane_id]))?;
        Ok(info.pane)
    }

    pub fn pane_split(
        &self,
        from_pane: &str,
        direction: SplitDirection,
        ratio: Option<f32>,
        cwd: Option<&Path>,
        focus: bool,
    ) -> Result<Pane> {
        let mut a = args([
            "pane",
            "split",
            from_pane,
            "--direction",
            direction.as_str(),
        ]);
        if let Some(ratio) = ratio {
            a.push("--ratio".into());
            a.push(ratio.to_string());
        }
        if let Some(cwd) = cwd {
            a.push("--cwd".into());
            a.push(cwd.as_os_str().to_string_lossy().into_owned());
        }
        a.push(if focus {
            "--focus".into()
        } else {
            "--no-focus".into()
        });
        let info: PaneInfo = self.run_json("pane split", &a)?;
        Ok(info.pane)
    }

    /// Rename a pane. This is how roles get their durable handle — pane IDs can
    /// change if a pane is closed and recreated, labels do not.
    pub fn pane_rename(&self, pane_id: &str, label: &str) -> Result<Pane> {
        let info: PaneInfo =
            self.run_json("pane rename", &args(["pane", "rename", pane_id, label]))?;
        Ok(info.pane)
    }

    /// Send a command line plus Enter, as if typed into the pane's shell.
    ///
    /// Unlike this wrapper's own argv handling, `command` *is* interpreted by the
    /// pane's shell — that is the point of it.
    pub fn pane_run(&self, pane_id: &str, command: &str) -> Result<()> {
        self.run_unit("pane run", &args(["pane", "run", pane_id, command]))
    }

    /// Type text into a pane without pressing Enter.
    ///
    /// Callers must not pass raw newlines: most agent CLIs submit on newline, so
    /// a multi-line string fires as several truncated prompts. Flatten first.
    pub fn pane_send_text(&self, pane_id: &str, text: &str) -> Result<()> {
        self.run_unit(
            "pane send-text",
            &args(["pane", "send-text", pane_id, text]),
        )
    }

    pub fn pane_send_keys(&self, pane_id: &str, keys: &[&str]) -> Result<()> {
        let mut a = args(["pane", "send-keys", pane_id]);
        a.extend(keys.iter().map(|k| k.to_string()));
        self.run_unit("pane send-keys", &a)
    }

    /// Read a pane's transcript. Returns text, not JSON.
    pub fn pane_read(&self, pane_id: &str, source: ReadSource, lines: u32) -> Result<String> {
        let a = args([
            "pane",
            "read",
            pane_id,
            "--source",
            source.as_str(),
            "--lines",
            &lines.to_string(),
        ]);
        self.run_text("pane read", &a)
    }

    pub fn pane_close(&self, pane_id: &str) -> Result<()> {
        self.run_unit("pane close", &args(["pane", "close", pane_id]))
    }

    // ---- agents --------------------------------------------------------
    //
    // herdr 0.8.2's agent surface. Preferred over the raw pane commands
    // wherever a CLI has a herdr `kind`, because herdr validates agent identity
    // and readiness itself:
    //   * `agent start` returns only once the agent is ready for input
    //   * `agent prompt` refuses to type into an agent at a dialog, *before*
    //     writing any bytes
    // Both are stronger than inferring readiness from a separate status read.

    /// Start an agent in an existing shell pane and wait for it to be ready.
    ///
    /// Returns [`HerdrError::AgentNotReady`] if the agent blocks during startup
    /// — a login or first-run prompt. That is not a failure: the name stays
    /// usable for reading the pane and answering it.
    ///
    /// `agent_args` are passed through to the agent after `--`.
    pub fn agent_start(
        &self,
        name: &str,
        kind: &str,
        pane_id: &str,
        timeout_ms: u64,
        agent_args: &[String],
    ) -> Result<AgentInfo> {
        let mut a = args(["agent", "start", name, "--kind", kind, "--pane", pane_id]);
        a.push("--timeout".into());
        a.push(timeout_ms.to_string());
        if !agent_args.is_empty() {
            a.push("--".into());
            a.extend(agent_args.iter().cloned());
        }
        let env: AgentEnvelope = self.run_json_agent("agent start", name, &a)?;
        Ok(env.agent)
    }

    /// Send a prompt to a running agent.
    ///
    /// With `wait`, herdr blocks until the agent reaches a settled state.
    /// Returns [`HerdrError::AgentBlocked`] — **without having sent anything** —
    /// if the agent is sitting at an approval or question dialog.
    pub fn agent_prompt(
        &self,
        target: &str,
        text: &str,
        wait: bool,
        timeout_ms: u64,
    ) -> Result<AgentInfo> {
        let mut a = args(["agent", "prompt", target, text]);
        if wait {
            a.push("--wait".into());
        }
        a.push("--timeout".into());
        a.push(timeout_ms.to_string());
        let env: AgentEnvelope = self.run_json_agent("agent prompt", target, &a)?;
        Ok(env.agent)
    }

    /// Every agent in the session, across all workspaces.
    ///
    /// Agent names are unique per *session*, not per workspace, so a launch has
    /// to know what is already taken — otherwise starting a second team reuses
    /// `pm`, `coder-1`… and herdr rejects it with `agent_name_taken`.
    pub fn agent_list(&self) -> Result<Vec<AgentInfo>> {
        let list: AgentList = self.run_json("agent list", &args(["agent", "list"]))?;
        Ok(list.agents)
    }

    pub fn agent_get(&self, target: &str) -> Result<AgentInfo> {
        let env: AgentEnvelope =
            self.run_json_agent("agent get", target, &args(["agent", "get", target]))?;
        Ok(env.agent)
    }

    pub fn agent_read(&self, target: &str, source: ReadSource, lines: u32) -> Result<String> {
        let a = args([
            "agent",
            "read",
            target,
            "--source",
            source.as_str(),
            "--lines",
            &lines.to_string(),
        ]);
        self.run_text("agent read", &a)
    }

    pub fn agent_send_keys(&self, target: &str, keys: &[&str]) -> Result<()> {
        let mut a = args(["agent", "send-keys", target]);
        a.extend(keys.iter().map(|k| k.to_string()));
        self.run_unit("agent send-keys", &a)
    }

    // ---- waiting -------------------------------------------------------

    /// Block until a pane reports `status`, or the timeout elapses.
    ///
    /// A timeout is [`WaitOutcome::TimedOut`], not an error: the launcher
    /// responds by withholding a briefing, which is normal operation.
    pub fn wait_agent_status(
        &self,
        pane_id: &str,
        status: AgentStatus,
        timeout_ms: u64,
    ) -> Result<WaitOutcome> {
        let a = args([
            "wait",
            "agent-status",
            pane_id,
            "--status",
            status.as_str(),
            "--timeout",
            &timeout_ms.to_string(),
        ]);
        self.run_wait("wait agent-status", &a)
    }

    pub fn wait_output(
        &self,
        pane_id: &str,
        pattern: &str,
        regex: bool,
        timeout_ms: u64,
    ) -> Result<WaitOutcome> {
        let mut a = args(["wait", "output", pane_id, "--match", pattern]);
        if regex {
            a.push("--regex".into());
        }
        a.push("--timeout".into());
        a.push(timeout_ms.to_string());
        self.run_wait("wait output", &a)
    }

    // ---- server --------------------------------------------------------

    /// True when a server is reachable for this session.
    pub fn server_running(&self) -> bool {
        !matches!(
            self.workspace_list(),
            Err(HerdrError::ServerUnavailable { .. })
        )
    }

    /// Spawn a detached headless server for this session.
    ///
    /// Truly detached: the server outlives us, and — critically — we must not
    /// outlive *it*. Null stdio alone is not enough on Windows, where a child
    /// inheriting the console keeps the parent's pipes open, so a caller that
    /// starts a server then exits appears to hang until the daemon stops.
    pub fn start_server(&self) -> Result<std::process::Child> {
        let mut cmd = Command::new(&self.exe);
        cmd.envs(self.env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        cmd.args(self.prefix_args());
        cmd.arg("server");
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const DETACHED_PROCESS: u32 = 0x0000_0008;
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
            cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
        }

        cmd.spawn().map_err(|source| HerdrError::Spawn {
            exe: self.exe.display().to_string(),
            source,
        })
    }

    pub fn session_stop(&self, name: &str) -> Result<()> {
        self.run_unit(
            "session stop",
            &["session".into(), "stop".into(), name.into()],
        )
    }

    pub fn session_delete(&self, name: &str) -> Result<()> {
        self.run_unit(
            "session delete",
            &["session".into(), "delete".into(), name.into()],
        )
    }

    // ---- plumbing ------------------------------------------------------

    fn prefix_args(&self) -> Vec<String> {
        match &self.session {
            Some(name) => vec!["--session".into(), name.clone()],
            None => Vec::new(),
        }
    }

    fn invoke(&self, args: &[String]) -> Result<Raw> {
        // Hidden: the app makes dozens of these per launch and two per poll.
        let mut cmd = crate::proc::hidden_command(&self.exe);
        cmd.envs(self.env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        cmd.args(self.prefix_args());
        cmd.args(args);

        let out = cmd.output().map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                HerdrError::NotFound
            } else {
                HerdrError::Spawn {
                    exe: self.exe.display().to_string(),
                    source,
                }
            }
        })?;

        Ok(Raw {
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            code: out.status.code(),
        })
    }

    /// Pull the `result` value out of herdr's envelope, converting an `error`
    /// payload into a typed error.
    ///
    /// **Both streams are inspected.** herdr writes success envelopes to stdout
    /// but API error envelopes to **stderr**, and exits non-zero for the latter.
    /// The JSON body is authoritative; the exit code is only a fallback for
    /// commands that print nothing (`wait` timeouts). Reading stdout alone
    /// silently degrades every API error to a generic command failure — which
    /// is exactly how `server_running()` once reported a dead server as alive.
    fn envelope(&self, context: &str, raw: &Raw) -> Result<Option<serde_json::Value>> {
        self.envelope_for(context, None, raw)
    }

    fn envelope_for(
        &self,
        context: &str,
        agent: Option<&str>,
        raw: &Raw,
    ) -> Result<Option<serde_json::Value>> {
        for text in [raw.stdout.trim(), raw.stderr.trim()] {
            if text.is_empty() {
                continue;
            }
            // Not JSON at all: `pane read` returns a raw transcript, and
            // `--version` returns a plain string.
            let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
                continue;
            };
            if let Some(err) = value.get("error") {
                let api: error::ApiError =
                    serde_json::from_value(err.clone()).map_err(|source| HerdrError::Parse {
                        context: context.to_string(),
                        source,
                        raw: text.to_string(),
                    })?;
                return Err(HerdrError::from_api_for(api, agent));
            }
            if let Some(result) = value.get("result") {
                return Ok(Some(result.clone()));
            }
        }
        Ok(None)
    }

    fn run_json<T: DeserializeOwned>(&self, context: &str, args: &[String]) -> Result<T> {
        let raw = self.invoke(args)?;
        match self.envelope(context, &raw)? {
            Some(result) => serde_json::from_value(result).map_err(|source| HerdrError::Parse {
                context: context.to_string(),
                source,
                raw: raw.stdout.clone(),
            }),
            None => Err(self.command_failed(context, args, raw)),
        }
    }

    /// Like [`Self::run_json`], but tags agent errors with the agent's name so
    /// the caller knows which pane needs attention without parsing prose.
    fn run_json_agent<T: DeserializeOwned>(
        &self,
        context: &str,
        agent: &str,
        args: &[String],
    ) -> Result<T> {
        let raw = self.invoke(args)?;
        match self.envelope_for(context, Some(agent), &raw)? {
            Some(result) => serde_json::from_value(result).map_err(|source| HerdrError::Parse {
                context: context.to_string(),
                source,
                raw: raw.stdout.clone(),
            }),
            None => Err(self.command_failed(context, args, raw)),
        }
    }

    fn run_unit(&self, context: &str, args: &[String]) -> Result<()> {
        let raw = self.invoke(args)?;
        // Surfaces a JSON error body if there is one. Some commands print
        // nothing on success, others (e.g. `pane rename`) print a result; both
        // are fine here.
        self.envelope(context, &raw)?;
        if raw.code == Some(0) {
            Ok(())
        } else {
            Err(self.command_failed(context, args, raw))
        }
    }

    fn run_text(&self, context: &str, args: &[String]) -> Result<String> {
        let raw = self.invoke(args)?;
        self.envelope(context, &raw)?;
        if raw.code == Some(0) {
            Ok(raw.stdout)
        } else {
            Err(self.command_failed(context, args, raw))
        }
    }

    fn run_wait(&self, context: &str, args: &[String]) -> Result<WaitOutcome> {
        let raw = self.invoke(args)?;
        // A real API error (no server, bad pane) still comes back as JSON.
        self.envelope(context, &raw)?;
        // Exit 2 is a syntax error, not an expired wait. Without this check a
        // command that does not exist on this herdr looks exactly like a
        // timeout, which is how a call to the non-existent `wait agent-status`
        // survived three phases of testing.
        self.reject_syntax_error(args, &raw)?;
        Ok(if raw.code == Some(0) {
            WaitOutcome::Reached
        } else {
            WaitOutcome::TimedOut
        })
    }

    /// herdr documents exit status 2 as a CLI syntax error. That is always our
    /// bug — a command or flag this herdr does not have — so it is never folded
    /// into a normal failure path.
    fn reject_syntax_error(&self, args: &[String], raw: &Raw) -> Result<()> {
        if raw.code == Some(2) {
            return Err(HerdrError::CliSyntax {
                args: args.join(" "),
                stderr: if raw.stderr.trim().is_empty() {
                    raw.stdout.trim().to_string()
                } else {
                    raw.stderr.trim().to_string()
                },
            });
        }
        Ok(())
    }

    fn command_failed(&self, context: &str, args: &[String], raw: Raw) -> HerdrError {
        let _ = context;
        if raw.code == Some(2) {
            return HerdrError::CliSyntax {
                args: args.join(" "),
                stderr: if raw.stderr.trim().is_empty() {
                    raw.stdout.trim().to_string()
                } else {
                    raw.stderr.trim().to_string()
                },
            };
        }
        HerdrError::CommandFailed {
            args: args.join(" "),
            code: raw.code,
            stderr: if raw.stderr.trim().is_empty() {
                raw.stdout.trim().to_string()
            } else {
                raw.stderr.trim().to_string()
            },
        }
    }
}

fn args<const N: usize>(parts: [&str; N]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

/// Locate an executable by base name.
///
/// Base name, never a filename: Phase 0 found four agent CLIs installed in three
/// different shapes (`.exe`, `.ps1`, native), so hardcoding an extension is
/// wrong (ground truth §7).
pub fn which(name: &str) -> Option<PathBuf> {
    let finder = if cfg!(windows) { "where.exe" } else { "which" };
    let out = crate::proc::hidden_command(finder)
        .arg(OsStr::new(name))
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(PathBuf::from)
}

/// Whether an install-path candidate is something we could actually spawn.
///
/// A regular file is necessary but, on Unix, not sufficient: a stray
/// non-executable `~/.local/bin/herdr` (a half-finished download, a hand-written
/// placeholder) would otherwise be reported as found, and the failure only
/// surfaces later as "Permission denied" from spawn. Windows has no execute
/// bit, so there a regular file is the whole check.
fn is_runnable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        (meta.permissions().mode() & 0o111) != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn default_install_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if cfg!(windows) {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            out.push(
                PathBuf::from(local)
                    .join("Programs")
                    .join("Herdr")
                    .join("bin")
                    .join("herdr.exe"),
            );
        }
    } else if let Ok(home) = std::env::var("HOME") {
        // herdr's install.sh defaults to $HOME/.local/bin (HERDR_INSTALL_DIR).
        out.push(
            PathBuf::from(&home)
                .join(".local")
                .join("bin")
                .join("herdr"),
        );
        out.push(
            PathBuf::from(&home)
                .join(".herdr")
                .join("bin")
                .join("herdr"),
        );
        out.push(PathBuf::from("/usr/local/bin/herdr"));
    }
    out
}

#[cfg(test)]
mod tests {
    /// herdr's install.sh puts the binary in `$HOME/.local/bin` by default, so
    /// that candidate must be tried before the older `~/.herdr/bin` layout and
    /// the system-wide `/usr/local/bin` fallback. Otherwise a stale copy in one
    /// of those wins over the one the user just installed.
    #[cfg(not(windows))]
    #[test]
    fn unix_candidates_try_local_bin_first() {
        use super::default_install_paths;
        use std::path::PathBuf;

        let home = PathBuf::from(std::env::var("HOME").expect("HOME is set on Unix test hosts"));
        let local_bin = home.join(".local").join("bin").join("herdr");
        let dot_herdr = home.join(".herdr").join("bin").join("herdr");
        let usr_local = PathBuf::from("/usr/local/bin/herdr");

        let candidates = default_install_paths();
        let position = |wanted: &PathBuf| candidates.iter().position(|c| c == wanted);

        let local_idx = position(&local_bin)
            .unwrap_or_else(|| panic!("{candidates:?} lacks {}", local_bin.display()));
        let dot_herdr_idx = position(&dot_herdr)
            .unwrap_or_else(|| panic!("{candidates:?} lacks {}", dot_herdr.display()));
        let usr_local_idx = position(&usr_local)
            .unwrap_or_else(|| panic!("{candidates:?} lacks {}", usr_local.display()));

        assert!(
            local_idx < dot_herdr_idx,
            "$HOME/.local/bin/herdr must come before $HOME/.herdr/bin/herdr: {candidates:?}"
        );
        assert!(
            local_idx < usr_local_idx,
            "$HOME/.local/bin/herdr must come before /usr/local/bin/herdr: {candidates:?}"
        );
    }

    /// A regular file is not enough. A stray non-executable `herdr` must be
    /// skipped here rather than reported as found and then failing to spawn.
    #[cfg(unix)]
    #[test]
    fn unix_candidates_must_carry_the_execute_bit() {
        use super::is_runnable;
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("herdup-herdr-exec-{}", std::process::id()));
        let plain = dir.join("plain").join("herdr");
        let exec = dir.join("exec").join("herdr");
        for path in [&plain, &exec] {
            std::fs::create_dir_all(path.parent().unwrap()).expect("create temp dir");
            std::fs::write(path, b"#!/bin/sh\n").expect("write candidate");
        }
        std::fs::set_permissions(&plain, std::fs::Permissions::from_mode(0o644)).unwrap();
        std::fs::set_permissions(&exec, std::fs::Permissions::from_mode(0o755)).unwrap();

        assert!(
            !is_runnable(&plain),
            "non-executable file must be rejected: {}",
            plain.display()
        );
        assert!(
            is_runnable(&exec),
            "executable file must be accepted: {}",
            exec.display()
        );
        assert!(
            !is_runnable(exec.parent().unwrap()),
            "a directory has the execute bit but is not a candidate"
        );
        assert!(!is_runnable(&dir.join("missing").join("herdr")));

        let _ = std::fs::remove_dir_all(&dir);
    }
}

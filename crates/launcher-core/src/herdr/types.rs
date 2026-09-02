//! Types mirroring herdr's JSON responses.
//!
//! Shapes verified against real captures from herdr 0.8.2-preview in
//! `tests/fixtures/herdr/` (see `docs/notes/2026-09-02-herdr-ground-truth.md`).
//! Unknown fields are ignored so a newer herdr does not break deserialisation.

use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Agent state as detected by herdr.
///
/// **`Idle` does not on its own mean "safe to send keystrokes."** herdr's
/// detection quality is per-agent, and Phase 0 observed Gemini CLI reporting
/// `Idle` while a blocking modal was on screen. Gate keystrokes on the registry's
/// `briefing_trust` tier as well — see spec §5.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Idle,
    Working,
    Blocked,
    Done,
    /// Also the landing spot for any status a future herdr adds. Treating an
    /// unrecognised status as `Unknown` keeps us on the cautious path, since
    /// nothing is ever auto-briefed on `Unknown`.
    #[serde(other)]
    Unknown,
}

impl AgentStatus {
    /// The agent has finished starting and is sitting at its prompt.
    ///
    /// A necessary condition for briefing, never a sufficient one.
    pub fn is_settled(self) -> bool {
        matches!(self, AgentStatus::Idle | AgentStatus::Done)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            AgentStatus::Idle => "idle",
            AgentStatus::Working => "working",
            AgentStatus::Blocked => "blocked",
            AgentStatus::Done => "done",
            AgentStatus::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Pane {
    pub pane_id: String,
    pub tab_id: String,
    pub workspace_id: String,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub label: Option<String>,
    /// herdr's detected agent id, e.g. `claude`. Absent until detection fires.
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default = "unknown_status")]
    pub agent_status: AgentStatus,
    #[serde(default)]
    pub terminal_id: Option<String>,
    #[serde(default)]
    pub terminal_title: Option<String>,
}

impl Pane {
    /// `cwd` with any trailing separator removed.
    ///
    /// herdr reports two forms for the same directory depending on pane state,
    /// not on which command you called: an idle shell reports
    /// `D:\work\herdup\`, and once an agent is running the same pane reports
    /// `D:\work\herdup`. Compare through this, never on the raw string.
    pub fn cwd_path(&self) -> PathBuf {
        let trimmed = self.cwd.trim_end_matches(['/', '\\']);
        if trimmed.is_empty() {
            PathBuf::from(&self.cwd)
        } else {
            PathBuf::from(trimmed)
        }
    }

    /// True when this pane's `cwd` names the same directory as `other`, ignoring
    /// any trailing separator on either side.
    pub fn cwd_is(&self, other: &Path) -> bool {
        let other = other.to_string_lossy();
        let other = other.trim_end_matches(['/', '\\']);
        self.cwd_path().as_path() == Path::new(other)
    }
}

fn unknown_status() -> AgentStatus {
    AgentStatus::Unknown
}

#[derive(Debug, Clone, Deserialize)]
pub struct Tab {
    pub tab_id: String,
    pub workspace_id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub number: u32,
    #[serde(default)]
    pub pane_count: u32,
    #[serde(default)]
    pub focused: bool,
    #[serde(default = "unknown_status")]
    pub agent_status: AgentStatus,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Workspace {
    pub workspace_id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub number: u32,
    #[serde(default)]
    pub pane_count: u32,
    #[serde(default)]
    pub tab_count: u32,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub active_tab_id: Option<String>,
    #[serde(default = "unknown_status")]
    pub agent_status: AgentStatus,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceCreated {
    pub workspace: Workspace,
    pub tab: Tab,
    pub root_pane: Pane,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TabCreated {
    pub tab: Tab,
    pub root_pane: Pane,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PaneInfo {
    pub pane: Pane,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PaneList {
    pub panes: Vec<Pane>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct WorkspaceList {
    pub workspaces: Vec<Workspace>,
}

/// Which transcript `pane read` should return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadSource {
    Visible,
    Recent,
    /// Recent text with soft wraps rejoined — the same transcript
    /// `wait output` matches against.
    RecentUnwrapped,
}

impl ReadSource {
    pub fn as_str(self) -> &'static str {
        match self {
            ReadSource::Visible => "visible",
            ReadSource::Recent => "recent",
            ReadSource::RecentUnwrapped => "recent-unwrapped",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SplitDirection {
    Right,
    Down,
}

impl SplitDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            SplitDirection::Right => "right",
            SplitDirection::Down => "down",
        }
    }
}

/// Result of a `wait` command.
///
/// A timeout is an expected outcome here, not an error — the launcher reacts to
/// it by withholding a briefing rather than by failing the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitOutcome {
    Reached,
    TimedOut,
}

impl WaitOutcome {
    pub fn reached(self) -> bool {
        self == WaitOutcome::Reached
    }
}

/// A parsed `herdr --version`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    /// Everything after the patch, e.g. `-preview.2026-08-31-b1ff4582e968`.
    pub suffix: String,
}

impl Version {
    /// Parse the output of `herdr --version`, e.g.
    /// `herdr 0.8.2-preview.2026-08-31-b1ff4582e968`.
    pub fn parse(output: &str) -> Option<Version> {
        let token = output
            .split_whitespace()
            .find(|t| t.chars().next().is_some_and(|c| c.is_ascii_digit()) && t.contains('.'))?;

        let (core, suffix) = match token.find(['-', '+']) {
            Some(i) => (&token[..i], token[i..].to_string()),
            None => (token, String::new()),
        };

        let mut parts = core.split('.');
        Some(Version {
            major: parts.next()?.parse().ok()?,
            minor: parts.next()?.parse().ok()?,
            patch: parts.next().unwrap_or("0").parse().ok()?,
            suffix,
        })
    }

    /// Ordering ignores the suffix: `0.8.2-preview` satisfies a `0.8.2` minimum.
    /// herdr's Windows builds are preview-only, so treating a preview as older
    /// than its release would reject every Windows install.
    pub fn at_least(&self, major: u32, minor: u32, patch: u32) -> bool {
        (self.major, self.minor, self.patch) >= (major, minor, patch)
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}.{}.{}{}",
            self.major, self.minor, self.patch, self.suffix
        )
    }
}

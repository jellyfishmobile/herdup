//! Typed errors for herdr CLI invocations.
//!
//! The variants that matter are the ones the launcher reacts to differently:
//! a missing binary is a preflight failure, a stopped server is recoverable by
//! starting one, and a protocol mismatch needs a human to restart their session.

use thiserror::Error;

/// herdr's `{"error": {"code", "message"}}` payload.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Error)]
pub enum HerdrError {
    #[error("the `herdr` binary was not found on PATH")]
    NotFound,

    #[error("herdr {found} is older than the required {required}")]
    VersionTooOld { found: String, required: String },

    #[error("could not parse a version from `herdr --version` output: {0:?}")]
    UnparsableVersion(String),

    /// No server is listening. Recoverable: start one and retry.
    #[error("no herdr server is running: {message}")]
    ServerUnavailable { message: String },

    /// Client and server binaries disagree. **Not** recoverable by us —
    /// resolving it means stopping the user's server, which kills their panes.
    #[error("herdr client/server protocol mismatch: {message}")]
    ProtocolMismatch { message: String },

    /// `agent start` found the agent blocked on a startup prompt — typically a
    /// login or a first-run "trust this folder" dialog. Not a failure: the name
    /// stays usable for `agent read` and `agent send-keys`.
    #[error("agent '{name}' is blocked during startup: {message}")]
    AgentNotReady { name: String, message: String },

    /// `agent start` found the pane occupied — usually because the shell in a
    /// just-created pane has not reached its prompt yet.
    ///
    /// A race, not a fault: retry briefly. herdr requires an *available* shell
    /// pane, meaning the shell is in the foreground with no command running.
    #[error("pane {pane} is not an available shell yet: {message}")]
    AgentPaneBusy { pane: String, message: String },

    /// `agent prompt` refused to type into an agent sitting at an approval or
    /// question dialog — **before writing any bytes**.
    ///
    /// This is herdr enforcing the property herdup exists to guarantee. Treat it
    /// as a withheld briefing, never as an error to retry through.
    #[error("agent '{name}' is at a dialog and will not be prompted: {message}")]
    AgentBlocked { name: String, message: String },

    /// A prompt produced no observed lifecycle change within herdr's window.
    #[error("agent '{name}' did not react to the prompt: {message}")]
    AgentPromptStalled { name: String, message: String },

    #[error("herdr returned an error ({code}): {message}")]
    Api { code: String, message: String },

    #[error("`herdr {args}` failed with exit code {code:?}: {stderr}")]
    CommandFailed {
        args: String,
        code: Option<i32>,
        stderr: String,
    },

    /// herdr exit status 2: a CLI *syntax* error — an unknown command or bad
    /// arguments. This is always a herdup bug, never a runtime condition, so it
    /// must surface loudly instead of being folded into an ordinary failure.
    ///
    /// It hid a real one: `wait agent-status` does not exist on herdr 0.8.2, and
    /// treating its exit 2 as "the wait timed out" made a broken call look like
    /// a working one for three phases.
    #[error("`herdr {args}` is not valid for this herdr version (exit 2): {stderr}")]
    CliSyntax { args: String, stderr: String },

    #[error("could not parse herdr output for `{context}`: {source}")]
    Parse {
        context: String,
        #[source]
        source: serde_json::Error,
        raw: String,
    },

    #[error("could not spawn `{exe}`: {source}")]
    Spawn {
        exe: String,
        #[source]
        source: std::io::Error,
    },
}

impl HerdrError {
    /// Map a herdr API error payload onto the typed variants.
    ///
    /// `context` is the agent name when the call targeted one, so the agent
    /// errors can name it — herdr's message does too, but the caller should not
    /// have to parse prose to find out which pane needs attention.
    pub(crate) fn from_api_for(err: ApiError, context: Option<&str>) -> Self {
        let name = || context.unwrap_or("unknown").to_string();
        match err.code.as_str() {
            "server_not_running" => HerdrError::ServerUnavailable {
                message: err.message,
            },
            "protocol_mismatch" => HerdrError::ProtocolMismatch {
                message: err.message,
            },
            "agent_not_ready" => HerdrError::AgentNotReady {
                name: name(),
                message: err.message,
            },
            "agent_blocked" => HerdrError::AgentBlocked {
                name: name(),
                message: err.message,
            },
            "agent_prompt_stalled" => HerdrError::AgentPromptStalled {
                name: name(),
                message: err.message,
            },
            "agent_pane_busy" => HerdrError::AgentPaneBusy {
                pane: name(),
                message: err.message,
            },
            _ => HerdrError::Api {
                code: err.code,
                message: err.message,
            },
        }
    }

    /// True when the agent exists but is waiting on a human.
    ///
    /// Both cases mean the same thing to the launcher: leave it alone, show the
    /// pane, and hold the briefing.
    pub fn is_agent_waiting_on_human(&self) -> bool {
        matches!(
            self,
            HerdrError::AgentNotReady { .. } | HerdrError::AgentBlocked { .. }
        )
    }

    /// Whether starting a server and retrying is worth attempting.
    pub fn is_recoverable_by_starting_server(&self) -> bool {
        matches!(self, HerdrError::ServerUnavailable { .. })
    }

    /// A transient race worth retrying after a short pause.
    pub fn is_transient(&self) -> bool {
        matches!(self, HerdrError::AgentPaneBusy { .. })
    }
}

pub type Result<T> = std::result::Result<T, HerdrError>;

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
    pub(crate) fn from_api(err: ApiError) -> Self {
        match err.code.as_str() {
            "server_not_running" => HerdrError::ServerUnavailable {
                message: err.message,
            },
            "protocol_mismatch" => HerdrError::ProtocolMismatch {
                message: err.message,
            },
            _ => HerdrError::Api {
                code: err.code,
                message: err.message,
            },
        }
    }

    /// Whether starting a server and retrying is worth attempting.
    pub fn is_recoverable_by_starting_server(&self) -> bool {
        matches!(self, HerdrError::ServerUnavailable { .. })
    }
}

pub type Result<T> = std::result::Result<T, HerdrError>;

//! Creating a GitHub repository, via the `gh` CLI.
//!
//! herdup registers no OAuth app and stores no token. `gh` already owns a
//! keychain-backed credential, and reusing it means there is nothing here for us
//! to leak (spec §3, §4).
//!
//! Command construction is separated from execution so the argv can be tested
//! exhaustively without creating anything on anyone's account.

use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GhError {
    #[error("the GitHub CLI (`gh`) is not installed")]
    NotInstalled,

    #[error("`gh` is installed but not signed in — run `gh auth login`")]
    NotAuthenticated,

    #[error("'{name}' is not a valid repository name: {reason}")]
    InvalidName { name: String, reason: String },

    #[error("{path} already exists; choose another name or another folder")]
    DestinationExists { path: String },

    #[error("the folder to clone into does not exist: {path}")]
    DestinationParentMissing { path: String },

    #[error("`gh {args}` failed: {stderr}")]
    CommandFailed { args: String, stderr: String },

    #[error("could not run `gh`: {0}")]
    Spawn(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, GhError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Private,
    Public,
}

impl Visibility {
    fn flag(self) -> &'static str {
        match self {
            Visibility::Private => "--private",
            Visibility::Public => "--public",
        }
    }
}

/// What to create, and where to put it.
#[derive(Debug, Clone)]
pub struct NewRepo {
    /// GitHub account or organisation. `None` uses `gh`'s default account.
    pub owner: Option<String>,
    pub name: String,
    /// Private unless deliberately changed — a repo made by accident should not
    /// be world-readable.
    pub visibility: Visibility,
    pub description: Option<String>,
    /// Directory the clone is created **inside**; the repo lands in a
    /// subdirectory named after itself.
    pub parent_dir: PathBuf,
}

impl NewRepo {
    pub fn private(name: impl Into<String>, parent_dir: impl Into<PathBuf>) -> Self {
        NewRepo {
            owner: None,
            name: name.into(),
            visibility: Visibility::Private,
            description: None,
            parent_dir: parent_dir.into(),
        }
    }

    /// Where the repository will end up.
    pub fn destination(&self) -> PathBuf {
        self.parent_dir.join(&self.name)
    }

    /// The `gh` arguments this request produces.
    ///
    /// Pure, so every combination is covered by tests without touching a real
    /// account. Every element is its own argv entry — no shell, so a name or
    /// path can never be reinterpreted as syntax.
    pub fn args(&self) -> Vec<String> {
        let target = match &self.owner {
            Some(owner) => format!("{owner}/{}", self.name),
            None => self.name.clone(),
        };
        let mut args = vec![
            "repo".to_string(),
            "create".to_string(),
            target,
            self.visibility.flag().to_string(),
            // Clone so the launch can go straight into a working tree.
            "--clone".to_string(),
        ];
        if let Some(description) = self
            .description
            .as_deref()
            .map(str::trim)
            .filter(|d| !d.is_empty())
        {
            args.push("--description".to_string());
            args.push(description.to_string());
        }
        args
    }

    /// The command as a human could read or paste it.
    ///
    /// Quotes any argument containing whitespace. The argv herdup actually
    /// passes needs no quoting — nothing is shell-interpreted — but an echoed
    /// command that joins on spaces is misleading: a multi-word description
    /// printed bare looks like it was truncated to its first word.
    pub fn display_command(&self) -> String {
        let quoted: Vec<String> = self
            .args()
            .into_iter()
            .map(|a| {
                if a.is_empty() || a.chars().any(char::is_whitespace) {
                    format!("\"{}\"", a.replace('"', "\\\""))
                } else {
                    a
                }
            })
            .collect();
        format!("gh {}", quoted.join(" "))
    }

    /// Check everything that can be checked before creating anything.
    ///
    /// A remote repository cannot be un-created quietly, so a bad name or an
    /// occupied destination must fail here rather than halfway through.
    pub fn validate(&self) -> Result<()> {
        validate_name(&self.name)?;

        if !self.parent_dir.is_dir() {
            return Err(GhError::DestinationParentMissing {
                path: self.parent_dir.display().to_string(),
            });
        }
        let destination = self.destination();
        if destination.exists() {
            return Err(GhError::DestinationExists {
                path: destination.display().to_string(),
            });
        }
        Ok(())
    }
}

/// GitHub's own rule set, applied before we call `gh`.
///
/// `gh` would reject most of these too, but its message arrives after a network
/// round trip and reads like an API error.
pub fn validate_name(name: &str) -> Result<()> {
    let invalid = |reason: &str| {
        Err(GhError::InvalidName {
            name: name.to_string(),
            reason: reason.to_string(),
        })
    };

    if name.is_empty() {
        return invalid("it is empty");
    }
    if name.len() > 100 {
        return invalid("it is longer than 100 characters");
    }
    if name == "." || name == ".." {
        return invalid("`.` and `..` are reserved");
    }
    if let Some(bad) = name
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')))
    {
        return invalid(&format!(
            "'{bad}' is not allowed — use letters, digits, hyphen, underscore or dot"
        ));
    }
    if name.starts_with('.') || name.starts_with('-') {
        return invalid("it must start with a letter, digit or underscore");
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct CreatedRepo {
    pub url: Option<String>,
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct Gh {
    exe: PathBuf,
}

impl Gh {
    pub fn new(exe: impl Into<PathBuf>) -> Self {
        Gh { exe: exe.into() }
    }

    pub fn discover() -> Result<Gh> {
        crate::herdr::which("gh")
            .map(Gh::new)
            .ok_or(GhError::NotInstalled)
    }

    pub fn exe(&self) -> &Path {
        &self.exe
    }

    /// Accounts the user could own a repo under: themselves, then their orgs.
    ///
    /// Best-effort — a failure here only means the UI cannot offer a picker, not
    /// that creation is impossible.
    pub fn owners(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(login) = self
            .run(&["api", "user", "--jq", ".login"])
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
        {
            out.push(login);
        }
        if let Ok(orgs) = self.run(&["api", "user/orgs", "--jq", ".[].login"]) {
            for org in orgs.lines().map(str::trim).filter(|l| !l.is_empty()) {
                if !out.iter().any(|o| o == org) {
                    out.push(org.to_string());
                }
            }
        }
        out
    }

    pub fn is_authenticated(&self) -> bool {
        self.run(&["auth", "status"]).is_ok()
    }

    /// Create the repository and clone it.
    ///
    /// **This is the one outward-facing action herdup takes.** It creates
    /// something on the user's GitHub account, so everything checkable is
    /// checked first and the destination is never overwritten.
    pub fn create(&self, repo: &NewRepo) -> Result<CreatedRepo> {
        repo.validate()?;
        if !self.is_authenticated() {
            return Err(GhError::NotAuthenticated);
        }

        let args = repo.args();
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        // `--clone` clones into the working directory, so run from the parent.
        let output = self.run_in(&refs, Some(&repo.parent_dir))?;

        Ok(CreatedRepo {
            url: extract_url(&output),
            path: repo.destination(),
        })
    }

    fn run(&self, args: &[&str]) -> Result<String> {
        self.run_in(args, None)
    }

    fn run_in(&self, args: &[&str], cwd: Option<&Path>) -> Result<String> {
        let mut cmd = crate::proc::hidden_command(&self.exe);
        cmd.args(args);
        if let Some(cwd) = cwd {
            cmd.current_dir(cwd);
        }
        let out = cmd.output()?;
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        if out.status.success() {
            return Ok(stdout);
        }
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        Err(GhError::CommandFailed {
            args: args.join(" "),
            stderr: first_line(&stderr, &stdout),
        })
    }
}

/// `gh repo create` prints the new repository's URL.
fn extract_url(output: &str) -> Option<String> {
    output
        .split_whitespace()
        .find(|t| t.starts_with("https://"))
        .map(|t| t.trim_end_matches(['.', ',']).to_string())
}

fn first_line(stderr: &str, stdout: &str) -> String {
    let text = if stderr.trim().is_empty() {
        stdout
    } else {
        stderr
    };
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("no output")
        .to_string()
}

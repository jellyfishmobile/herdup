//! User settings, including the first-run verification cache.

use crate::config::{ConfigError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// One "this CLI has completed first-run in this project" record.
///
/// Keyed by **CLI and project**, because the two things Stage 1 clears are
/// scoped differently in appearance but identically in practice: a login is
/// per-CLI, but a trust prompt is per-CLI-per-folder, and the folder is the
/// tighter constraint. Using the tighter key means we never wrongly skip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedRecord {
    pub cli: String,
    pub project: String,
    /// Unix seconds. A plain integer avoids a date-library dependency for what
    /// is only ever displayed and compared.
    pub at_unix: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// Where the project picker starts looking.
    #[serde(default)]
    pub projects_root: Option<String>,
    /// Command used to hand off to a terminal. Platform default when unset.
    #[serde(default)]
    pub terminal: Option<String>,
    /// Where to look for a newer herdup. Unset means the public GitHub release
    /// feed; QA points this at a local server. Never a credential.
    #[serde(default)]
    pub update_endpoint: Option<String>,
    #[serde(default)]
    pub verified: Vec<VerifiedRecord>,
}

impl Settings {
    pub fn from_toml(text: &str, file: &str) -> Result<Settings> {
        toml::from_str(text).map_err(|source| ConfigError::Toml {
            file: file.to_string(),
            source,
        })
    }

    /// Load from `<dir>/settings.toml`, treating absent or unreadable as empty.
    ///
    /// A corrupt settings file must not stop someone launching; the worst case
    /// is that Stage 1 runs again, which is safe.
    pub fn load_from(dir: Option<&Path>) -> Settings {
        let Some(dir) = dir else {
            return Settings::default();
        };
        let path = dir.join("settings.toml");
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| Settings::from_toml(&text, &path.display().to_string()).ok())
            .unwrap_or_default()
    }

    pub fn load() -> Settings {
        Settings::load_from(crate::config::config_dir().as_deref())
    }

    /// Persist to the platform config directory.
    ///
    /// A missing config directory is not an error: herdup still works, it just
    /// re-runs first-run next time.
    pub fn save(&self) -> std::io::Result<()> {
        match crate::config::config_dir() {
            Some(dir) => self.save_to(&dir),
            None => Ok(()),
        }
    }

    /// Persist to `<dir>/settings.toml`, creating the directory if needed.
    pub fn save_to(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let text = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(dir.join("settings.toml"), text)
    }

    /// Has this CLI already completed first-run in this project?
    ///
    /// **A hint, not a guarantee** — tokens expire and folder trust can be
    /// revoked. Stage 2's per-pane readiness check remains the real safety net,
    /// so a stale `true` here costs a withheld briefing, never a wrong action.
    pub fn is_verified(&self, cli: &str, project: &Path) -> bool {
        let key = normalise(project);
        self.verified
            .iter()
            .any(|r| r.cli == cli && r.project == key)
    }

    /// Record a successful first-run, replacing any earlier record.
    pub fn mark_verified(&mut self, cli: &str, project: &Path) {
        let key = normalise(project);
        self.verified
            .retain(|r| !(r.cli == cli && r.project == key));
        self.verified.push(VerifiedRecord {
            cli: cli.to_string(),
            project: key,
            at_unix: now_unix(),
        });
    }

    pub fn forget(&mut self, cli: &str, project: &Path) {
        let key = normalise(project);
        self.verified
            .retain(|r| !(r.cli == cli && r.project == key));
    }

    /// Of `clis`, those that still need a first-run pass in this project.
    pub fn unverified<'a>(&self, clis: &[&'a str], project: &Path) -> Vec<&'a str> {
        clis.iter()
            .copied()
            .filter(|cli| !self.is_verified(cli, project))
            .collect()
    }

    pub fn projects_root_path(&self) -> Option<PathBuf> {
        self.projects_root.as_ref().map(PathBuf::from)
    }
}

/// Normalise a project path for use as a cache key.
///
/// herdr reports the same directory with and without a trailing separator
/// depending on pane state (ground truth §3.5), and the user may type either.
fn normalise(project: &Path) -> String {
    let text = project.to_string_lossy();
    let trimmed = text.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        text.to_string()
    } else {
        trimmed.to_string()
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

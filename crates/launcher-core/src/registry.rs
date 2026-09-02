//! The known-CLI registry: what to launch, how to install it, and whether
//! herdup is allowed to type into it unattended.

use crate::config::{ConfigError, Result};
use serde::Deserialize;
use std::collections::BTreeMap;

const BUILTIN: &str = include_str!("../assets/registry.toml");

/// Whether herdup may send a role briefing to this CLI without a human looking
/// at the pane first.
///
/// Phase 0 disproved the assumption that `agent_status == idle` is sufficient:
/// Gemini CLI reported `idle` while a blocking trust modal was on screen, and a
/// test briefing was swallowed by that modal, its trailing Enter granting folder
/// trust. herdr's detection quality is per-agent, so this permission is too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BriefingTrust {
    /// We reproduced the `blocked -> idle` transition for this CLI ourselves.
    Verified,
    /// The default, and the only safe assumption for an untested CLI.
    #[default]
    Manual,
}

impl BriefingTrust {
    /// True only for CLIs whose blocked-detection we have actually tested.
    pub fn may_auto_brief(self) -> bool {
        self == BriefingTrust::Verified
    }
}

/// Install commands differ by OS, and for several CLIs the recommended method
/// is a native installer rather than a package manager.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallHint {
    #[serde(default)]
    pub windows: Option<String>,
    #[serde(default)]
    pub unix: Option<String>,
}

impl InstallHint {
    pub fn for_current_os(&self) -> Option<&str> {
        if cfg!(windows) {
            self.windows.as_deref()
        } else {
            self.unix.as_deref()
        }
    }

    fn is_empty(&self) -> bool {
        self.windows.is_none() && self.unix.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliEntry {
    /// Matches herdr's agent-manifest id, so herdr attributes the pane correctly.
    pub id: String,
    pub display_name: String,
    /// **Base name only**, resolved via `where`/`which` at preflight. Phase 0
    /// found four CLIs installed in three different shapes on one machine, so a
    /// hardcoded filename (`claude.cmd`) is wrong.
    pub binary: String,
    /// herdr's agent kind for `agent start --kind`, when it has one.
    ///
    /// `None` means herdr does not recognise this CLI as an agent, so herdup
    /// falls back to raw pane commands and can never auto-brief it — herdr's
    /// `agent_blocked` guard is unavailable, and that guard is half the reason
    /// auto-briefing is safe at all.
    pub kind: Option<String>,
    pub install_hint: InstallHint,
    pub docs_url: Option<String>,
    /// Empty string means "no flags". Only populated where verified.
    pub flag_presets: Vec<String>,
    pub briefing_trust: BriefingTrust,
}

impl CliEntry {
    /// Whether herdup can drive this CLI through herdr's validated agent API.
    pub fn has_agent_kind(&self) -> bool {
        self.kind.is_some()
    }
}

/// A complete entry, as the built-in file must supply it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FullEntry {
    display_name: String,
    binary: String,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    install_hint: InstallHint,
    #[serde(default)]
    docs_url: Option<String>,
    #[serde(default)]
    flag_presets: Vec<String>,
    #[serde(default)]
    briefing_trust: BriefingTrust,
}

/// A user's partial entry: every field optional, so people record only their
/// deltas and an upgrade to the built-ins does not clobber their edits.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PartialEntry {
    display_name: Option<String>,
    binary: Option<String>,
    kind: Option<String>,
    install_hint: Option<InstallHint>,
    docs_url: Option<String>,
    flag_presets: Option<Vec<String>>,
    briefing_trust: Option<BriefingTrust>,
}

#[derive(Debug, Clone, Default)]
pub struct Registry {
    entries: BTreeMap<String, CliEntry>,
}

impl Registry {
    /// The registry shipped with herdup.
    pub fn builtin() -> Registry {
        Self::from_toml(BUILTIN, "built-in registry.toml")
            .expect("the built-in registry must always parse; this is a build-time bug")
    }

    pub fn from_toml(text: &str, file: &str) -> Result<Registry> {
        let raw: BTreeMap<String, FullEntry> =
            toml::from_str(text).map_err(|source| ConfigError::Toml {
                file: file.to_string(),
                source,
            })?;
        Ok(Registry {
            entries: raw
                .into_iter()
                .map(|(id, e)| {
                    (
                        id.clone(),
                        CliEntry {
                            id,
                            display_name: e.display_name,
                            binary: e.binary,
                            kind: e.kind,
                            install_hint: e.install_hint,
                            docs_url: e.docs_url,
                            flag_presets: e.flag_presets,
                            briefing_trust: e.briefing_trust,
                        },
                    )
                })
                .collect(),
        })
    }

    /// Merge a user's registry file over this one, field by field.
    ///
    /// Overriding a built-in may set any subset of fields. Defining a *new* CLI
    /// must supply at least `display_name` and `binary`, since there is nothing
    /// to inherit them from.
    pub fn with_user_overrides(mut self, text: &str, file: &str) -> Result<Registry> {
        let raw: BTreeMap<String, PartialEntry> =
            toml::from_str(text).map_err(|source| ConfigError::Toml {
                file: file.to_string(),
                source,
            })?;

        for (id, patch) in raw {
            match self.entries.get_mut(&id) {
                Some(existing) => {
                    if let Some(v) = patch.display_name {
                        existing.display_name = v;
                    }
                    if let Some(v) = patch.binary {
                        existing.binary = v;
                    }
                    if patch.kind.is_some() {
                        existing.kind = patch.kind.clone();
                    }
                    if let Some(v) = patch.install_hint {
                        existing.install_hint = v;
                    }
                    if patch.docs_url.is_some() {
                        existing.docs_url = patch.docs_url;
                    }
                    if let Some(v) = patch.flag_presets {
                        existing.flag_presets = v;
                    }
                    if let Some(v) = patch.briefing_trust {
                        existing.briefing_trust = v;
                    }
                }
                None => {
                    let (Some(display_name), Some(binary)) = (patch.display_name, patch.binary)
                    else {
                        return Err(ConfigError::NewEntryIncomplete {
                            id,
                            missing: "both `display_name` and `binary`",
                        });
                    };
                    self.entries.insert(
                        id.clone(),
                        CliEntry {
                            id,
                            display_name,
                            binary,
                            kind: patch.kind,
                            install_hint: patch.install_hint.unwrap_or_default(),
                            docs_url: patch.docs_url,
                            flag_presets: patch.flag_presets.unwrap_or_else(|| vec![String::new()]),
                            // A CLI nobody has tested is never auto-briefed.
                            briefing_trust: patch.briefing_trust.unwrap_or_default(),
                        },
                    );
                }
            }
        }
        Ok(self)
    }

    pub fn get(&self, id: &str) -> Option<&CliEntry> {
        self.entries.get(id)
    }

    pub fn contains(&self, id: &str) -> bool {
        self.entries.contains_key(id)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Entries in stable alphabetical order, for predictable UI listings.
    pub fn iter(&self) -> impl Iterator<Item = &CliEntry> {
        self.entries.values()
    }

    /// CLIs herdup may auto-brief. Expected to be a short list.
    pub fn auto_briefable(&self) -> impl Iterator<Item = &CliEntry> {
        self.iter().filter(|e| e.briefing_trust.may_auto_brief())
    }
}

impl CliEntry {
    /// Install guidance for this machine, if the registry has any.
    pub fn install_command(&self) -> Option<&str> {
        self.install_hint.for_current_os()
    }

    pub fn has_install_guidance(&self) -> bool {
        !self.install_hint.is_empty() || self.docs_url.is_some()
    }
}

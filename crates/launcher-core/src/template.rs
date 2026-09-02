//! Team templates: which roles exist, what each runs, and what each is told.

use crate::config::{ConfigError, Result};
use crate::herdr::types::SplitDirection;
use crate::registry::Registry;
use serde::Deserialize;
use std::collections::BTreeMap;

const BUILTIN: &str = include_str!("../assets/templates.toml");

/// Where a pane comes from, relative to panes created before it.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Split {
    pub direction: SplitDirection,
    #[serde(default)]
    pub ratio: Option<f32>,
    /// Index of an **earlier** pane in the same template.
    pub from: usize,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaneSpec {
    pub role: String,
    /// Must name an entry in the [`Registry`].
    pub cli: String,
    #[serde(default)]
    pub flags: String,
    pub briefing: String,
    /// Exactly one pane per template may set this, and it must be pane 0.
    #[serde(default)]
    pub coordinator: bool,
    /// Absent only on pane 0, which is the workspace's root pane.
    #[serde(default)]
    pub split: Option<Split>,
}

impl PaneSpec {
    /// The briefing as a single line.
    ///
    /// Most agent CLIs submit on newline, so sending a raw multi-line briefing
    /// would fire it as several truncated prompts. Templates keep readable
    /// multi-line text; this is what actually gets typed.
    pub fn flattened_briefing(&self) -> String {
        flatten(&self.briefing)
    }

    /// The command line for this pane, e.g. `claude --permission-mode acceptEdits`.
    pub fn command(&self, binary: &str) -> String {
        command_line(binary, &self.flags)
    }
}

/// Join a binary and flag string, omitting whitespace-only flags.
pub fn command_line(binary: &str, flags: &str) -> String {
    if flags.trim().is_empty() {
        binary.to_string()
    } else {
        format!("{} {}", binary, flags.trim())
    }
}

/// Collapse all whitespace runs — including newlines — into single spaces.
pub fn flatten(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Debug, Clone, PartialEq)]
pub struct Template {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub panes: Vec<PaneSpec>,
}

impl Template {
    /// Index of the coordinator pane, if this template has one.
    pub fn coordinator(&self) -> Option<usize> {
        self.panes.iter().position(|p| p.coordinator)
    }

    /// Distinct CLI ids this template needs, in first-use order.
    ///
    /// Preflight and the sign-in stage work per-CLI, not per-pane: three
    /// `claude` panes need one login and one trust answer.
    pub fn distinct_clis(&self) -> Vec<&str> {
        let mut seen = Vec::new();
        for pane in &self.panes {
            if !seen.contains(&pane.cli.as_str()) {
                seen.push(pane.cli.as_str());
            }
        }
        seen
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTemplate {
    display_name: String,
    description: String,
    pane: Vec<PaneSpec>,
}

#[derive(Debug, Clone, Default)]
pub struct Templates {
    templates: BTreeMap<String, Template>,
}

impl Templates {
    /// The templates shipped with herdup, validated against the built-in registry.
    pub fn builtin() -> Templates {
        Self::from_toml(BUILTIN, "built-in templates.toml")
            .expect("the built-in templates must always parse; this is a build-time bug")
    }

    /// Parse and structurally validate. CLI names are checked separately by
    /// [`Templates::validate_against`], since that needs a registry.
    pub fn from_toml(text: &str, file: &str) -> Result<Templates> {
        let raw: BTreeMap<String, RawTemplate> =
            toml::from_str(text).map_err(|source| ConfigError::Toml {
                file: file.to_string(),
                source,
            })?;

        let mut templates = BTreeMap::new();
        for (id, t) in raw {
            let template = Template {
                id: id.clone(),
                display_name: t.display_name,
                description: t.description,
                panes: t.pane,
            };
            validate_structure(&template)?;
            templates.insert(id, template);
        }
        Ok(Templates { templates })
    }

    /// Merge a user's templates over these, replacing whole templates by id.
    ///
    /// Unlike registry entries, a template is replaced rather than field-merged:
    /// a partially-merged pane list would produce layouts nobody wrote.
    pub fn with_user_overrides(mut self, text: &str, file: &str) -> Result<Templates> {
        let incoming = Templates::from_toml(text, file)?;
        for (id, template) in incoming.templates {
            self.templates.insert(id, template);
        }
        Ok(self)
    }

    /// Check every pane's `cli` resolves in `registry`.
    pub fn validate_against(&self, registry: &Registry) -> Result<()> {
        for template in self.templates.values() {
            for pane in &template.panes {
                if !registry.contains(&pane.cli) {
                    return Err(ConfigError::UnknownCli {
                        template: template.id.clone(),
                        role: pane.role.clone(),
                        cli: pane.cli.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&Template> {
        self.templates.get(id)
    }

    pub fn len(&self) -> usize {
        self.templates.len()
    }

    pub fn is_empty(&self) -> bool {
        self.templates.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Template> {
        self.templates.values()
    }
}

/// Enforce the layout invariants a plan generator depends on.
fn validate_structure(t: &Template) -> Result<()> {
    if t.panes.is_empty() {
        return Err(ConfigError::EmptyTemplate {
            template: t.id.clone(),
        });
    }

    let mut coordinator: Option<&str> = None;
    for (index, pane) in t.panes.iter().enumerate() {
        match (index, &pane.split) {
            // Pane 0 is the workspace root pane: `workspace create` makes it.
            (0, Some(_)) => {
                return Err(ConfigError::RootPaneHasSplit {
                    template: t.id.clone(),
                    role: pane.role.clone(),
                })
            }
            (0, None) => {}
            (_, None) => {
                return Err(ConfigError::MissingSplit {
                    template: t.id.clone(),
                    role: pane.role.clone(),
                    index,
                })
            }
            (_, Some(split)) => {
                // Splitting from a later pane would reference something that
                // does not exist yet at execution time.
                if split.from >= index {
                    return Err(ConfigError::SplitFromNotEarlier {
                        template: t.id.clone(),
                        role: pane.role.clone(),
                        index,
                        from: split.from,
                    });
                }
            }
        }

        if pane.coordinator {
            if let Some(first) = coordinator {
                return Err(ConfigError::MultipleCoordinators {
                    template: t.id.clone(),
                    first: first.to_string(),
                    second: pane.role.clone(),
                });
            }
            if index != 0 {
                return Err(ConfigError::CoordinatorNotFirst {
                    template: t.id.clone(),
                    role: pane.role.clone(),
                    index,
                });
            }
            coordinator = Some(&pane.role);
        }
    }
    Ok(())
}

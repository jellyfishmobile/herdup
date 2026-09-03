//! Errors for loading and validating the registry and templates.
//!
//! Every variant names the offending file, template and field, because these
//! are user-editable TOML files and a vague "invalid config" is useless to
//! someone trying to fix their own edit.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("{file}: {source}")]
    Toml {
        file: String,
        #[source]
        source: toml::de::Error,
    },

    #[error("{file}: {source}")]
    Io {
        file: String,
        #[source]
        source: std::io::Error,
    },

    #[error(
        "registry entry '{id}' is new (not a built-in) so it must set {missing}; \
         overrides of built-in entries may omit fields, new entries may not"
    )]
    NewEntryIncomplete { id: String, missing: &'static str },

    #[error("template '{template}' has no panes")]
    EmptyTemplate { template: String },

    #[error(
        "template '{template}': pane 0 ('{role}') is the root pane and must not set `split` \
         — it is created by `workspace create`, not by splitting something else"
    )]
    RootPaneHasSplit { template: String, role: String },

    #[error(
        "template '{template}': pane {index} ('{role}') must set `split` (only pane 0 may omit it)"
    )]
    MissingSplit {
        template: String,
        role: String,
        index: usize,
    },

    #[error(
        "template '{template}': pane {index} ('{role}') splits from {from}, which is not an \
         earlier pane — `from` must reference a pane that already exists"
    )]
    SplitFromNotEarlier {
        template: String,
        role: String,
        index: usize,
        from: usize,
    },

    #[error(
        "template '{template}': '{role}' at pane {index} sets coordinator, but the coordinator \
         must be pane 0 — it holds the root pane the others split from"
    )]
    CoordinatorNotFirst {
        template: String,
        role: String,
        index: usize,
    },

    #[error("template '{template}': both '{first}' and '{second}' set coordinator; at most one is allowed")]
    MultipleCoordinators {
        template: String,
        first: String,
        second: String,
    },

    #[error(
        "template '{template}': pane '{role}' names cli '{cli}', which is not in the registry"
    )]
    UnknownCli {
        template: String,
        role: String,
        cli: String,
    },
}

pub type Result<T> = std::result::Result<T, ConfigError>;

// ---------------------------------------------------------------------------
// on-disk configuration
// ---------------------------------------------------------------------------

use crate::registry::Registry;
use crate::template::Templates;
use std::path::PathBuf;

/// Where herdup keeps user configuration.
///
/// Windows: `%APPDATA%\herdup\`, macOS: `~/Library/Application Support/herdup/`.
/// Returns `None` only if the platform's home variable is unset, in which case
/// herdup runs on built-ins alone rather than failing.
pub fn config_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(|p| PathBuf::from(p).join("herdup"))
    } else {
        std::env::var_os("HOME").map(|p| {
            PathBuf::from(p)
                .join("Library")
                .join("Application Support")
                .join("herdup")
        })
    }
}

/// Read a user config file, treating "absent" and "unreadable" as "no overrides".
///
/// A missing file is the normal case; an unreadable one should not stop herdup
/// from launching on built-in defaults.
fn read_optional(path: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

/// Built-in registry, with the user's `registry.toml` merged over it if present.
pub fn load_registry() -> Result<Registry> {
    load_registry_from(config_dir().as_deref())
}

/// Built-in templates, with the user's `templates.toml` merged over them, then
/// validated against `registry`.
pub fn load_templates(registry: &Registry) -> Result<Templates> {
    load_templates_from(config_dir().as_deref(), registry)
}

/// Directory-explicit variants, so tests need not touch real user config.
pub fn load_registry_from(dir: Option<&std::path::Path>) -> Result<Registry> {
    let builtin = Registry::builtin();
    let Some(dir) = dir else { return Ok(builtin) };
    let path = dir.join("registry.toml");
    match read_optional(&path) {
        Some(text) => builtin.with_user_overrides(&text, &path.display().to_string()),
        None => Ok(builtin),
    }
}

pub fn load_templates_from(
    dir: Option<&std::path::Path>,
    registry: &Registry,
) -> Result<Templates> {
    let builtin = Templates::builtin();
    let templates = match dir.map(|d| d.join("templates.toml")) {
        Some(path) => match read_optional(&path) {
            Some(text) => builtin.with_user_overrides(&text, &path.display().to_string())?,
            None => builtin,
        },
        None => builtin,
    };
    templates.validate_against(registry)?;
    Ok(templates)
}

/// Templates for a launch into `project`: built-ins, the user's overrides,
/// and the project's own team when it has a valid one.
///
/// A repo team that fails to load comes back in the second slot instead of
/// failing the call, so a typo in `.herdr/team.toml` can be shown next to a
/// team list that still works.
pub fn load_templates_for(
    project: &std::path::Path,
    registry: &Registry,
) -> Result<(Templates, Option<ConfigError>)> {
    let templates = load_templates(registry)?;
    Ok(match crate::template::load_repo_team(project, registry) {
        None => (templates, None),
        Some(Ok(team)) => (templates.with_repo_team(team), None),
        Some(Err(e)) => (templates, Some(e)),
    })
}

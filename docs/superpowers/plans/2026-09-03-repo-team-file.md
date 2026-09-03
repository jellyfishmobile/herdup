# Per-repo team file Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A repository's `.herdr/team.toml` is offered first and preselected when that project is chosen, in the GUI and via `--template repo` in the CLI, with an invalid file reported inline and never blocking a built-in team.

**Architecture:** One loader in launcher-core reads the bare template file, applies the `repo` id and folder-name default, and runs the existing structural and registry checks. A config helper merges it over the built-in and user templates and hands back the load error separately. The app's DTOs and the CLI resolve `repo` through that helper; the team step orders and tags it.

**Tech Stack:** Rust 2021 (`toml`, `serde`, `thiserror`), Tauri 2, React 18 + TypeScript strict.

**Spec:** `docs/superpowers/specs/2026-09-03-repo-team-file-design.md`

**Working agreement.** Two coders share one checkout on `main`. Commit with explicit paths, `git commit -m "…" -- <files>`, so a sibling's staged files never ride along. Run `cargo fmt --all` before every commit. Every commit message ends with:

```
Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01DF5BCBNLmiW4vhZVyqJXLd
```

---

## File structure

| File | Responsibility |
|---|---|
| `crates/launcher-core/src/config.rs` (modify) | `ConfigError::Io`; `load_templates_for(project, registry)` |
| `crates/launcher-core/src/template.rs` (modify) | constants, `RawRepoTeam`, `parse_repo_team`, `load_repo_team`, `Templates::with_repo_team`, `validate_clis` factored out |
| `crates/launcher-core/tests/repo_team.rs` (create) | loader, merge and planning tests |
| `crates/launcher-cli/src/main.rs` (modify) | `resolve_template` helper used by plan, preflight, launch; usage text |
| `app/src-tauri/src/lib.rs` (modify) | `from_repo` on `TemplateDto`, `team_file` on `ProjectStatusDto`, `list_templates(project)`, `repo` resolution in `build_plan_inner` |
| `app/src/api.ts` (modify) | the two type fields; `listTemplates(project)` |
| `app/src/App.tsx` (modify) | re-list on project change, preselect, ordering, tag, error line |
| `app/src/styles.css` (modify) | one rule for the tag inside a segment button |

Order and ownership: Task 1 and Task 2 (core, coder-1) first. Task 5 (frontend, coder-2) can run alongside them. Task 3 (app Rust, coder-2) and Task 4 (CLI, coder-1) need Tasks 1–2 merged. Task 6 is QA plus the user's eyes.

---

### Task 1: the loader in launcher-core

**Files:**
- Modify: `crates/launcher-core/src/config.rs` (the error enum near the top)
- Modify: `crates/launcher-core/src/template.rs`
- Test: `crates/launcher-core/tests/repo_team.rs` (create)

- [x] **Step 1: Write the failing tests**

Create `crates/launcher-core/tests/repo_team.rs`:

```rust
//! A repository's own team, from `.herdr/team.toml`.

use launcher_core::config::ConfigError;
use launcher_core::registry::Registry;
use launcher_core::template::{
    load_repo_team, parse_repo_team, Templates, REPO_TEAM_FILE, REPO_TEMPLATE_ID,
};
use std::path::{Path, PathBuf};

const VALID: &str = r#"
display_name = "Repo squad"
description  = "two panes"

[[pane]]
role        = "PM"
cli         = "claude"
coordinator = true
briefing    = "Coordinate."

[[pane]]
role     = "Dev"
cli      = "claude"
split    = { direction = "right", from = 0 }
briefing = "Build."
"#;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("herdup-repo-team-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn write_team(project: &Path, text: &str) {
    let file = project.join(REPO_TEAM_FILE);
    std::fs::create_dir_all(file.parent().unwrap()).expect(".herdr");
    std::fs::write(file, text).expect("team.toml");
}

#[test]
fn a_valid_team_loads_under_the_repo_id() {
    let project = scratch("valid");
    write_team(&project, VALID);
    let team = load_repo_team(&project, &Registry::builtin())
        .expect("file exists")
        .expect("valid");
    assert_eq!(team.id, REPO_TEMPLATE_ID);
    assert_eq!(team.display_name, "Repo squad");
    assert_eq!(team.description, "two panes");
    assert_eq!(team.panes.len(), 2);
    assert_eq!(team.coordinator(), Some(0));
    std::fs::remove_dir_all(&project).unwrap();
}

#[test]
fn the_display_name_defaults_to_the_folder_name() {
    let project = scratch("named");
    let text = VALID.replacen("display_name = \"Repo squad\"\n", "", 1);
    write_team(&project, &text);
    let team = load_repo_team(&project, &Registry::builtin()).unwrap().unwrap();
    assert_eq!(team.display_name, project.file_name().unwrap().to_string_lossy());
    std::fs::remove_dir_all(&project).unwrap();
}

#[test]
fn no_file_means_none() {
    let project = scratch("absent");
    assert!(load_repo_team(&project, &Registry::builtin()).is_none());
    std::fs::remove_dir_all(&project).unwrap();
}

#[test]
fn a_wrapping_key_is_rejected_and_named() {
    let text = format!("[squad]\n{}", VALID.replace("[[pane]]", "[[squad.pane]]"));
    let err = parse_repo_team(&text, "team.toml", Path::new("/p/demo"), &Registry::builtin())
        .expect_err("a wrapping key is not the bare shape");
    let msg = err.to_string();
    assert!(msg.contains("squad"), "{msg}");
    assert!(matches!(err, ConfigError::Toml { .. }), "{err:?}");
}

#[test]
fn an_unknown_cli_is_rejected_by_role() {
    let text = VALID.replace("cli      = \"claude\"", "cli      = \"nope\"");
    let err = parse_repo_team(&text, "team.toml", Path::new("/p/demo"), &Registry::builtin())
        .expect_err("unknown cli");
    match err {
        ConfigError::UnknownCli { template, role, cli } => {
            assert_eq!(template, REPO_TEMPLATE_ID);
            assert_eq!(role, "Dev");
            assert_eq!(cli, "nope");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_root_pane_may_not_split() {
    let text = VALID.replacen(
        "coordinator = true\n",
        "coordinator = true\nsplit = { direction = \"right\", from = 0 }\n",
        1,
    );
    let err = parse_repo_team(&text, "team.toml", Path::new("/p/demo"), &Registry::builtin())
        .expect_err("root split");
    assert!(matches!(err, ConfigError::RootPaneHasSplit { .. }), "{err:?}");
}

#[test]
fn the_coordinator_must_be_first() {
    let text = VALID
        .replacen("coordinator = true\n", "", 1)
        .replacen("role     = \"Dev\"\n", "role     = \"Dev\"\ncoordinator = true\n", 1);
    let err = parse_repo_team(&text, "team.toml", Path::new("/p/demo"), &Registry::builtin())
        .expect_err("coordinator at 1");
    assert!(matches!(err, ConfigError::CoordinatorNotFirst { .. }), "{err:?}");
}

#[test]
fn an_unreadable_file_is_an_error_not_none() {
    let project = scratch("unreadable");
    // A directory where the file should be: exists, cannot be read as text.
    std::fs::create_dir_all(project.join(REPO_TEAM_FILE)).unwrap();
    let outcome = load_repo_team(&project, &Registry::builtin()).expect("something is there");
    assert!(matches!(outcome, Err(ConfigError::Io { .. })), "{outcome:?}");
    std::fs::remove_dir_all(&project).unwrap();
}

#[test]
fn with_repo_team_offers_it_under_repo_and_replaces_an_earlier_one() {
    let registry = Registry::builtin();
    let first = parse_repo_team(VALID, "team.toml", Path::new("/p/one"), &registry).unwrap();
    let second = parse_repo_team(
        &VALID.replace("Repo squad", "Second"),
        "team.toml",
        Path::new("/p/two"),
        &registry,
    )
    .unwrap();
    let templates = Templates::builtin().with_repo_team(first).with_repo_team(second);
    assert_eq!(templates.get(REPO_TEMPLATE_ID).unwrap().display_name, "Second");
    assert_eq!(templates.len(), Templates::builtin().len() + 1);
}
```

- [x] **Step 2: Run them to verify they fail**

Run: `cargo test -p launcher-core --test repo_team`
Expected: compile errors: `unresolved imports` for `load_repo_team`, `parse_repo_team`, `REPO_TEAM_FILE`, `REPO_TEMPLATE_ID`, and `no variant named Io`.

- [x] **Step 3: The error variant**

In `crates/launcher-core/src/config.rs`, inside `pub enum ConfigError`, directly after the `Toml` variant:

```rust
    #[error("{file}: {source}")]
    Io {
        file: String,
        #[source]
        source: std::io::Error,
    },
```

- [x] **Step 4: The loader**

In `crates/launcher-core/src/template.rs`, after the `BUILTIN`/`ADDABLE` constants:

```rust
/// The id under which a repository's own team is offered. Reserved: a user
/// template with this id is replaced by the repository's file when one exists.
pub const REPO_TEMPLATE_ID: &str = "repo";
/// Where a repository keeps its team, relative to the project folder.
pub const REPO_TEAM_FILE: &str = ".herdr/team.toml";
```

After `struct RawTemplate`:

```rust
/// The bare shape of `.herdr/team.toml`: the top level *is* the team.
///
/// `display_name` is optional because the folder name is usually right.
/// Unknown keys are rejected so a file written in the `templates.toml` shape,
/// with a wrapping `[squad]` table, fails naming that key rather than loading
/// as an empty team.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRepoTeam {
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    description: String,
    pane: Vec<PaneSpec>,
}

/// A repository's own team, from `<project>/.herdr/team.toml`.
///
/// `None` when there is no file. `Some(Err)` for anything wrong with a file
/// that exists — unreadable, malformed, an invariant broken, an unknown CLI —
/// so a typo is reported rather than silently ignored.
pub fn load_repo_team(project: &Path, registry: &Registry) -> Option<Result<Template>> {
    let path = project.join(REPO_TEAM_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(source) => {
            return Some(Err(ConfigError::Io {
                file: path.display().to_string(),
                source,
            }))
        }
    };
    Some(parse_repo_team(
        &text,
        &path.display().to_string(),
        project,
        registry,
    ))
}

/// Parse the bare team shape and validate it exactly as a built-in is.
///
/// `project` supplies the default display name; `file` is only for messages.
pub fn parse_repo_team(
    text: &str,
    file: &str,
    project: &Path,
    registry: &Registry,
) -> Result<Template> {
    let raw: RawRepoTeam = toml::from_str(text).map_err(|source| ConfigError::Toml {
        file: file.to_string(),
        source,
    })?;
    let folder = project
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| project.display().to_string());
    let template = Template {
        id: REPO_TEMPLATE_ID.to_string(),
        display_name: raw
            .display_name
            .filter(|n| !n.trim().is_empty())
            .unwrap_or(folder),
        description: raw.description,
        panes: raw.pane,
    };
    validate_structure(&template)?;
    validate_clis(&template, registry)?;
    Ok(template)
}
```

Add `use std::path::Path;` to the module's imports.

In `impl Templates`, after `with_user_overrides`:

```rust
    /// Offer a repository's own team under [`REPO_TEMPLATE_ID`].
    ///
    /// Replaces an earlier `repo` entry, including one a user wrote in their
    /// own `templates.toml`: the repository's file wins for that repository.
    /// Ordering is the caller's business; the GUI lists it first.
    pub fn with_repo_team(mut self, team: Template) -> Templates {
        self.templates.insert(REPO_TEMPLATE_ID.to_string(), team);
        self
    }
```

Factor the registry check so both paths share it. Replace the body of `validate_against` and add the helper:

```rust
    /// Check every pane's `cli` resolves in `registry`.
    pub fn validate_against(&self, registry: &Registry) -> Result<()> {
        self.templates
            .values()
            .try_for_each(|t| validate_clis(t, registry))
    }
```

```rust
/// Every pane's `cli` must be a registry entry.
fn validate_clis(template: &Template, registry: &Registry) -> Result<()> {
    for pane in &template.panes {
        if !registry.contains(&pane.cli) {
            return Err(ConfigError::UnknownCli {
                template: template.id.clone(),
                role: pane.role.clone(),
                cli: pane.cli.clone(),
            });
        }
    }
    Ok(())
}
```

- [x] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p launcher-core --test repo_team`
Expected: `test result: ok. 9 passed`.

If `a_wrapping_key_is_rejected_and_named` fails because the TOML error names `pane` (missing field) before `squad` (unknown field): put the wrapping key **after** a valid bare body in that test instead — `format!("{VALID}\n[squad]\nx = 1\n")` — so the unknown table is what the parser trips on, and keep the assertion that the message contains `squad`.

- [x] **Step 6: Lint and commit**

```bash
cargo fmt --all
cargo clippy -p launcher-core --all-targets -- -D warnings
cargo test -p launcher-core
git commit -m "Load a repository's own team from .herdr/team.toml" -- crates/launcher-core/src/config.rs crates/launcher-core/src/template.rs crates/launcher-core/tests/repo_team.rs
```

---

### Task 2: `load_templates_for` and the planning check

**Files:**
- Modify: `crates/launcher-core/src/config.rs` (after `load_templates_from`)
- Test: `crates/launcher-core/tests/repo_team.rs` (append)

- [x] **Step 1: Write the failing tests**

Append to `crates/launcher-core/tests/repo_team.rs`:

```rust
use launcher_core::config::load_templates_for;
use launcher_core::plan::{plan, LaunchRequest};

#[test]
fn load_templates_for_merges_a_valid_team() {
    let project = scratch("merge");
    write_team(&project, VALID);
    let registry = Registry::builtin();
    let (templates, error) = load_templates_for(&project, &registry).expect("loads");
    assert!(error.is_none());
    assert!(templates.get(REPO_TEMPLATE_ID).is_some());
    assert!(templates.get("squad").is_some(), "built-ins are kept");
    std::fs::remove_dir_all(&project).unwrap();
}

#[test]
fn load_templates_for_reports_a_bad_team_and_keeps_the_builtins() {
    let project = scratch("bad");
    write_team(&project, "display_name = 1\n");
    let registry = Registry::builtin();
    let (templates, error) = load_templates_for(&project, &registry).expect("loads");
    assert!(templates.get(REPO_TEMPLATE_ID).is_none());
    assert!(templates.get("squad").is_some());
    let error = error.expect("the bad file is reported");
    assert!(error.to_string().contains("team.toml"), "{error}");
    std::fs::remove_dir_all(&project).unwrap();
}

#[test]
fn load_templates_for_without_a_file_is_just_the_templates() {
    let project = scratch("plain");
    let registry = Registry::builtin();
    let (templates, error) = load_templates_for(&project, &registry).expect("loads");
    assert!(error.is_none());
    assert!(templates.get(REPO_TEMPLATE_ID).is_none());
    std::fs::remove_dir_all(&project).unwrap();
}

#[test]
fn a_repo_team_plans_like_the_same_builtin() {
    // The duo shape, written as a repo team, must plan identically to duo.
    let registry = Registry::builtin();
    let builtin = Templates::builtin();
    let duo = builtin.get("duo").expect("duo exists");
    let mut text = String::from("display_name = \"Duo\"\ndescription = \"d\"\n");
    for pane in &duo.panes {
        text.push_str("\n[[pane]]\n");
        text.push_str(&format!("role = {:?}\ncli = {:?}\nflags = {:?}\nbriefing = {:?}\n",
            pane.role, pane.cli, pane.flags, pane.briefing));
        if pane.coordinator {
            text.push_str("coordinator = true\n");
        }
        if let Some(split) = &pane.split {
            // Serialise the split the way templates.toml writes it.
            let direction = format!("{:?}", split.direction).to_lowercase();
            match split.ratio {
                Some(r) => text.push_str(&format!(
                    "split = {{ direction = \"{direction}\", ratio = {r}, from = {} }}\n",
                    split.from
                )),
                None => text.push_str(&format!(
                    "split = {{ direction = \"{direction}\", from = {} }}\n",
                    split.from
                )),
            }
        }
    }
    let repo = parse_repo_team(&text, "team.toml", Path::new("/work/demo"), &registry).unwrap();
    let project = Path::new("/work/demo");
    let a = plan(&LaunchRequest::new(project, duo), &registry).unwrap();
    let b = plan(&LaunchRequest::new(project, &repo), &registry).unwrap();
    let shape = |p: &launcher_core::plan::LaunchPlan| {
        p.panes.iter().map(|x| (x.role.clone(), x.cli.clone(), x.command.clone())).collect::<Vec<_>>()
    };
    assert_eq!(shape(&a), shape(&b));
}
```

If `SplitDirection` does not print as `Right`/`Down` under `{:?}`, or `PlannedPane` names its fields differently, adjust those two spots to the real names in `crates/launcher-core/src/plan.rs` and `herdr/types.rs`; the intent is a byte-for-byte same pane list.

- [x] **Step 2: Run to verify they fail**

Run: `cargo test -p launcher-core --test repo_team`
Expected: `unresolved import launcher_core::config::load_templates_for`.

- [x] **Step 3: The helper**

In `crates/launcher-core/src/config.rs`, after `load_templates_from`:

```rust
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
```

- [x] **Step 4: Run to verify they pass**

Run: `cargo test -p launcher-core --test repo_team`
Expected: `test result: ok. 13 passed`.

- [x] **Step 5: Lint and commit**

```bash
cargo fmt --all
cargo clippy -p launcher-core --all-targets -- -D warnings
cargo test --workspace
git commit -m "Merge a project's own team over the templates for a launch" -- crates/launcher-core/src/config.rs crates/launcher-core/tests/repo_team.rs
```

---

### Task 3: the app resolves `repo`

**Files:**
- Modify: `app/src-tauri/src/lib.rs` — `TemplateDto` (`:45-51`), `ProjectStatusDto` (`:191-198`), `build_plan_inner` (`:275-283`), `list_templates` (`:326-345`), `project_status` (`:333-356`)

Verification is clippy plus the workspace tests, as for every Tauri-layer task; Task 6 exercises it.

- [x] **Step 1: DTO fields**

Add to `TemplateDto`:

```rust
    /// The team came from the project's own `.herdr/team.toml`.
    from_repo: bool,
```

Add to `ProjectStatusDto`:

```rust
    /// The project has a `.herdr/team.toml` that failed to load: the message.
    /// `None` when there is no file or it is valid.
    team_file: Option<String>,
```

- [x] **Step 2: A DTO builder and the listing**

Replace `list_templates` with:

```rust
fn template_dto(t: &launcher_core::template::Template) -> TemplateDto {
    TemplateDto {
        id: t.id.clone(),
        display_name: t.display_name.clone(),
        description: t.description.clone(),
        from_repo: t.id == launcher_core::template::REPO_TEMPLATE_ID,
        panes: t
            .panes
            .iter()
            .map(|p| TemplatePaneDto {
                role: p.role.clone(),
                cli: p.cli.clone(),
                flags: p.flags.clone(),
                coordinator: p.coordinator,
            })
            .collect(),
    }
}

/// Built-in and user templates, plus the project's own team when `project`
/// is given and has a valid one. A bad team file is not an error here; it is
/// reported by [`project_status`] so the list still renders.
#[tauri::command]
fn list_templates(project: Option<String>) -> Result<Vec<TemplateDto>, String> {
    let registry = launcher_core::config::load_registry().map_err(|e| e.to_string())?;
    let templates = match project.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        Some(project) => {
            launcher_core::config::load_templates_for(std::path::Path::new(project), &registry)
                .map_err(|e| e.to_string())?
                .0
        }
        None => launcher_core::config::load_templates(&registry).map_err(|e| e.to_string())?,
    };
    Ok(templates.iter().map(template_dto).collect())
}
```

- [x] **Step 3: Project status reports a bad file**

In `project_status`, inside the `spawn_blocking` closure, before building the DTO:

```rust
        let team_file = launcher_core::config::load_registry()
            .ok()
            .and_then(|registry| {
                match launcher_core::template::load_repo_team(&path, &registry) {
                    Some(Err(e)) => Some(e.to_string()),
                    _ => None,
                }
            });
```

and add `team_file,` to the `ProjectStatusDto { … }` literal.

- [x] **Step 4: Planning resolves `repo` against the project**

In `build_plan_inner`, replace

```rust
    let (registry, templates, settings) = load()?;
    let template = templates
        .get(&options.template)
        .ok_or_else(|| format!("no template '{}'", options.template))?;

    let project = PathBuf::from(&options.project);
```

with

```rust
    let (registry, _, settings) = load()?;
    let project = PathBuf::from(&options.project);
    let (templates, repo_error) =
        launcher_core::config::load_templates_for(&project, &registry).map_err(|e| e.to_string())?;
    let template = match templates.get(&options.template) {
        Some(t) => t,
        None if options.template == launcher_core::template::REPO_TEMPLATE_ID => {
            return Err(match repo_error {
                Some(e) => e.to_string(),
                None => format!(
                    "no {} in {}",
                    launcher_core::template::REPO_TEAM_FILE,
                    project.display()
                ),
            })
        }
        None => return Err(format!("no template '{}'", options.template)),
    };
```

- [x] **Step 5: Verify and commit**

```bash
cargo fmt --all
cargo clippy -p herdup-app --all-targets -- -D warnings
cargo test --workspace
git commit -m "Offer a project's own team in the app and resolve repo against the project" -- app/src-tauri/src/lib.rs
```

---

### Task 4: the CLI honours `repo`

**Files:**
- Modify: `crates/launcher-cli/src/main.rs` — usage text (`:93-95`), `show_plan` (`:208-225`), `show_preflight` (`:305-322`), `launch` (`:462-483`), `template_ids` (`:293`)

- [x] **Step 1: One resolver for the three commands**

Directly above `fn template_ids`:

```rust
/// Load the templates for `cwd` and pick `id`, or explain why not.
///
/// `repo` is the project's own `.herdr/team.toml`; its absence or its load
/// error is the message. Any other unknown id lists what exists. Returns
/// `Ok(None)` after printing, so callers keep their current early-return
/// shape; the process exit code is set by the caller.
fn resolve_template(
    verb: &str,
    id: &str,
    cwd: &std::path::Path,
    registry: &launcher_core::registry::Registry,
) -> Result<Option<launcher_core::template::Template>, AppError> {
    use launcher_core::template::{REPO_TEAM_FILE, REPO_TEMPLATE_ID};
    let (templates, repo_error) = launcher_core::config::load_templates_for(cwd, registry)?;
    if let Some(t) = templates.get(id) {
        return Ok(Some(t.clone()));
    }
    if id == REPO_TEMPLATE_ID {
        match repo_error {
            Some(e) => eprintln!("{verb}: {e}"),
            None => eprintln!("{verb}: no {REPO_TEAM_FILE} in {}", cwd.display()),
        }
    } else {
        eprintln!(
            "{verb}: no template '{id}'. Known: {}",
            template_ids(&templates)
        );
    }
    std::process::exit(2);
}
```

If `AppError` does not convert from `ConfigError` with `?` here, wrap with the same conversion the surrounding commands use for `load_templates(&registry)?`. The `exit(2)` makes an unknown or unloadable template a non-zero exit, which the spec asks for and the earlier code did not do; keep the `Ok(None)` type so the signature reads honestly even though the early return never happens in practice — or, simpler, return `Result<launcher_core::template::Template, AppError>` and drop the `Option`; pick one and be consistent in the three call sites below.

- [x] **Step 2: Use it in `plan`, `preflight`, `launch`**

In each of the three commands, replace the pair

```rust
    let templates = launcher_core::config::load_templates(&registry)?;
    let Some(template) = templates.get(&id) else {
        eprintln!(
            "<verb>: no template '{id}'. Known: {}",
            template_ids(&templates)
        );
        return Ok(());
    };
```

with

```rust
    let template = resolve_template("<verb>", &id, &cwd, &registry)?;
```

using the verb `plan`, `preflight`, `launch` respectively, and make sure `cwd` is computed before the call (it already is in all three). `LaunchRequest::new(&cwd, template)` takes a reference: pass `&template`.

- [x] **Step 3: Usage text**

Where the usage string lists `--template ID`, add one line under the launch usage:

```text
         `--template repo` uses the project's own .herdr/team.toml
```

- [x] **Step 4: Verify by hand and commit**

```bash
cargo fmt --all
cargo clippy -p launcher-cli --all-targets -- -D warnings
cargo test --workspace
D=$(mktemp -d); cargo run -q -p launcher-cli -- plan --template repo --cwd "$D"; echo "exit $?"
```
Expected: `plan: no .herdr/team.toml in /…` and `exit 2`. Then write a valid team there (the `VALID` text from Task 1) and rerun: the plan prints with the two panes and exits 0.

```bash
git commit -m "CLI: --template repo resolves the project's own team" -- crates/launcher-cli/src/main.rs
```

---

### Task 5: the team step

**Files:**
- Modify: `app/src/api.ts` (`Template`, `ProjectStatus`, `listTemplates`)
- Modify: `app/src/App.tsx` (state effects `:136-142` and `:160-170`, the `TeamStep` props and `presets` `:696-720`, the segment buttons `:830-840`)
- Modify: `app/src/styles.css` (append)

- [x] **Step 1: Types and the call**

In `app/src/api.ts`, add to `Template`:

```ts
  /// From the project's own .herdr/team.toml.
  from_repo: boolean;
```

Add to `ProjectStatus`:

```ts
  /// The project's .herdr/team.toml exists but failed to load: its message.
  team_file: string | null;
```

Change the listing call:

```ts
  listTemplates: (project: string | null) =>
    invoke<Template[]>("list_templates", { project }),
```

- [x] **Step 2: Re-list when the project changes, and preselect**

In `App()`, remove the `api.listTemplates()` line from the mount effect and add, after the project-status effect:

```tsx
  // The team list depends on the project: a repository may carry its own
  // team, which is offered first and preselected. Leaving such a project
  // drops that selection back to the default.
  useEffect(() => {
    let live = true;
    api
      .listTemplates(project || null)
      .then((ts) => {
        if (!live) return;
        setTemplates(ts);
        const repo = ts.find((t) => t.from_repo);
        setTemplateId((id) => (repo ? "repo" : id === "repo" ? "squad" : id));
      })
      .catch((e) => live && setError(String(e)));
    return () => {
      live = false;
    };
  }, [project]);
```

Pass `status` into the team step: add `status={status}` where `<TeamStep` is rendered, and `status: ProjectStatus | null;` to its props.

- [x] **Step 3: Order, tag, error line**

In `TeamStep`, replace `presets`:

```tsx
  // Size order, except that the repository's own team always comes first.
  const presets = useMemo(() => {
    const sorted = [...props.templates].sort((a, b) => a.panes.length - b.panes.length);
    return [...sorted.filter((t) => t.from_repo), ...sorted.filter((t) => !t.from_repo)];
  }, [props.templates]);
```

In the segment button, after `<span className="l">{t.display_name}</span>`:

```tsx
            {t.from_repo && (
              <span className="tag repo" data-testid="template-repo-tag">
                this repo
              </span>
            )}
```

Directly after the closing `</div>` of the `.seg` block:

```tsx
      {props.status?.team_file && (
        <p className="state warn" data-testid="team-file-error">
          .herdr/team.toml: {props.status.team_file}
        </p>
      )}
```

- [x] **Step 4: Style**

Append to `app/src/styles.css`:

```css
.segbtn .tag.repo { margin-left: 6px; border-color: var(--acc-line); color: var(--acc); }
```

- [x] **Step 5: Verify and commit**

Run: `cd app && npm run build`
Expected: `tsc` silent; vite `✓ built`. Until Task 3 lands, the running app's `list_templates` ignores the `project` argument and `team_file` is absent, which is harmless.

```bash
git commit -m "Team step: offer the repository's own team first" -- app/src/api.ts app/src/App.tsx app/src/styles.css
```

---

### Task 6: QA and the user's eyes

Needs Tasks 1–5 on main. Build once with `cargo tauri dev` on the isolated session (`HERDUP_SESSION=herdup-qa`) as in the update work; every click and every look is the user's.

- [x] **Step 1: Fixtures**

Create three scratch project folders: `good` with the `VALID` team from Task 1 and no `display_name`; `bad` with `display_name = 1`; `plain` with no file. `git init` each so the project step does not warn about version control.

- [x] **Step 2: CLI**

```bash
cargo run -q -p launcher-cli -- plan --template repo --cwd <good>    # two panes, exit 0, title is the folder name
cargo run -q -p launcher-cli -- plan --template repo --cwd <bad>     # the loader message, exit 2
cargo run -q -p launcher-cli -- plan --template repo --cwd <plain>   # "no .herdr/team.toml in …", exit 2
cargo run -q -p launcher-cli -- plan --template squad --cwd <good>   # built-ins still work there
```

- [x] **Step 3: GUI, reported by the user**

Pick `good`: the team step shows the folder's name first with a *this repo* tag and it is selected; Solo, Duo, Squad, Full team follow. Pick `bad`: no repo entry, a warning line *.herdr/team.toml: …* under the sizes, Squad selected. Pick `plain`: no line, Squad selected. Go back from `good` to `plain`: the selection returns to Squad.

- [x] **Step 4: Record**

Add the outcome to the plan doc's Phase list as Phase 11 in the style of Phase 10, and update the README status table, in one docs commit.

---

## Self-review

**Spec coverage.** File shape and defaults: Task 1. Discovery on project pick: Task 3 step 3. Team step order, tag, preselection, inline error: Task 5. Planning and launch via `repo`: Task 3 step 4. CLI: Task 4. Error table: Task 1 (loader), Task 3 (app messages), Task 4 (CLI messages). Tests: Tasks 1, 2, 4 step 4, Task 6. Risks: unchanged code paths for preview and warnings.

**Type consistency.** `from_repo` on `TemplateDto` ↔ `Template.from_repo`; `team_file` on `ProjectStatusDto` ↔ `ProjectStatus.team_file`; command `list_templates` takes `project: Option<String>` ↔ `listTemplates(project: string | null)`; `REPO_TEMPLATE_ID` is `"repo"` everywhere the frontend hardcodes `"repo"`.

# Save a launched team to the project Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One button on the final screen writes the team that just launched into the project's `.herdr/team.toml`, asking before it replaces an existing file.

**Architecture:** `plan.rs` already resolves each pane once; that resolution is extracted so a second entry point, `resolve_team`, returns the effective team as a `Template`. `template.rs` gains a hand-written TOML writer whose output the existing loader reads back, and a save function. The app remembers the launch's options and one command does the write. The round-trip test between writer and loader is the contract.

**Tech Stack:** Rust 2021, Tauri 2, React 18 + TypeScript strict.

**Spec:** `docs/superpowers/specs/2026-09-04-save-team-to-project-design.md`

**Working agreement.** Two coders share one checkout on `main`. Commit with explicit paths, `git commit -m "…" -- <files>`, so a sibling's staged files never ride along. Run `cargo fmt --all` before every commit. Every commit message ends with:

```
Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01DF5BCBNLmiW4vhZVyqJXLd
```

---

## File structure

| File | Responsibility |
|---|---|
| `crates/launcher-core/src/plan.rs` (modify) | `resolve_flags` extracted; `resolve_team` |
| `crates/launcher-core/src/template.rs` (modify) | `to_repo_toml`, `save_repo_team`, `SaveOutcome` |
| `crates/launcher-core/tests/save_team.rs` (create) | resolution, round-trip and save tests |
| `app/src-tauri/src/lib.rs` (modify) | `last_launch` state, `SaveTeamDto`, `save_team_file` |
| `app/src/api.ts` (modify) | the DTO type and one call |
| `app/src/App.tsx` (modify) | the three states in `DoneStep` |

Order and ownership: Task 1 then Task 2 (core, coder-1). Task 4 (frontend, coder-2) may run alongside. Task 3 (app Rust, coder-2) needs Tasks 1–2 merged. Task 5 is QA plus the user's eyes.

---

### Task 1: `resolve_team` in the planner

**Files:**
- Modify: `crates/launcher-core/src/plan.rs`
- Test: `crates/launcher-core/tests/save_team.rs` (create)

- [ ] **Step 1: Write the failing tests**

Create `crates/launcher-core/tests/save_team.rs`:

```rust
//! Saving the team that launched back into the project.

use launcher_core::plan::{plan, resolve_team, LaunchRequest};
use launcher_core::registry::Registry;
use launcher_core::template::{PaneSpec, Templates, REPO_TEMPLATE_ID};
use std::path::Path;

fn project() -> &'static Path {
    if cfg!(windows) {
        Path::new("D:\\work\\herdup")
    } else {
        Path::new("/work/herdup")
    }
}

fn added(role: &str, cli: &str) -> PaneSpec {
    PaneSpec {
        role: role.to_string(),
        cli: cli.to_string(),
        flags: String::new(),
        briefing: format!("You are {role}."),
        coordinator: false,
        split: None,
    }
}

#[test]
fn the_resolved_team_matches_what_the_plan_launched() {
    let registry = Registry::builtin();
    let templates = Templates::builtin();
    let squad = templates.get("squad").expect("squad");

    let request = LaunchRequest::new(project(), squad)
        .skip_pane(1)
        .override_cli(2, "hermes")
        .add_pane(added("Scribe", "claude"));

    let planned = plan(&request, &registry).expect("plans");
    let team = resolve_team(&request, &registry).expect("resolves");

    assert_eq!(team.id, REPO_TEMPLATE_ID);
    assert_eq!(team.panes.len(), planned.panes.len());
    for (pane, planned) in team.panes.iter().zip(planned.panes.iter()) {
        assert_eq!(pane.role, planned.role, "role");
        assert_eq!(pane.cli, planned.cli, "cli for {}", pane.role);
        assert_eq!(pane.coordinator, planned.coordinator, "coordinator");
        // The command the plan built is the binary plus exactly these flags.
        let entry = registry.get(&pane.cli).expect("registry entry");
        assert_eq!(
            planned.command,
            launcher_core::template::command_line(&entry.binary, &pane.flags),
            "flags for {}",
            pane.role
        );
    }
}

#[test]
fn flags_dropped_by_a_swap_are_not_saved() {
    // squad's panes carry Claude Code's permission flags. Swapping a pane to a
    // CLI with no verified preset for them drops the flags; the saved team must
    // record what ran, not what the template wished for.
    let registry = Registry::builtin();
    let templates = Templates::builtin();
    let squad = templates.get("squad").expect("squad");
    let request = LaunchRequest::new(project(), squad).override_cli(1, "codex");
    let team = resolve_team(&request, &registry).expect("resolves");
    let swapped = &team.panes[1];
    assert_eq!(swapped.cli, "codex");
    assert!(swapped.flags.is_empty(), "{:?}", swapped.flags);
}

#[test]
fn dropping_the_pane_others_split_from_leaves_a_valid_layout() {
    let registry = Registry::builtin();
    let templates = Templates::builtin();
    let full = templates.get("full").or_else(|| templates.get("squad")).expect("a big template");
    let request = LaunchRequest::new(project(), full).skip_pane(0);
    let team = resolve_team(&request, &registry).expect("resolves");
    assert!(team.panes[0].split.is_none(), "the new root has no split");
    for (i, pane) in team.panes.iter().enumerate().skip(1) {
        let split = pane.split.expect("every non-root pane splits from something");
        assert!(split.from < i, "pane {i} splits from {}", split.from);
    }
}
```

The template id for the six-pane team may be `full`, `full-team` or similar; the third test falls back to `squad`, so it passes either way. If `LaunchRequest::new(...).skip_pane(...)` is not the builder shape in `plan.rs`, use whatever the existing tests in `crates/launcher-core/tests/plan.rs` use.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p launcher-core --test save_team`
Expected: `unresolved import launcher_core::plan::resolve_team`.

- [ ] **Step 3: Extract the flag rule**

In `crates/launcher-core/src/plan.rs`, lift the swap rule out of `plan()` into a function directly above it:

```rust
/// The flags a pane actually runs with, and any the swap discarded.
///
/// A template's flags are written for the template's CLI. When the user swaps
/// the CLI, keep them only if the new CLI is known to accept them; otherwise
/// drop them and say so. Same rule as the registry itself: never use a flag
/// nobody verified for that CLI.
fn resolve_flags<'a>(
    spec: &'a PaneSpec,
    cli_id: &str,
    entry: &crate::registry::CliEntry,
) -> (&'a str, Option<String>) {
    let swapped = cli_id != spec.cli;
    let wanted = spec.flags.trim();
    let accepted = entry
        .flag_presets
        .iter()
        .any(|preset| preset.trim() == wanted);
    if swapped && !wanted.is_empty() && !accepted {
        ("", Some(wanted.to_string()))
    } else {
        (wanted, None)
    }
}
```

In `plan()`, replace the inline block that computes `(flags, dropped_flags)` with:

```rust
        let (flags, dropped_flags) = resolve_flags(k.spec, &cli_id, entry);
```

and delete the comment that moved with it. Everything else in `plan()` is unchanged.

- [ ] **Step 4: `resolve_team`**

Add after `plan()`:

```rust
/// The team this request actually launches, as a [`Template`].
///
/// The same `compact()` and the same per-pane CLI and flag resolution as
/// [`plan`], so what comes back is what ran: dropped panes are absent, a
/// swapped CLI is the one that started, added roles are included, and splits
/// are remapped onto the compacted indices. Written back out this is a
/// `.herdr/team.toml` the loader reads.
pub fn resolve_team(
    request: &LaunchRequest<'_>,
    registry: &Registry,
) -> Result<crate::template::Template> {
    let template = request.template;
    if let Some(&bad) = request.skip.iter().find(|&&i| i >= template.panes.len()) {
        return Err(PlanError::SkipOutOfRange {
            index: bad,
            count: template.panes.len(),
        });
    }
    let kept = compact(template, &request.extra, &request.skip)?;
    let mut panes = Vec::with_capacity(kept.len());
    for k in &kept {
        let cli_id = k
            .original_index
            .and_then(|i| request.cli_overrides.get(&i).cloned())
            .unwrap_or_else(|| k.spec.cli.clone());
        let entry = registry.get(&cli_id).ok_or_else(|| PlanError::UnknownCli {
            role: k.spec.role.clone(),
            cli: cli_id.clone(),
        })?;
        let (flags, _dropped) = resolve_flags(k.spec, &cli_id, entry);
        panes.push(PaneSpec {
            role: k.spec.role.clone(),
            cli: cli_id,
            flags: flags.to_string(),
            briefing: k.spec.briefing.clone(),
            coordinator: k.spec.coordinator,
            split: k.split.as_ref().map(|s| crate::template::Split {
                direction: s.direction,
                ratio: s.ratio,
                from: s.from,
            }),
        });
    }
    Ok(crate::template::Template {
        id: crate::template::REPO_TEMPLATE_ID.to_string(),
        display_name: request
            .workspace_label
            .clone()
            .unwrap_or_else(|| template.display_name.clone()),
        description: template.description.clone(),
        panes,
    })
}
```

Add `use crate::template::PaneSpec;` to the module imports if it is not already there.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p launcher-core --test save_team`
Expected: `test result: ok. 3 passed`.

If `Template` or `PaneSpec` fields are not constructible from outside `template.rs` because a field is private, they are all `pub` today; if that has changed, add a constructor there rather than making fields public ad hoc.

- [ ] **Step 6: Lint and commit**

```bash
cargo fmt --all
cargo clippy -p launcher-core --all-targets -- -D warnings
cargo test --workspace
git commit -m "Resolve the team a request launches, for saving it back" -- crates/launcher-core/src/plan.rs crates/launcher-core/tests/save_team.rs
```

---

### Task 2: the writer and the save

**Files:**
- Modify: `crates/launcher-core/src/template.rs`
- Test: `crates/launcher-core/tests/save_team.rs` (append)

- [ ] **Step 1: Write the failing tests**

Append to `crates/launcher-core/tests/save_team.rs`:

```rust
use launcher_core::template::{
    parse_repo_team, save_repo_team, to_repo_toml, SaveOutcome, Template, REPO_TEAM_FILE,
};
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("herdup-save-team-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn team_with(briefing: &str) -> Template {
    Template {
        id: "repo".to_string(),
        display_name: "Saved team".to_string(),
        description: "from a launch".to_string(),
        panes: vec![
            PaneSpec {
                role: "PM".to_string(),
                cli: "claude".to_string(),
                flags: "--permission-mode bypassPermissions".to_string(),
                briefing: briefing.to_string(),
                coordinator: true,
                split: None,
            },
            added("Dev", "claude"),
        ],
    }
}

/// The second pane needs a split to be a valid team; `added` leaves it None.
fn writable(mut team: Template) -> Template {
    team.panes[1].split = Some(launcher_core::template::Split {
        direction: launcher_core::herdr::types::SplitDirection::Right,
        ratio: Some(0.5),
        from: 0,
    });
    team
}

#[test]
fn a_written_team_reads_back_identically() {
    let registry = Registry::builtin();
    for briefing in [
        "One line.",
        "Two\nlines, with a \"quote\".",
        "A backslash \\ and a triple \"\"\" quote.",
    ] {
        let team = writable(team_with(briefing));
        let text = to_repo_toml(&team);
        let back = parse_repo_team(&text, "team.toml", project(), &registry)
            .unwrap_or_else(|e| panic!("{briefing:?} did not round-trip: {e}\n{text}"));
        assert_eq!(back.panes, team.panes, "{briefing:?}\n{text}");
        assert_eq!(back.display_name, team.display_name);
    }
}

#[test]
fn a_simple_briefing_is_written_readably() {
    let team = writable(team_with("Do the work.\nThen stop."));
    let text = to_repo_toml(&team);
    assert!(text.contains("briefing = \"\"\""), "{text}");
    assert!(text.contains("Do the work.\nThen stop."), "{text}");
}

#[test]
fn saving_creates_the_herdr_folder() {
    let project = scratch("fresh");
    let team = writable(team_with("Go."));
    match save_repo_team(&project, &team, false).expect("saves") {
        SaveOutcome::Written(path) => {
            assert_eq!(path, project.join(REPO_TEAM_FILE));
            assert!(path.is_file());
        }
        other => panic!("{other:?}"),
    }
    std::fs::remove_dir_all(&project).unwrap();
}

#[test]
fn an_existing_file_is_reported_not_replaced() {
    let project = scratch("exists");
    let file = project.join(REPO_TEAM_FILE);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "# hand written\n").unwrap();
    let team = writable(team_with("Go."));

    match save_repo_team(&project, &team, false).expect("reports") {
        SaveOutcome::Exists(path) => assert_eq!(path, file),
        other => panic!("{other:?}"),
    }
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "# hand written\n");

    match save_repo_team(&project, &team, true).expect("overwrites") {
        SaveOutcome::Written(_) => {}
        other => panic!("{other:?}"),
    }
    assert!(std::fs::read_to_string(&file).unwrap().contains("[[pane]]"));
    std::fs::remove_dir_all(&project).unwrap();
}
```

If `SplitDirection` is not re-exported at `launcher_core::herdr::types`, use the path the other tests use; `crates/launcher-core/tests/plan.rs` imports it.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p launcher-core --test save_team`
Expected: unresolved imports for `to_repo_toml`, `save_repo_team`, `SaveOutcome`.

- [ ] **Step 3: The writer**

In `crates/launcher-core/src/template.rs`, after `parse_repo_team`:

```rust
/// A team as `.herdr/team.toml`, in the bare shape [`parse_repo_team`] reads.
///
/// Hand-written rather than serialised: this file is committed, read and
/// edited by people, and a serialiser would put every briefing on one escaped
/// line. Field order is fixed so a re-save produces a readable diff.
pub fn to_repo_toml(team: &Template) -> String {
    let mut out = String::new();
    out.push_str("# Written by herdup from a launched team. Edit freely.\n");
    out.push_str("# herdup offers this team when the project is opened.\n\n");
    out.push_str(&format!("display_name = {}\n", basic_string(&team.display_name)));
    out.push_str(&format!("description  = {}\n", basic_string(&team.description)));
    for pane in &team.panes {
        out.push_str("\n[[pane]]\n");
        out.push_str(&format!("role     = {}\n", basic_string(&pane.role)));
        out.push_str(&format!("cli      = {}\n", basic_string(&pane.cli)));
        if !pane.flags.trim().is_empty() {
            out.push_str(&format!("flags    = {}\n", basic_string(pane.flags.trim())));
        }
        if pane.coordinator {
            out.push_str("coordinator = true\n");
        }
        if let Some(split) = &pane.split {
            let direction = match split.direction {
                SplitDirection::Right => "right",
                SplitDirection::Down => "down",
            };
            match split.ratio {
                Some(ratio) => out.push_str(&format!(
                    "split    = {{ direction = \"{direction}\", ratio = {ratio}, from = {} }}\n",
                    split.from
                )),
                None => out.push_str(&format!(
                    "split    = {{ direction = \"{direction}\", from = {} }}\n",
                    split.from
                )),
            }
        }
        out.push_str(&format!("briefing = {}\n", briefing_string(&pane.briefing)));
    }
    out
}

/// A TOML basic string: always correct, always one line.
fn basic_string(text: &str) -> String {
    toml::Value::String(text.to_string()).to_string()
}

/// A briefing as a multi-line string when that is safe, else escaped.
///
/// Multi-line keeps a paragraph readable in the committed file. Text holding a
/// backslash or a triple quote cannot go in one without escaping rules that
/// are easy to get subtly wrong, so it falls back to the always-correct form.
fn briefing_string(text: &str) -> String {
    if text.contains('\\') || text.contains("\"\"\"") || text.ends_with('"') {
        return basic_string(text);
    }
    // A leading newline directly after the opening delimiter is trimmed by
    // TOML, so text that starts with one would lose it: use the escaped form.
    if text.starts_with('\n') {
        return basic_string(text);
    }
    format!("\"\"\"\n{text}\"\"\"")
}
```

The multi-line form above puts the text on the line after the opening `"""`, which TOML trims, so the value starts at the text; it ends immediately before the closing delimiter, so a trailing newline in the briefing is preserved. If the round-trip test shows an off-by-one newline, adjust here, not in the loader.

`SplitDirection` is already imported at the top of the module. If its variants are named differently, match the real ones; the loader's `Deserialize` names are the truth.

- [ ] **Step 4: The save**

Add after the writer:

```rust
/// What a save did, so the caller can ask before replacing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveOutcome {
    Written(std::path::PathBuf),
    /// The file is already there and `overwrite` was false. Nothing was read
    /// from it and nothing was written.
    Exists(std::path::PathBuf),
}

/// Write `team` to `<project>/.herdr/team.toml`.
///
/// Refuses an existing file unless `overwrite`, because that file may be
/// committed and hand-edited. Creates `.herdr` when it is missing.
pub fn save_repo_team(
    project: &Path,
    team: &Template,
    overwrite: bool,
) -> std::io::Result<SaveOutcome> {
    let path = project.join(REPO_TEAM_FILE);
    if !overwrite && path.exists() {
        return Ok(SaveOutcome::Exists(path));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, to_repo_toml(team))?;
    Ok(SaveOutcome::Written(path))
}
```

- [ ] **Step 5: Run to verify they pass**

Run: `cargo test -p launcher-core --test save_team`
Expected: `test result: ok. 7 passed`.

- [ ] **Step 6: Lint and commit**

```bash
cargo fmt --all
cargo clippy -p launcher-core --all-targets -- -D warnings
cargo test --workspace
git commit -m "Write a team back out as .herdr/team.toml" -- crates/launcher-core/src/template.rs crates/launcher-core/tests/save_team.rs
```

---

### Task 3: the command

**Files:**
- Modify: `app/src-tauri/src/lib.rs` — `AppState`, a DTO, one command, the handler list

- [ ] **Step 1: Remember the launch**

Add to `AppState`:

```rust
    /// The options the last launch ran with, so the team can be saved back to
    /// the project exactly as it started.
    last_launch: Mutex<Option<LaunchOptions>>,
```

`LaunchOptions` must be `Clone`; add `#[derive(Clone)]` to it if it is not already, keeping its existing derives.

In `launch`, where the outcome is stored, store the options too:

```rust
        if let Some(state) = app.try_state::<AppState>() {
            *state.outcome.lock().unwrap() = Some(outcome);
            *state.last_launch.lock().unwrap() = Some(options.clone());
        }
```

`options` is moved into the closure; clone it before `build_launch_plan` if the borrow checker objects, or store it at the top of the closure.

- [ ] **Step 2: The DTO and the command**

Add near the other DTOs:

```rust
#[derive(Serialize)]
pub struct SaveTeamDto {
    /// False when the file exists and the caller did not ask to replace it.
    written: bool,
    path: String,
}
```

Add the command after `send_briefing_now`:

```rust
/// Write the team that last launched into the project's `.herdr/team.toml`.
///
/// Re-resolves from the launch's own options, so what lands is what ran:
/// dropped panes absent, swapped tools as they started, added roles included.
/// Without `overwrite` an existing file is reported and left untouched.
#[tauri::command]
async fn save_team_file(
    state: State<'_, AppState>,
    overwrite: bool,
) -> Result<SaveTeamDto, String> {
    let options = state
        .last_launch
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
        .ok_or("launch a team first")?;
    tauri::async_runtime::spawn_blocking(move || {
        let registry = launcher_core::config::load_registry().map_err(|e| e.to_string())?;
        let project = PathBuf::from(&options.project);
        let (templates, repo_error) =
            launcher_core::config::load_templates_for(&project, &registry)
                .map_err(|e| e.to_string())?;
        let template = templates.get(&options.template).ok_or_else(|| {
            repo_error
                .map(|e| e.to_string())
                .unwrap_or_else(|| format!("no template '{}'", options.template))
        })?;

        let mut request = LaunchRequest::new(&project, template);
        for index in &options.skip {
            request = request.skip_pane(*index);
        }
        for (index, cli) in &options.overrides {
            request = request.override_cli(*index, cli);
        }
        let addable = launcher_core::template::addable_roles();
        for entry in &options.extra {
            let (id, cli) = match entry.split_once(':') {
                Some((id, cli)) => (id, Some(cli)),
                None => (entry.as_str(), None),
            };
            let role = addable
                .iter()
                .find(|r| r.id == id)
                .ok_or_else(|| format!("no role '{id}' to add"))?;
            let mut spec = role.spec.clone();
            if let Some(cli) = cli {
                if !registry.contains(cli) {
                    return Err(format!("no tool '{cli}' in the registry"));
                }
                spec.cli = cli.to_string();
                spec.flags = String::new();
            }
            request = request.add_pane(spec);
        }

        let team = launcher_core::plan::resolve_team(&request, &registry)
            .map_err(|e| e.to_string())?;
        match launcher_core::template::save_repo_team(&project, &team, overwrite)
            .map_err(|e| format!("could not write the team file: {e}"))?
        {
            launcher_core::template::SaveOutcome::Written(path) => Ok(SaveTeamDto {
                written: true,
                path: path.display().to_string(),
            }),
            launcher_core::template::SaveOutcome::Exists(path) => Ok(SaveTeamDto {
                written: false,
                path: path.display().to_string(),
            }),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}
```

The block that rebuilds the request duplicates `build_plan_inner`. If that function can be split so both share one "options to request" helper without disturbing its reserved-agent-name handling, do that instead and say so; otherwise leave the duplication and add a one-line comment pointing at `build_plan_inner`.

Register `save_team_file` in `tauri::generate_handler![ … ]` after `send_briefing_now`.

- [ ] **Step 3: Verify and commit**

```bash
cargo fmt --all
cargo clippy -p herdup-app --all-targets -- -D warnings
cargo test --workspace
git commit -m "Add the save_team_file command" -- app/src-tauri/src/lib.rs
```

---

### Task 4: the button

**Files:**
- Modify: `app/src/api.ts`
- Modify: `app/src/App.tsx` (`DoneStep`)

- [ ] **Step 1: The call**

In `app/src/api.ts`, after the `CreatedRepo` type:

```ts
/// Result of writing the launched team into the project.
export type SaveTeam = {
  /// False when the file exists and the caller did not ask to replace it.
  written: boolean;
  path: string;
};
```

Inside `export const api = { … }`, after `sendBriefingNow`:

```ts
  saveTeamFile: (overwrite: boolean) => invoke<SaveTeam>("save_team_file", { overwrite }),
```

- [ ] **Step 2: The three states**

In `DoneStep` in `app/src/App.tsx`, after the `release` definition:

```tsx
  // Saving the team into the project: offer, then ask before replacing an
  // existing file, then say where it went.
  const [saved, setSaved] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);

  const save = (overwrite: boolean) => {
    setSaveError(null);
    api
      .saveTeamFile(overwrite)
      .then((r) => {
        if (r.written) {
          setSaved(r.path);
          setConfirming(false);
        } else {
          setConfirming(true);
        }
      })
      .catch((e) => {
        setSaveError(String(e));
        setConfirming(false);
      });
  };
```

Add `useState` to the React import in this file if it is not already there; it is.

- [ ] **Step 3: Render**

In `DoneStep`'s returned markup, directly above the `<div className="actions" style={{ justifyContent: "center" }}>` block that holds *Start another*:

```tsx
      {saved ? (
        <p className="state ok" data-testid="team-saved">
          Saved to {saved} — commit it and this team comes back with the project.
        </p>
      ) : confirming ? (
        <div className="warnbox" data-testid="team-save-confirm">
          <span className="ic">!</span>
          <div>
            <strong>.herdr/team.toml already exists</strong>
            <p>Replace it with the team that just launched?</p>
            <div className="acts">
              <button className="btn solid" onClick={() => save(true)} data-testid="team-save-replace">
                Replace
              </button>
              <button className="btn quiet" onClick={() => setConfirming(false)}>
                Cancel
              </button>
            </div>
          </div>
        </div>
      ) : (
        <div className="actions" style={{ justifyContent: "center" }}>
          <button className="btn" onClick={() => save(false)} data-testid="team-save">
            Save this team to the project
          </button>
        </div>
      )}
      {saveError && (
        <p className="state warn" data-testid="team-save-error">
          {saveError}
        </p>
      )}
```

- [ ] **Step 4: Verify and commit**

Run: `cd app && npm run build`
Expected: `tsc` silent; vite `✓ built`. Until Task 3 lands the command does not exist and the button errors; that is expected.

```bash
git commit -m "Offer to save the launched team to the project" -- app/src/api.ts app/src/App.tsx
```

---

### Task 5: QA and the user's eyes

Needs Tasks 1–4 on main.

- [ ] **Step 1: Core evidence**

```bash
cargo test -p launcher-core --test save_team    # 7 passed
cargo test --workspace
```

Then, without a GUI, prove the round trip against the real loader: write a small scratch binary or a `#[test]` that resolves the `squad` template with one pane skipped and one CLI swapped, writes it to a temp project, and loads it back with `load_repo_team`, asserting the pane list matches. Report the generated file verbatim; it is what a user will commit.

- [ ] **Step 2: GUI, reported by the user**

Start `cargo tauri dev` with `HERDUP_SESSION=herdup-qa` as in previous rounds. The user picks a scratch project with no team file, assembles a small team, launches it, and then:

1. Clicks *Save this team to the project*: expects the saved line naming the path.
2. Reads the written file: expects the roles, tools and briefings of the team that just launched.
3. Reopens the project in the app: expects that team offered first with the *this repo* tag, from the previous feature.
4. Clicks save again on a fresh launch: expects the replace question, and Cancel to leave the file untouched.

Because a real launch starts real agents, use the isolated session and close the workspace afterwards.

- [ ] **Step 3: Record**

Add Phase 12 to `docs/superpowers/plans/2026-09-02-herdup-plan.md` in the style of Phase 11, strike the Deferred entry for saving a workspace, add the README status row and a sentence to the README's Team file section saying herdup can write the file for you. One docs commit.

---

## Self-review

**Spec coverage.** Resolution from the launch's own inputs: Task 1. Dropped-swap flags not saved: Task 1 test 2. Layout preserved: Task 1 test 3. Writer and readable briefings: Task 2. Create `.herdr`, refuse then replace: Task 2. Remembering the launch and the command: Task 3. Button, confirmation, saved line, error: Task 4. Round trip against the loader: Tasks 2 and 5.

**Type consistency.** `SaveOutcome::{Written,Exists}` ↔ `SaveTeamDto { written, path }` ↔ `SaveTeam` in TypeScript; command `save_team_file(overwrite: bool)` ↔ `saveTeamFile(overwrite)`; `resolve_team` returns `Template`, which `to_repo_toml` and `save_repo_team` both take.

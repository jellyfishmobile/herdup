# Per-repo team file — design

**Date:** 2026-09-03 · **Status:** design approved in conversation; spec awaiting review.

A repository can carry its own team shape in `.herdr/team.toml`. When that
project is picked, herdup offers that team first. The file is versioned with
the code, so a team's roles, CLIs, flags and briefings travel with the repo.

## Decisions already taken

| Decision | Choice | Why |
|---|---|---|
| What the file is | A full template: the same shape as one entry in the built-in templates file | The loader already knows it; everything about the team is editable per repo; no second schema |
| File shape | Bare: the top level *is* the template, no wrapping key | Least to type; the id is not the author's to choose |
| Id and name | Id is always `repo`; `display_name` defaults to the folder name | One stable id the GUI and CLI can both address |
| Team step | The repo's team is listed first, tagged as from the repository, and preselected; built-ins stay listed | A quick fix into that repo can still use Solo |
| CLI | `launcher-cli launch --template repo` honours it | GUI and CLI must agree on what `repo` means |
| Invalid file | Error shown inline under the team list; built-ins remain usable | A typo in the file must never block launching |

## Non-goals

- Writing the file. Authoring is manual; "save this team to the repo" is a
  later feature.
- `extends` or any inheritance from a built-in.
- More than one team per repo.
- Watching the file. It is re-read whenever the project is (re)chosen and
  again at launch.
- Trusting the file more than a user's own template. Its flags and briefings
  appear in the plan preview like any other team's.

## The file

`<project>/.herdr/team.toml`:

```toml
display_name = "herdup squad"          # optional; defaults to the folder name
description  = "PM, two coders, QA — the shape this repo is built with"

[[pane]]
role        = "PM"
cli         = "claude"
flags       = "--permission-mode bypassPermissions"
coordinator = true
briefing    = "You coordinate this team..."

[[pane]]
role     = "Coder 1"
cli      = "claude"
flags    = "--permission-mode bypassPermissions"
split    = { direction = "right", ratio = 0.5, from = 0 }
briefing = "You implement features..."
```

Pane fields, meanings and validation are exactly those of `templates.toml`
(design spec §7.2): the first pane is the root and the only one without
`split`; every other `split.from` references a lower index; at most one
`coordinator = true`, and it must be index 0. Every `cli` must be a registry
id. Unknown keys are rejected, so a wrapping table such as `[squad]` is an
error that names the key.

## Behaviour

**Discovery.** The moment a project folder is chosen, herdup looks for
`.herdr/team.toml` under it, in the same cheap read-only pass that reports
git state. Three outcomes: no file; a valid team; an invalid file with a
message.

**Team step.** With a valid team, the list shows it first with a
*from this repository* tag, selected. Built-ins follow in their usual order.
Adding a role, dropping a pane and swapping a CLI work on it as on any team.
With an invalid file, an inline line under the list reads
*`.herdr/team.toml`: <loader message>* and the first built-in is selected as
today. With no file, nothing changes.

**Planning and launch.** Template id `repo` resolves against the chosen
project. The plan, preflight, first-run gate and executor are unchanged: they
receive a `Template` and do not know where it came from.

**CLI.** `launcher-cli launch --template repo --cwd <path>` loads the file from
`<path>`. Without the file, the error is
*no .herdr/team.toml in <path>*; an invalid file reports the loader message.
`launcher-cli templates` (or whatever lists templates today) shows `repo`
only when given a project that has one.

## Architecture

### launcher-core

- `template.rs` gains:
  - `pub const REPO_TEMPLATE_ID: &str = "repo"` and
    `pub const REPO_TEAM_FILE: &str = ".herdr/team.toml"`.
  - `pub fn load_repo_team(project: &Path, registry: &Registry) -> Option<Result<Template>>`:
    `None` when the file is absent; `Some(Err)` for a read, parse, invariant
    or registry failure, each carrying the file path; `Some(Ok)` otherwise,
    with id `repo` and the folder-name default applied.
  - `Templates::with_repo_team(self, team: Template) -> Templates` inserts it
    at the front, replacing any earlier `repo`.
  - The pane invariants and registry check are factored so `from_toml` and
    `load_repo_team` share them; behaviour of `from_toml` is unchanged.
- `Template` gains `from_repo: bool` (default false) so DTOs can tag it.
- Errors reuse `ConfigError`; a new variant only if none fits the
  wrapping-key case.

### app/src-tauri

- `ProjectStatusDto` gains `team_file: Option<String>` — the error message when
  the file exists and is invalid, else `None`.
- `list_templates` takes `project: Option<String>` and, when the project has
  a valid team, returns it first with `from_repo: true`; `TemplateDto` gains
  that field.
- `preview_plan`, `run_preflight`, `launch` and `start_first_run` already take
  the project in `LaunchOptions`; template resolution for `repo` goes through
  one helper that calls `load_repo_team` and reports its error as the command
  error.

### Frontend

- `Template` type gains `from_repo`; `ProjectStatus` gains `team_file`.
- The team step re-requests the list when the project changes, preselects
  the `from_repo` entry when present, renders the tag, and shows the
  `team_file` error line.

### launcher-cli

- `--template repo` resolves via the same helper. The listing command shows
  `repo` for a project that has one.

## Error handling

| Situation | GUI | CLI |
|---|---|---|
| No file | nothing shown | `repo` unknown: *no .herdr/team.toml in <path>* |
| TOML syntax error | inline: file path and line/column from the parser | same message, exit non-zero |
| Wrapping key or unknown field | inline: *unexpected key `squad`; the file is one team, not a list* | same |
| Invariant broken (root has split, coordinator not first) | inline with the existing invariant message | same |
| Unknown CLI id | inline: *unknown cli `foo` in pane 2* | same |
| File valid but a CLI not installed | handled by preflight as for any team | same |

## Testing

- Loader (`tests/template.rs` or a new `tests/repo_team.rs`): valid file;
  missing `display_name` defaults to the folder name; wrapping key rejected
  with the key named; unknown field rejected; unknown CLI rejected; root pane
  with `split` rejected; coordinator at index 1 rejected; absent file gives
  `None`; unreadable file gives `Some(Err)`.
- `Templates::with_repo_team` puts `repo` first and replaces an earlier one.
- Planning: `repo` produces the same plan as an identical built-in (`plan.rs`).
- CLI: `launch --template repo` in a folder without the file exits non-zero
  with the message above (argv-level test in the CLI's existing style).
- GUI: no harness on macOS; the user checks the tag, the preselection and the
  inline error by eye, as with the update work.

## Risks

- A repo's file can name any flag, including a permission-bypass one, for any
  CLI. The plan preview already shows every command before launch and the
  file is tagged as from the repository; that is the mitigation, and it is
  the same exposure as a user's own template.
- Re-reading at launch means a file edited between preview and launch is
  used as edited. Acceptable: the preview is advisory, and the same is true
  of user templates today.

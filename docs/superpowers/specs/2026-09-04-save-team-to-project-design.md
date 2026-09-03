# Save a launched team to the project — design

**Date:** 2026-09-04 · **Status:** design approved in conversation; spec awaiting review.

After herdup launches a team, one button writes that exact team into the
project's `.herdr/team.toml`. Commit it and the shape travels with the code,
where the per-repo team file feature (2026-09-03) reads it back.

## Decisions already taken

| Decision | Choice | Why |
|---|---|---|
| Where it is written | The project's `.herdr/team.toml` | The file the launcher already reads; versioned with the code |
| What can be saved | Only a team herdup itself launched | herdr reports roles and CLIs but not flags, briefings or layout; a reconstruction would be a different team wearing the same name |
| Where the button lives | The final screen, after a launch | "Save the team that is running" |
| Existing file | Asks before overwriting; nothing is written until confirmed | The file may be committed and hand-edited |
| Format | Hand-written TOML, briefings as multi-line strings | The file is read and edited by people; a serialiser would emit one long escaped line |

## Non-goals

- Saving from the review screen, before a launch.
- Saving to the user's own `templates.toml`.
- Saving a workspace herdup did not launch, including one started by hand in
  herdr or one from an earlier run of herdup.
- Editing or merging an existing team file. It is replaced wholesale, on
  confirmation, exactly as a user template replaces a built-in.
- Committing or staging the file. herdup writes it; git is the user's.

## Behaviour

**The button.** On the final screen, beside *Open a terminal* and *Start
another*, a *Save this team to the project* button. It is present whenever the
launch produced panes, including a partial launch, because the team that was
planned is still the team the user assembled.

**A clean save.** With no existing file, the click writes
`<project>/.herdr/team.toml`, creating `.herdr` if needed, and the button is
replaced by the line *Saved to .herdr/team.toml — commit it and this team
comes back with the project.*

**An existing file.** The first click reports *`.herdr/team.toml` already
exists. Replace it?* with *Replace* and *Cancel*. Nothing is read from or
written to the file until *Replace* is clicked. Cancel restores the button.

**What is written.** The team as launched, which is the template with the
user's edits applied: panes dropped are absent, a swapped tool is the tool that
ran, added roles are included in the order they were added, and each pane keeps
the flags that were actually used. Flags the planner dropped, because a swapped
tool had no verified preset for them, are not written; that is what ran. The
coordinator and the split layout are preserved. `display_name` is the workspace
label; `description` records that it was saved from a launch, with the date.

**Failure.** A write that fails, because the folder is read-only or the path is
not writable, reports the reason inline and changes nothing.

## Architecture

### launcher-core

`plan.rs` already resolves every pane exactly once inside `plan()`. Two
extractions, no behaviour change:

- `fn resolve_flags(spec: &PaneSpec, cli_id: &str, entry: &CliEntry) -> (String, Option<String>)`
  — the existing swap rule, lifted verbatim out of the loop in `plan()` and
  called from it.
- `pub fn resolve_team(request: &LaunchRequest<'_>, registry: &Registry) -> Result<Template>`
  — runs the same `compact()` and the same per-pane CLI and flag resolution,
  and returns a `Template` whose panes are the effective ones, with splits
  remapped onto the compacted indices. Id is `REPO_TEMPLATE_ID`.

`template.rs` gains the writer:

- `pub fn to_repo_toml(team: &Template) -> String` — the bare shape the loader
  reads: `display_name`, `description`, then one `[[pane]]` per pane with
  `role`, `cli`, `flags` when non-empty, `coordinator` when true, `split` when
  present, and `briefing` last. A briefing with neither a backslash nor a
  triple quote is written as a multi-line basic string so it stays readable;
  anything else falls back to a single escaped line, which is always correct.
- `pub fn save_repo_team(project: &Path, team: &Template, overwrite: bool) -> Result<SaveOutcome>`
  — `SaveOutcome::Written(PathBuf)` or `SaveOutcome::Exists(PathBuf)` when the
  file is there and `overwrite` is false. Creates `.herdr` as needed.

The writer's output must satisfy the loader: a round-trip test is the contract.

### app/src-tauri

- `AppState` gains `last_launch: Mutex<Option<LaunchOptions>>`, set by `launch`
  next to the outcome it already stores.
- `save_team_file(overwrite: bool) -> Result<SaveTeamDto, String>` rebuilds the
  request from those options exactly as `build_plan` does, calls
  `resolve_team`, then `save_repo_team`. `SaveTeamDto { written: bool, path: String }`,
  with `written: false` meaning the file exists and confirmation is needed.
  Without stored options the error is *launch a team first*.

### Frontend

`DoneStep` gains three states: the button, the replace question, and the saved
line, plus an inline error. One new API call.

## Testing

- `resolve_team` equals the launched shape: a template with one pane skipped,
  one CLI swapped and one role added resolves to the pane list the plan
  produced, compared role by role on role, cli, flags and coordinator.
- Splits are remapped: dropping the pane that others split from leaves every
  surviving `split.from` pointing at a pane that exists and precedes it.
- Round trip: `to_repo_toml` then `parse_repo_team` returns an identical pane
  list, including a multi-line briefing, a briefing containing a quote, and a
  briefing containing a backslash and a triple quote.
- `save_repo_team` writes into a folder with no `.herdr`; returns `Exists`
  without touching a present file; overwrites when told to.
- The desktop and GUI paths are checked by QA and the user, as before.

## Risks

- The saved file grants whatever flags ran, including a permission bypass. It
  is the team the user assembled and launched, and the loader shows every
  command in the plan preview before anything starts. Same exposure as the
  team file feature already documents.
- Options stored from the launch could drift from what is on disk if the user
  edits `templates.toml` between launching and saving. Re-resolving reads the
  current files, so the save would reflect the edit. Acceptable and rare; the
  saved file is shown to the user by path, and git shows the diff.

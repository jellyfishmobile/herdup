# herdup — interface design

The launcher is used by people who have never heard of herdr. Everything below
follows from that.

## The vocabulary rule

No user-visible string in the app may use herdr's vocabulary. This is not a
style preference — it is the whole design brief. The backend keeps its own
names; the translation happens at the edge, in `app/src/App.tsx`.

| Never shown | Say instead |
|---|---|
| pane | teammate |
| template | team, or a size (1 / 2 / 4 / 6) |
| briefing | instructions |
| workspace, session | (hidden entirely; "your workspace" only as the picture's caption) |
| agent blocked | needs you |
| coordinator | the lead — shown as a tinted lane, not a word |
| first run | approve access |
| preflight | one last look |

Backend role names (`PM`, `QA`, `BuildMaster`) still surface, because they come
from `templates.toml` and users can edit that file. Added roles use plain names
(`Lead`, `Coder`, `Tester`, `Builds`, `Research`) from `addable.toml`.

## The two decisions

The flow has six steps but only **two decisions**: which project, and who's on
the team. Everything after is a consequence, so the progress indicator shows two
dots, not six. A newcomer should never feel they are filling in a form with four
screens left to go.

**Step 1 — which project.** Recent folders first (per-machine, `localStorage`),
then "Choose a folder…", with the free-text path demoted below them. The path
input is still there; it is just no longer the first thing a newcomer meets.

**Step 2 — who's on the team.** The climax is a picture of the workspace: one
lane per teammate, the lead's lane tinted. Removal happens on the lane itself
(✕), so there is one control and one picture rather than a list duplicating a
preview. Presets seed the roster; the "+ Role" buttons extend it.

## Quiet unless risky

A clean, version-controlled folder shows **no checks at all**. The UI speaks up
only when something is genuinely un-undoable:

- no git → "has no version history … there's no way back"
- uncommitted changes → "their edits will mix into work you haven't committed"

Both appear on step 1, next to the choice that caused them, not three screens
later. This is why `project_status` exists as a command separate from the full
`run_preflight`: the warning must land at the moment of choice.

Warnings never block. They must be acknowledged individually on the check
screen, because a launch puts file-editing agents into a folder and that should
never be one click away from a mistyped path.

## Two index spaces

`PlannedPane` carries both `index` (compacted — shifts whenever a pane is
dropped) and `origin` (the template index, `null` for an added pane).

**Anything feeding back into `skip` or `overrides` must use `origin`.** Using
`index` silently drops the wrong teammate on the second removal. There is a
regression test for exactly this
(`dropping_twice_removes_the_two_panes_the_user_actually_pointed_at`) and a GUI
check in `app/e2e/run.mjs`.

## The UI never writes prompts

Added roles send an **id** and nothing else. The briefing text lives in
`crates/launcher-core/assets/addable.toml`. A front end that could compose
prompt text would be a way to smuggle instructions into an agent; keeping the
text in the core crate means every prompt is reviewable in one place.

An added pane is never the coordinator — a team has at most one, and it comes
from the template. `PlanError::MultipleCoordinators` enforces it rather than
letting two agents each believe they run the team.

## Visual language

Space Grotesk, vendored through `@fontsource` so the packaged app needs no
network. Hairlines over boxes, one warm accent (`--acc`), tabular figures on
every count so the buttons never reflow as the team changes. Light and dark
palettes are both defined on tokens; nothing gets its only definition inside a
media query.

The lane bars are abstract on purpose — never fake code, never fake text. They
read as "work happening" and claim nothing more.

## What was deliberately not done

- The later four screens (check, approve access, launching, done) were restyled
  and re-worded, not redesigned. They are consequences, not decisions.
- No ambient/decorative motion. An explored variant put a WebGL field behind the
  panel; it was cut because a launcher you use for twenty seconds does not earn
  ambience.

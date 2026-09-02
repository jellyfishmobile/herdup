# herdup — implementation plan

**Date:** 2026-09-02
**Spec:** [`../specs/2026-09-02-herdup-design.md`](../specs/2026-09-02-herdup-design.md)

## Sequencing principle

Two rules drive the order below.

**Verify assumptions before building on them.** The spec's command sequences come
from reading herdr's source and `SKILL.md`, not from watching a real herdr
respond. Every JSON shape in it is an assumption. Phase 0 turns those into
recorded fact before a line of app code depends on them.

**Push the GUI as late as possible.** Phases 1–6 build a working command-line
launcher. By the end of Phase 6 a full team can be launched with no UI at all,
which means the Tauri layer in Phase 7 is a thin shell over proven logic rather
than the place where logic and UI bugs get debugged together.

Sizes are relative (S / M / L), not calendar estimates.

---

## Phase 0 — Ground truth · S · **blocking**

herdr is not currently installed on this machine, and Windows support is
preview-only beta. Everything downstream depends on what it actually does.

**Do**

1. Install herdr (`irm https://herdr.dev/install.ps1 | iex`) and record the version.
2. Start a server; create a scratch workspace.
3. Run and capture raw stdout for each command the spec relies on:
   `workspace list`, `workspace create`, `workspace close`, `tab create`,
   `pane list`, `pane get`, `pane split`, `pane rename`, `pane run`,
   `pane send-text`, `pane send-keys`, `pane read`, `pane close`,
   `wait agent-status`, `server agent-manifests --json`.
4. Launch a real `claude` in a pane and record `agent_status` through its whole
   lifecycle: starting → at prompt → mid-task → awaiting permission.
5. Sign out of one CLI and record what `agent_status` reports at a login prompt.

**Deliverable** — `tests/fixtures/herdr/*.json` (real captured output) and
`docs/notes/2026-09-02-herdr-ground-truth.md`.

**Exit criteria**

- [ ] Every JSON path the spec names is confirmed present, or the spec is corrected.
      Specifically: `result.pane.pane_id`, `result.workspace`, `result.tab`,
      `result.root_pane`.
- [ ] `agent_status` at a **login prompt** is recorded. The spec assumes `blocked`
      or `unknown`. **If a logged-out CLI reports `idle`, §5 of the spec is wrong
      and the briefing-safety guarantee collapses** — stop and revise before Phase 1.
- [ ] Minimum supported herdr version chosen and written into the spec.

> This phase exists to fail cheaply. It is the only phase whose outcome can
> invalidate the design.

---

## Phase 1 — `herdr_cli` · M

The single module that knows herdr's command shapes.

**Do**

- Rust workspace: `crates/launcher-core` (lib) and `crates/launcher-cli` (bin).
- Typed wrapper: one function per herdr command, spawning via
  `std::process::Command` with an argv vector — **never through a shell** (spec §6.3).
- Typed errors: `HerdrNotFound`, `ServerUnavailable`, `CommandFailed{code,stderr}`,
  `ParseError`, `Timeout`.
- Deserialise against the Phase 0 fixtures.
- Test double: `tests/fake_herdr/` — a small binary that reads a script file and
  echoes canned responses, including scripted `agent-status` transitions.

**Tests** — parse every Phase 0 fixture; argv construction is correct for paths and
briefings containing spaces, quotes, and non-ASCII; each error variant is produced
by the matching fake-herdr failure.

**Exit** — `herdr_cli` drives a real herdr end to end from a scratch test, and the
whole suite passes against the fake with no herdr installed.

---

## Phase 2 — `registry` + `template` · M

Pure data. No process spawning.

**Do**

- Schemas per spec §7.1 / §7.2; `serde` + `toml`.
- Built-in `registry.toml` — 18 entries keyed to herdr's manifest ids. Verified
  `flag_presets` only for Claude Code; every other entry ships `[""]` (spec §7.1).
- Built-in `templates.toml` — Solo, Duo, Squad, Full team, with the six role
  briefings from spec §10.
- User-file merge by `id`, user over built-in.
- Load-time validation of the two invariants: first entry is the root pane and
  omits `split`; a `coordinator` must be index 0. Errors name the offending
  template and field.

**Tests** — merge precedence; unknown keys rejected with a useful message; each
invariant violation produces its specific error; every built-in template loads
and validates; every built-in `cli` id resolves to a registry entry.

**Exit** — configs load, merge, and validate; malformed input fails legibly.

---

## Phase 3 — plan generation · M

The heart of the design, and entirely pure.

**Do**

- `Step` enum and `LaunchPlan` per spec §6.2.
- `plan(project, template, registry) -> Result<LaunchPlan>`.
- Ordering rule from spec §8: coordinator's pane created **first**, its
  `SendBriefing` emitted **last**.
- `PaneRef` → real `pane_id` resolution at execute time.
- Briefing flattening: newlines stripped at send time; templates keep readable
  multi-line source.
- Coordinator briefing assembled from the finished roster, including the
  ID-compaction warning and the match-on-role-label instruction (spec §9).

**Tests** — snapshot the step sequence for each built-in template; coordinator
pane first / briefing last; flattening produces exactly one line; a dropped pane
(preflight remediation) renumbers `from` references correctly; the coordinator
briefing contains every teammate's role and CLI.

**Exit** — every built-in template produces a correct plan. **No herdr required to
run these tests.**

---

## Phase 4 — executor · M

**Do**

- Walk a `LaunchPlan` through `herdr_cli`, resolving `PaneRef`s as panes appear.
- `AwaitIdle` maps to `wait agent-status --status idle`; treat `idle`/`done` as
  ready, `blocked` as needs-attention, timeout as needs-attention (spec §5).
- Withhold `SendBriefing` for any pane not ready; record it as `needs_briefing`
  with its recent output.
- On step failure: **stop, do not roll back** (spec §11). Return which step, which
  pane, and what remains unexecuted.
- Progress events streamed to the caller (a channel; Phase 7 forwards these to the UI).

**Tests (against fake herdr)** — the four scripted scenarios from spec §12:
blocked-then-idle fires the briefing; never-leaves-`working` times out and
withholds; teardown-then-`ReReadPaneIds` uses post-compaction IDs; a mid-plan
failure leaves earlier panes untouched and reports the right step.

**Exit** — plans execute; failures are legible; no briefing is ever sent to a
non-ready pane.

---

## Phase 5 — `preflight` + `auth` · M

**Do**

- Preflight (spec §8 Stage 0): herdr present and ≥ minimum version; server up, or
  start `herdr server` detached and retry once; `where.exe` / `which` per distinct
  CLI; `gh auth status` when GitHub is in play.
- Remediation model: for each missing CLI return the three options (install hint /
  switch CLI / drop pane) as data the UI renders.
- `[verified]` cache read/write in `settings.toml`.
- Stage 1 orchestration: setup workspace, one bare-binary pane per unverified CLI,
  1 s polling of `pane get` + `pane read`, URL/device-code extraction by regex,
  5-minute cap, cancellable.
- Teardown then `ReReadPaneIds`, in that order.

**Tests** — preflight classifies present/missing correctly with a stubbed
`where`; cache hit skips Stage 1 and cache miss doesn't; a CLI reaching `idle`
writes `[verified]`; cancellation tears down the setup workspace; URL and device
codes are extracted from realistic login output.

**Exit** — a launch with a missing CLI is blocked with options; a launch with an
unauthenticated CLI routes through Stage 1 and completes.

---

## Phase 6 — terminal handoff + CLI milestone · S

**Do**

- Windows: `wt.exe -d <project> herdr`, falling back to
  `powershell.exe -NoExit -Command herdr`.
- macOS: `osascript` driving Terminal.app.
- Overridable via `settings.toml`.
- Wire `launcher-cli` into a real command: `launcher-cli launch --project P --template squad`.

**Tests** — command construction per OS and for a path with spaces. Actually
opening a terminal is verified manually.

**Exit — the plan's first true milestone.** A full team launches from the command
line on both OSes, with no GUI in existence. Everything after this is presentation.

---

## Phase 7 — Tauri app · L

**Do**

- Tauri v2 scaffold; React + TypeScript + Vite.
- Tauri commands wrapping Phases 2–6. No business logic in the front end.
- Progress events from Phase 4 streamed to the UI.
- Screens: project picker (live herdr workspaces + native folder picker) →
  template picker with per-role CLI/flags editing → preflight checklist →
  Stage 1 sign-in → launch progress → done, with per-pane *Send briefing now* and
  *Retry* for anything that needs attention.

**Tests** — Tauri commands unit-tested on the Rust side. UI verified manually
against the checklist below.

**Exit** — every Phase 6 flow is reachable through the GUI, plus the manual checks.

---

## Phase 8 — GitHub new-project flow · S

**Do**

- `gh --version` and `gh auth status` detection; owner list via `gh api user`.
- `gh repo create <owner>/<name> --private|--public --clone` into the projects root.
- On success, feed the clone path into the normal launch flow.
- When `gh` is absent or logged out, disable **only** this flow with the reason
  shown; launching is unaffected (spec §11).

**Tests** — argv construction for public/private/owner combinations; absent-`gh`
degradation touches nothing else.

**Exit** — new repo → clone → team launched, in one pass.

---

## Phase 9 — packaging · M

**Do** — `.msi` and `.dmg` via Tauri bundler; app icon; first-run surfacing of
herdr's Windows preview-beta warning (spec §13); README with install and a
30-second quick start.

**Exit** — a clean machine can install and launch a team.

---

## Manual verification checklist

Automated tests cannot cover these. Run on **both** OSes.

- [ ] Full-team launch into a real repo; all six panes correct, named, briefed.
- [ ] Coordinator can actually read and drive a sibling pane via `herdr pane read` / `pane run`.
- [ ] Launch with one CLI uninstalled → blocked, all three remediations work.
- [ ] Launch with one CLI logged out → Stage 1 completes, team builds briefed.
- [ ] Kill a pane mid-launch → clear error, earlier panes survive, retry works.
- [ ] Close a pane, then have the coordinator re-read IDs → it recovers via role labels.
- [ ] Terminal handoff lands in the right workspace and cwd.
- [ ] Edit `registry.toml` flags, relaunch → the edit is used and survives.

---

## Risk register

| Risk | Phase | Handling |
|---|---|---|
| Logged-out CLI reports `idle` | 0 | **Invalidates spec §5.** Stop; fall back to per-CLI login patterns and accept the verification cost. |
| herdr JSON differs from the spec's assumption | 0 | Correct the spec from fixtures before Phase 1. |
| Windows herdr beta is unstable | 0, 6 | Smoke-test Windows **first**, not last. If blocking, ship macOS and gate Windows behind the beta warning. |
| `agent_status` misreads a CLI | 4 | Degrades to a withheld briefing and a visible button — never a wrong action. |
| Tauri sidecar/PATH resolution differs from a dev shell | 7 | Resolve `herdr` and `gh` by absolute path at preflight; don't inherit assumptions from the dev environment. |

## Deferred

Not in this plan; revisit once the launcher is proven.

- Native GitHub OAuth device flow (spec §3 — `gh` covers it)
- Disk-scanning project browser
- Committed per-repo `.herdr/team.toml`
- Saving a running workspace back out as a template
- Verified flag presets for the other 17 CLIs — added as they're confirmed, not guessed

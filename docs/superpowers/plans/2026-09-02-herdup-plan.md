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

**Never test against the default herdr session.** A developer's live session owns
real panes, and `herdr server stop` exits every one of them. Every test and every
manual check uses an isolated named session, which gets its own socket *and* state
directory:

```
herdr --session herdup-test server
herdr --session herdup-test <command>
herdr session stop herdup-test && herdr session delete herdup-test
```

`HERDR_SOCKET_PATH` alone is **not** sufficient — Phase 0 found it redirects the
socket while the server still reads the shared state directory, restoring a
duplicate of the live session's panes.

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

**Exit criteria — COMPLETE 2026-09-02.** Findings:
[`docs/notes/2026-09-02-herdr-ground-truth.md`](../../notes/2026-09-02-herdr-ground-truth.md)

- [x] Every JSON path the spec names is confirmed present. `result.workspace`,
      `result.tab`, `result.root_pane`, `result.pane.pane_id` all exist.
      ID *format* differs from the 0.7.0 docs (`w1` / `w1:t1` / `w1:p1`) — we parse,
      never construct, so no impact.
- [x] **The guarantee failed, and the spec has been corrected.** Not via a login
      prompt but a *trust-this-folder* prompt: **Gemini CLI reported `idle` while a
      blocking modal was on screen**, and a test briefing was swallowed by it —
      the trailing Enter selecting "Trust folder" and granting a real permission.
      Spec §5.1 now tiers the guarantee per-CLI (`verified` / `manual`).
- [x] Minimum herdr pinned to **0.8.2**. Pane IDs are monotonic there, so the
      compaction handling is dropped.

> This phase existed to fail cheaply, and it did exactly that. The design's central
> safety claim was disproven in an afternoon rather than in Phase 4 — and would
> have shipped as a security bug, since the failure silently grants folder trust.

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

**Exit — COMPLETE 2026-09-02.**

- [x] 27 tests green: 18 against `fake-herdr`, 9 parsing the Phase 0 fixtures.
      None require herdr to be installed.
- [x] `launcher-cli smoke` drives real herdr 0.8.2 end to end — create workspace,
      split, rename, list, verify cwd normalisation, verify ID monotonicity,
      close — in a disposable named session it creates and destroys.
- [x] `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` clean.

**Bug found and fixed during this phase:** herdr writes success envelopes to
stdout but **API error envelopes to stderr**. Phase 0 missed it because every
capture used `2>&1`. A parser reading stdout alone turned every API error into a
generic "command failed", which made `server_running()` report a dead server as
alive — so `smoke` skipped starting one and then failed. Both streams are now
parsed, with regression tests for each.

---

## Phase 2 — `registry` + `template` · M

Pure data. No process spawning.

**Do**

- Schemas per spec §7.1 / §7.2; `serde` + `toml`.
- Built-in `registry.toml` — 18 entries keyed to herdr's manifest ids. Verified
  `flag_presets` only for Claude Code; every other entry ships `[""]` (spec §7.1).
- `briefing_trust` on every entry: `"verified"` for `claude` only, `"manual"` for
  the other 17 (spec §5.1). `binary` is a **base name**, never a filename —
  Phase 0 found three different install shapes across four CLIs.
- Built-in `templates.toml` — Solo, Duo, Squad, Full team, with the six role
  briefings from spec §10.
- User-file merge by `id`, user over built-in.
- Load-time validation of the two invariants: first entry is the root pane and
  omits `split`; a `coordinator` must be index 0. Errors name the offending
  template and field.

**Tests** — merge precedence; unknown keys rejected with a useful message; each
invariant violation produces its specific error; every built-in template loads
and validates; every built-in `cli` id resolves to a registry entry.

**Exit — COMPLETE 2026-09-02.**

- [x] 18 registry entries, keyed to herdr's manifest ids, with a test asserting
      the set matches exactly.
- [x] 4 templates (solo/duo/squad/full-team), all six role briefings written.
- [x] Field-level user merge for the registry; whole-template replacement for
      templates. Unknown keys rejected naming both the file and the bad key.
- [x] All six layout invariants enforced with their own error variant and test.
- [x] On-disk loading from `%APPDATA%\herdup\` / `~/Library/Application Support/herdup/`,
      with a missing directory falling back to built-ins rather than failing.
- [x] `launcher-cli config` prints the merged result. 29 config tests; 56 total.
      clippy `-D warnings` and `fmt --check` clean.

**Judgement call recorded:** `binary` ships best-effort base names for the 14
CLIs not present on the dev machine, while `flag_presets` and `briefing_trust`
ship empty/`manual` for everything unverified. The asymmetry is deliberate — a
wrong base name fails loudly and harmlessly at preflight and is fixed by editing
one field, whereas a wrong permission flag or trust tier fails *silently* and
could disable a sandbox or type into a live prompt. Tests enforce both halves.

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
  match-on-role-label instruction (spec §9).

**Tests** — snapshot the step sequence for each built-in template; coordinator
pane first / briefing last; flattening produces exactly one line; a dropped pane
(preflight remediation) renumbers `from` references correctly; the coordinator
briefing contains every teammate's role and CLI.

**Exit — COMPLETE 2026-09-02.**

- [x] `Step`/`LaunchPlan`/`PaneRef`, and `plan()` as a pure deterministic function.
- [x] Exact step sequence asserted for the built-in templates; coordinator pane
      created first, briefed last; every split references an earlier pane.
- [x] Coordinator briefing carries a symbolic roster resolved to real pane ids at
      execution time, and names each teammate's role, pane and CLI.
- [x] Dropping a pane re-points orphaned children to their nearest surviving
      ancestor; dropping the root promotes the next survivor and clears its split.
- [x] `BriefingGate` on every briefing step, so the Phase 0 finding is carried
      into execution rather than re-derived there.
- [x] `launcher-cli plan` prints a dry run. 21 plan tests; 77 total. clippy
      `-D warnings` and `fmt --check` clean. **No herdr required.**

**Bug found by reading the dry run:** swapping a pane's CLI carried the template's
flags across, producing `gemini --permission-mode acceptEdits` — a Claude Code
flag handed to Gemini. A test had asserted this behaviour and a comment had
rationalised it. Flags now survive a CLI swap only if the new CLI's registry
entry lists them, and a discarded flag is recorded in `dropped_flags` and shown,
never dropped silently. Same rule as the registry: never use a flag nobody
verified for that CLI.

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

**Tests (against fake herdr)** — the scripted scenarios from spec §12:
blocked-then-idle fires the briefing; never-leaves-`working` times out and
withholds; a mid-plan failure leaves earlier panes untouched and reports the right
step; and the regression Phase 0 bought us — **a `manual`-tier CLI reporting
`idle` must NOT be auto-briefed**, which is the Gemini case as an executable test.

Note: the old `ReReadPaneIds` scenario is gone with the compaction handling.

**Exit — COMPLETE 2026-09-02.**

- [x] Executor walks a plan, resolving `PaneRef`s to real ids as panes appear.
- [x] `AwaitIdle` trusts the **observed** `agent_status` over the wait's exit
      code — a wait can land while the pane is really sitting on a prompt.
- [x] Two independent briefing gates: the pane must have been observed `Ready`,
      **and** the CLI must be `verified`. Either one withholds. A pane never
      observed ready is withheld rather than assumed.
- [x] Withheld briefings are kept for release, with the pane's recent output
      captured so the UI can show *why*.
- [x] No rollback on failure: earlier panes stand, the failing step is named,
      and the workspace id is still returned so the user can go look.
- [x] 9 executor tests, 86 total. clippy `-D warnings` and `fmt --check` clean.

The Phase 0 finding now has an executable regression test:
`an_unverified_cli_reporting_idle_is_still_not_briefed` scripts herdr into
insisting the pane is `idle` and asserts the briefing is *withheld anyway*.

**Design correction during the phase:** the first draft reused `Starting` for
both "created" and "settled", which made the briefing gate ambiguous — a pane
whose readiness was never confirmed could have been briefed. Split into an
explicit `Ready` state so that only an *observed* ready pane can be typed into.

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
- Teardown: close setup panes, then the setup workspace.
- Stage 1 runs **in the target project directory** so first-run trust prompts are
  cleared there rather than resurfacing in Stage 2. Cache key is CLI + project.

**Tests** — preflight classifies present/missing correctly with a stubbed
`where`; cache hit skips Stage 1 and cache miss doesn't; a CLI reaching `idle`
writes `[verified]`; cancellation tears down the setup workspace; URL and device
codes are extracted from realistic login output.

**Exit — COMPLETE 2026-09-02.**

- [x] Binary detection behind a `BinaryResolver` trait, so tests stub PATH
      instead of depending on what the machine happens to have installed.
- [x] Missing CLI blocks and names the panes needing it, with an install command,
      a docs link, and the installed CLIs it could be switched to.
- [x] Verification cache keyed by **CLI + project**, because a trust prompt is
      per-folder and the tighter key can never wrongly skip. Trailing separators
      normalised; a corrupt settings file degrades to defaults rather than
      refusing to launch.
- [x] Stage 1 runs the **bare binary with no flags** (asserted on exact argv) in
      the target project directory, mirrors pane output, and lifts URLs and
      device codes into copy-ready hints.
- [x] Abandoning the pass caches nothing; teardown closes the workspace even if
      a pane refuses to close.
- [x] `launcher-cli preflight` verified against the real machine. 24 preflight
      tests, 110 total. clippy `-D warnings` and `fmt --check` clean.

**UX correction found by running it for real:** a stopped server was listed as
"blocking launch" on the same line that said herdup starts one automatically.
Split into `blocking_issues()` (the user must act) and `auto_resolvable_issues()`
(herdup handles it), so `can_launch()` no longer sends people to fix a non-problem.

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

**Exit — COMPLETE on Windows 2026-09-02; macOS verified 2026-09-03.**

- [x] **A full six-agent team launched end to end from the command line, with no
      GUI in existence** — 75.5 s, exit 0, all six briefed, coordinator briefed
      last with a roster naming every teammate's agent name and pane.
- [x] Terminal handoff: argv built with no shell on either platform; 10 tests
      cover both shapes from either host. **Actually opening a terminal is still
      the manual check**, on both OSes.
- [x] 131 tests, clippy `-D warnings` and `fmt --check` clean.
- [x] macOS (2026-09-03): `launcher-cli smoke` passed against herdr 0.8.2, and
      the Terminal.app handoff opened a window running herdr in an isolated
      session. Terminal.app runs the generated script inside the user's login
      shell, so a bare `herdr` resolved from `~/.local/bin`.

**This phase forced the agent-API rework** — see
[`docs/notes/2026-09-02-agent-api-discovery.md`](../../notes/2026-09-02-agent-api-discovery.md).
`wait agent-status` does not exist on herdr 0.8.2; the executor had been calling
it since Phase 4 and reading exit 2 as a timeout. The design now uses
`agent start` and `agent prompt`, so herdr enforces the no-typing-into-a-dialog
property alongside herdup's own registry tier.

**Method correction for later phases:** `cargo run` holds the process tree and
makes a finished launch look like a hang. Test with the built binary.

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

**Exit — BUILT, PARTIALLY VERIFIED 2026-09-02.**

- [x] Tauri v2 + React + TypeScript scaffolded by hand (not the interactive
      generator), building as a workspace member. `tsc --noEmit` and `vite build`
      clean; the Rust crate compiles and clippy `-D warnings` passes.
- [x] Twelve commands wrapping Phases 2–6. **No business logic in the app crate**
      — it maps `launcher-core` types to DTOs and streams progress events.
- [x] Capabilities scoped to what the window actually uses: core defaults, event
      listen/unlisten, and the folder picker. No filesystem, shell or HTTP
      permission is granted to the webview.
- [x] All six screens implemented, including per-pane *Send briefing now*,
      *switch CLI* / *drop pane* remediation, and first-run hint copying.
- [x] **Verified visually:** the window renders and IPC works — the first screen
      showed live `list_workspaces` output.
- [x] **Verified by a selector-based harness** (`app/e2e/`, 12 checks): screen
      navigation, IPC, plan preview, template selection, and every guardrail.
      Runs the real release binary under `tauri-driver`, addressing elements by
      `data-testid` — a selector cannot land outside the window, which is what
      made the earlier coordinate-clicking attempt dangerous.
- [ ] Completing a launch through the GUI is **deliberately not** covered: the
      harness stops before the final button, so it starts no agents and creates
      no session. Launching is verified in Phase 6 against real herdr.
- [x] macOS (2026-09-03): `cargo tauri build` produces
      `herdup_0.1.0_aarch64.dmg` and the app launches. The selector harness
      cannot run here — tauri-driver has no macOS backend — so the GUI flow is
      still unverified on a Mac. Two macOS-only bugs had to be fixed before
      anything could resolve: launchd's bare PATH (herdup now adopts the login
      shell's PATH at startup) and the herdr fallback probe missing herdr's
      actual install directory, `~/.local/bin`.

**Two bugs the harness found immediately, both invisible to the earlier method:**

1. **`cargo build` does not produce a working Tauri app.** Even a release build
   opened on *"localhost refused to connect"* — the dev/production decision is
   made by the Tauri CLI through env vars a plain cargo build never sets. Must
   use `cargo tauri build`.
2. **Synchronous Tauri commands froze the window.** `run_preflight` shells out to
   `herdr`, `git` and `gh` on the main thread; under WebDriver it deadlocked with
   no error, and in normal use it would freeze the window for seconds. Every
   process-spawning command is now `async` + `spawn_blocking`.

**Notable:** the app deliberately uses herdup's own named herdr session, so it
can never disturb workspaces the user started themselves — the same isolation
rule the tests follow.

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

**Exit — BUILT, LIVE PATH UNVERIFIED 2026-09-02.**

- [x] `gh` detection, owner list, repo creation and clone, wired into both the
      CLI (`new-repo`) and the GUI (a collapsed panel on the project step).
- [x] 15 tests covering argv construction and validation as pure functions —
      no network, no account. Private is the default; `--public` must be asked
      for; an existing destination is refused rather than overwritten.
- [x] Absent or logged-out `gh` disables only this flow, with the reason shown.
- [x] **Live run completed 2026-09-02.** After the user granted `delete_repo`,
      `herdup-e2e-scratch` was created private, cloned, chained into a launch,
      then deleted. Verified: repo `PRIVATE` on GitHub, clone had the right
      remote, repo gone afterwards, 67 other repositories untouched, no herdr
      session or process left behind.

**Two things the live run showed that no unit test could:**

- `gh repo create --clone` on a brand-new repo produces an **empty clone with no
  commits**, on `master` rather than `main`. Preflight reads that correctly as a
  clean repository with no warnings.
- The chained launch entered the first-run stage and waited — correct, since a
  brand-new folder has never been trusted — but with `--no-terminal` there was
  no terminal in which to answer it. The one-pass flow therefore needs a
  terminal, or the caller must clear first-run separately.

**Defect found and fixed:** the echoed command joined argv on spaces, so a
multi-word `--description` printed as `--description Throwaway` and read as if
the value had been truncated. The argv was correct; the echo was not.
`display_command()` now quotes arguments containing whitespace.

---

## Phase 9 — packaging · M

**Do** — `.msi` and `.dmg` via Tauri bundler; app icon; first-run surfacing of
herdr's Windows preview-beta warning (spec §13); README with install and a
30-second quick start.

**Exit — BUILT, CLEAN-MACHINE INSTALL UNVERIFIED 2026-09-02.**

- [x] `herdup_0.1.0_x64_en-US.msi`, 3.33 MB. Validated by administrative
      extract (`msiexec /a`) rather than by installing: the payload is exactly
      `herdup-app.exe`, reporting `ProductName: herdup, FileVersion: 0.1.0`.
- [x] herdr's Windows preview-beta status is surfaced in the environment check,
      in both the GUI and the CLI (spec §13).
- [x] README with install steps, a 30-second start, an explicit "what it will
      not do", and an honest status table.
- [ ] **Never installed on a clean machine**, which is what the criterion
      actually asks. Doing it here would only prove it installs on the machine
      that built it.
- [ ] `.dmg` unbuilt — the bundler correctly skips it on Windows.
- [ ] The MSI is **unsigned**, so SmartScreen warns. Signing needs a
      certificate and is a distribution decision, not a build one.

**Found while packaging:** the first bundle shipped a `herdup_app_lib.dll` that
nothing loads — the Tauri template declares `cdylib`/`staticlib` crate types for
mobile. Removing them was not enough on its own: the bundler sweeps DLLs out of
`target/release`, so the stale artifact kept being packaged until it was deleted.

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

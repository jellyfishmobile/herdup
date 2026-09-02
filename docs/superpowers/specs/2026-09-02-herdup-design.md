# herdup — design

**Date:** 2026-09-02
**Status:** approved for planning
**Target:** Windows 11 and macOS desktop app

## 1. Problem

herdr is a terminal-native agent multiplexer. Standing up a multi-agent team in it
today is manual: create a workspace, split panes one at a time, remember each
agent CLI's permission flags, name the panes, and brief every agent by hand. Doing
that repeatedly is tedious and error-prone, and there is no way to save a team
shape and reuse it.

Two failure modes make the manual flow worse than it looks. An agent CLI that
isn't installed fails at the moment you need it, and an agent CLI that is
installed but not signed in doesn't fail at all — it sits at a login prompt
looking like a running agent.

## 2. Goals

- Pick a project, pick a team template, launch a fully-formed herdr workspace.
- Roles are real: each agent starts with a briefing that tells it what it is.
- The coordinator role can actually drive the other panes.
- Detect missing CLI tools and missing logins *before* they corrupt a launch.
- Create a new GitHub repo and launch a team into it in one flow.

## 3. Non-goals

These are explicitly out of scope, with rationale.

| Not doing | Why |
|---|---|
| Installing or updating herdr | herdr ships `install.sh`, `install.ps1`, `herdr update`, `herdr channel`. Duplicating it adds code-signing, elevation, and uninstall for no gain. We detect herdr and point at its installer. |
| Installing agent CLIs | Auto-running third-party installers is the highest-risk thing this app could do. We show the documented command with a copy button. |
| Our own GitHub OAuth | `gh` already owns a keychain-backed credential. Registering an OAuth app means requesting the broad `repo` scope and storing a token we'd then be responsible for. |
| Rendering agent output as a GUI | herdr's whole premise is that you see the agent's own terminal. We build the layout and hand off to a real terminal. |
| Modifying herdr | Every operation below uses herdr's existing public CLI. No Rust changes upstream, no socket protocol work. |

## 4. Scope decisions

Recorded so the reasoning survives.

1. **Launcher and templates only.** Install and update delegate to herdr.
2. **Tauri desktop app.** A GUI was requested. Tauri keeps the binary small and
   produces real `.msi` / `.dmg` artifacts. Acknowledged tension: herdr's README
   is emphatic about not being a GUI. The resolution is that this app configures
   and launches, then exits the picture — it never wraps the agent views.
3. **Workspace source is the live herdr server**, not a disk scan. The app reads
   running workspaces, tabs, panes and their agent status, and can attach to one
   or add to it. A native folder picker covers launching into a folder that isn't
   yet a workspace.
4. **GitHub via `gh`.** Detect it, shell out to it. No tokens of our own.
5. **Roles carry briefings, and the coordinator is wired.** Otherwise a "PM" and a
   "QA" are the same process with different pane labels.
6. **Staged sign-in.** A dedicated stage resolves logins before the team is built.

## 5. Key technical decision: readiness comes from herdr

The original plan was to detect "this CLI is ready for input" with
`herdr wait output <pane> --match ">"`, which requires inventing a ready-pattern
for every supported CLI.

herdr already solves this. Its 18 detection manifests
(`src/detect/manifests/*.toml`) encode per-agent prompt patterns, and it exposes
the result as a first-class command:

```
herdr wait agent-status <pane_id> --status idle --timeout 30000
```

We use that instead. It is maintained upstream, refreshes via
`herdr server update-agent-manifests`, and distinguishes the states we need:

| `agent_status` | Interpretation | Action |
|---|---|---|
| `idle` | CLI is up and at its prompt | Safe to send the briefing |
| `blocked` | Needs a human — login prompt *or* permission prompt | **Never brief.** Surface the pane. |
| `working` | Still starting or busy | Keep waiting until timeout |
| `done` | Finished work, unseen | Treat as ready |
| `unknown` | Unrecognised output | Keep waiting, then surface |

### 5.1 The guarantee is per-CLI, not universal

**Revised 2026-09-02 after Phase 0 testing. The original form of this section was
wrong and is corrected here** — see
[ground truth §4](../../notes/2026-09-02-herdr-ground-truth.md).

The table above holds only where herdr's detection manifest for that agent is
good. It is not uniformly good. herdr's own README grades its agents, and lists
*"detected but not fully tested: gemini cli, cline."*

Measured on herdr 0.8.2:

- **Claude Code** behaved exactly as the table promises: `blocked` on its
  trust-this-folder prompt, `idle` once answered.
- **Gemini CLI reported `idle` while a blocking trust modal was on screen.**
  Sending the briefing as Stage 2 would have, the text was swallowed by the modal
  and the trailing Enter selected *"1. Trust folder"* — silently granting a
  permission that lets that folder's config execute code.

So `idle` alone cannot gate a keystroke. Each registry entry therefore carries a
`briefing_trust` tier:

| Tier | Meaning | Behaviour |
|---|---|---|
| `verified` | We have reproduced the `blocked → idle` transition for this CLI ourselves | Auto-brief on `idle` |
| `manual` | Everything else — the default | **Never auto-brief.** Show the pane's screen; require a human click on *Send briefing now* |

Today exactly one CLI ships `verified`: `claude`. Promotion requires someone
reproducing the transition and recording it — never assumption. This is the same
rule as flag presets in §7.1: ship blank, fill in what is proven.

The safety property, correctly stated: **a briefing is only ever sent
automatically to a CLI whose blocked-detection we have tested. For every other
CLI a human sees the pane before anything is typed into it.**

## 6. Architecture

One Tauri application. herdr runs unmodified as a child process.

```
webview UI (React + TypeScript)
      │  Tauri commands (typed, async)
      ▼
  launcher ──plan──▶ LaunchPlan ──execute──▶ herdr_cli ──▶ `herdr` ──▶ socket ──▶ herdr server
      │
      ├── registry    known CLIs: binary, install hint, flag presets
      ├── template    role sets: panes, geometry, briefings
      ├── preflight   binary presence, gh auth, verification cache
      ├── auth        stage-1 sign-in orchestration and teardown
      ├── github      gh detection, repo creation
      └── terminal    opens the OS terminal attached to herdr
```

### 6.1 Modules

| Module | Responsibility | Depends on |
|---|---|---|
| `herdr_cli` | Typed wrapper over the `herdr` binary; spawns it and parses JSON. The **only** module that knows command shapes. | `herdr` on PATH |
| `registry` | Load/merge built-in + user CLI definitions | config files |
| `template` | Load/merge built-in + user team templates | config files |
| `preflight` | Detect herdr, `gh` auth, each CLI binary; read verification cache | `herdr_cli`, `registry` |
| `auth` | Stage 1: setup panes, poll, verify, tear down | `herdr_cli`, `registry` |
| `launcher` | Produce a `LaunchPlan`; execute it | all of the above |
| `github` | `gh auth status`, `gh repo create --clone` | `gh` |
| `terminal` | Open terminal attached to herdr | settings |

**Isolation rule:** `launcher` never spawns a process directly; it goes through
`herdr_cli`. `herdr_cli` never makes policy decisions; it maps Rust calls to
herdr commands and back. This keeps the side-effecting surface in one small,
mockable module.

### 6.2 Plan/execute split

`launcher` has two phases with a data structure between them:

```rust
enum Step {
    CreateWorkspace { cwd: PathBuf, label: String },
    CreateTab       { workspace: WorkspaceId, label: String },
    SplitPane       { from: PaneRef, direction: Direction, ratio: f32, cwd: PathBuf },
    RenamePane      { pane: PaneRef, label: String },
    RunCommand      { pane: PaneRef, argv: Vec<String> },
    AwaitIdle       { pane: PaneRef, timeout_ms: u64 },
    SendBriefing    { pane: PaneRef, text: String },
    ClosePane       { pane: PaneRef },
}

struct LaunchPlan { steps: Vec<Step> }
```

`PaneRef` is an index into panes the plan itself creates, resolved to a real
`pane_id` at execution time — the plan is built before any IDs exist.

This matters for two reasons. The UI can render the plan before anything runs, so
"what is this about to do to my machine" is answerable. And plan generation is a
pure function, so the majority of the logic is unit-testable **with herdr not
installed at all**.

### 6.3 Process spawning

`herdr_cli` spawns `herdr` directly via `std::process::Command` with an argv
vector. **Never through a shell.** Briefing text, repo paths, and project labels
are user-controlled data; routing them through `cmd.exe` or `sh -c` would be a
command-injection vector and would break on paths containing spaces. Passing
argv elements directly sidesteps quoting entirely.

The command herdr runs *inside a pane* (`pane run`) is a different matter — that
string is interpreted by the pane's own shell, which is correct and intended.
Registry-supplied binaries and flags are what land there.

## 7. Data model

All files are TOML. User files merge over built-ins by `id`, so upgrading the app
does not clobber edits and users only record their deltas.

**Locations**
- Windows: `%APPDATA%\herdup\`
- macOS: `~/Library/Application Support/herdup/`

### 7.1 `registry.toml`

```toml
[claude]
display_name   = "Claude Code"
binary         = "claude"          # base name only; resolved at preflight
install_hint   = "npm i -g @anthropic-ai/claude-code"
flag_presets   = ["--permission-mode bypassPermissions", "--permission-mode acceptEdits", ""]
briefing_trust = "verified"        # see §5.1; everything else ships "manual"
```

`id` (the table key) matches herdr's detection manifest `id` so that herdr's
sidebar attributes the pane to the right agent.

**Deliberate limitation.** Flag presets ship only where they can be verified.
Every other CLI ships `flag_presets = [""]` plus a free-text field in the UI. A
blank field is honest; a confidently wrong `--dangerously-*` flag could silently
disable someone's sandbox. Users fill theirs in once and it persists.

**`binary` is a base name, never a filename.** Phase 0 found four installed CLIs
in three different shapes on one machine: `claude.exe` in `~/.local/bin` (native),
`gemini.ps1` in the npm prefix (shim), `kimi.exe` in its own tree. An earlier
draft hardcoded `claude.cmd` for Windows and was simply wrong. Preflight resolves
the base name via `where`/`which` and stores the absolute path it finds.

### 7.2 `templates.toml`

```toml
[squad]
display_name = "Squad"
description  = "A coordinator, two coders, and QA."

[[squad.pane]]
role      = "PM"
cli       = "claude"
flags     = "--permission-mode bypassPermissions"
coordinator = true
briefing  = "You coordinate this team..."

[[squad.pane]]
role      = "Coder 1"
cli       = "claude"
split     = { direction = "right", ratio = 0.5, from = 0 }
briefing  = "You implement features..."
```

`from` is the index of the pane to split, so geometry is explicit and any tree is
expressible.

Two invariants, validated at load time with a clear error on violation:

- **The first pane entry is the root pane** and must omit `split`. Every other
  entry must have one, and its `from` must reference a lower index.
- **At most one pane may set `coordinator = true`**, and if present it must be
  index 0 — the coordinator holds the root pane that the others split from.

### 7.3 `settings.toml`

```toml
projects_root  = "D:\\work"
terminal       = "wt.exe"          # per-OS default, overridable

[verified]
claude = 2026-09-01T14:22:00Z      # last time this CLI reached `idle`
droid  = 2026-08-28T09:10:00Z
```

`[verified]` is a **hint, not a guarantee** — tokens expire. It only decides
whether Stage 1 can be skipped. Stage 2's per-pane status check is the real
safety net.

## 8. The three-stage launch

### Stage 0 — Preflight

Runs before anything is created. Blocking.

1. `herdr --version` — missing → show herdr's install command, stop.
2. `herdr workspace list` — succeeds means the server is up. Failure means no
   server; start one with `herdr server` detached and retry once.
3. `gh auth status` — only when the GitHub flow is used. Non-zero exit means not
   signed in. This is the one auth probe that is verified and stable.
4. For each **distinct** CLI in the chosen template: `where.exe <bin>` on Windows,
   `which <bin>` on macOS.

Presence-checking uses `where`/`which` rather than `<bin> --version` because
spawning an npm shim directly from Rust is unreliable on Windows. The real launch
happens inside a herdr pane, which is a genuine shell, so the shim works there.

Missing CLI → the UI offers three remediations, per affected role:
- show the install command with a copy button, then re-check
- switch that role to a different installed CLI
- drop that pane from the launch

### Stage 1 — First-run pass

**Widened from "sign-in" after Phase 0.** Logins are not the only thing that
blocks a fresh CLI. Both Claude Code and Gemini CLI show a *trust this folder*
prompt the first time they run in an unfamiliar directory — which is the **normal**
case for the "create a repo, launch a team into it" flow, not an edge case. Stage 1
clears every first-run interstitial, of which login is one kind.

Runs for any CLI absent from `[verified]`, keyed by **CLI *and* project directory**
— a CLI trusted in one repo is not trusted in the next. Skipped entirely when the
cache is warm.

Both concerns are per-CLI-per-project, not per-pane: three `claude` panes in one
repo need one login and one trust answer. Phase 0 confirmed the trust decision
persists — a second `claude` in the same folder went straight to `idle` in 4.7s.

1. `herdr workspace create --cwd <project> --label "herdup setup" --no-focus`
   — **in the target project directory**, so the trust prompt raised here is the
   same one Stage 2 would otherwise hit.
2. One pane per un-verified CLI, each running the **bare binary with no flags** —
   permission flags are irrelevant to logging in and could suppress the prompt.
3. Open the terminal attached to this workspace. Some login flows need a real TTY;
   the app says so rather than pretending otherwise.
4. Poll each pane once per second:
   - `herdr pane get <id>` for `agent_status`
   - `herdr pane read <id> --source recent --lines 40` for display
5. Mirror pane text in the UI, regex-lifting URLs and device codes into copy
   buttons. Reaching `idle` marks that CLI verified and writes `[verified]`.
6. Cap the stage at 5 minutes, cancellable at any point.

**Teardown.** Close each setup pane (`herdr pane close`), then the setup workspace
(`herdr workspace close`).

> **Obsolete constraint, removed.** An earlier draft ordered teardown before
> Stage 2 because 0.7.0's docs warn that pane IDs compact when panes close. On
> **0.8.2 they are monotonic** — Phase 0 closed `w1:p2`, re-split, and got `w1:p4`,
> not a reused `w1:p2`. The `ReReadPaneIds` step and the ordering rule are dropped,
> and **minimum supported herdr is pinned to 0.8.2** so the assumption holds.

### Stage 2 — Build the team

**Creation order and briefing order are different.** Panes are created in template
order — the coordinator is pane 0, the root pane, and the others split from it, so
it must exist *first*. But its briefing embeds the finished roster, so it is
briefed *last*. Concretely:

1. Create, rename, and `pane run` every pane in template order.
2. Await `idle` and send briefings for every non-coordinator pane.
3. Send the coordinator's briefing once the full roster is known.

Per pane, the command sequence is:

```
herdr pane split <prev> --direction right --ratio 0.5 --cwd <project> --no-focus
    → parse result.pane.pane_id
herdr pane rename <id> "QA"
herdr pane run    <id> "droid"
herdr wait agent-status <id> --status idle --timeout 30000
herdr pane send-text <id> "<briefing>"
herdr pane send-keys <id> Enter
```

The first pane of a new workspace is the root pane returned by
`workspace create`; it is not split into existence.

**Briefings are sent as a single paragraph with newlines stripped at send time.**
Most agent CLIs submit on newline, so a multi-line briefing would fire as several
truncated prompts. Templates may store readable multi-line text; the sender
flattens it.

A briefing is sent automatically only when **both** hold:

1. the pane reports `idle` (or `done`), and
2. the CLI's `briefing_trust` is `verified` (§5.1).

A `manual`-tier CLI is **never** auto-briefed even at `idle` — Phase 0 caught
Gemini reporting `idle` behind a blocking modal, so for untested CLIs a human
looks at the pane first.

Otherwise — `blocked`, a timeout, or a `manual`-tier CLI — the briefing is
**withheld**. The pane stays, marked *needs attention*, with its recent output
and a **Send briefing now** button. This is the same mechanism that protects
against an expired token the Stage 1 cache wrongly considered fresh.

### Stage 3 — Hand off

Open the terminal attached to herdr, focused on the new workspace, and stop
touching anything.

- Windows default: `wt.exe -d <project> herdr`; fallback
  `powershell.exe -NoExit -Command herdr`.
- macOS default: `osascript` driving Terminal.app to run `herdr` in `<project>`.
- Overridable via `settings.toml`.

## 9. Coordinator wiring

The coordinator pane's briefing is assembled at launch, after every other pane
exists, and contains:

- the roster: role name, pane id, CLI for each teammate
- the herdr commands to drive them — `pane read`, `pane run`, `wait agent-status`
- an instruction to match teammates on their **role label**, re-reading with
  `herdr pane list` rather than trusting a remembered id

Role labels are the durable handle, which is why `pane rename` runs on every pane
before any briefing is sent. On herdr 0.8.2 pane IDs are also stable (they do not
compact — see Stage 1 teardown), so the earlier compaction warning has been
dropped from the briefing text; matching on labels remains the instruction because
a pane that is closed and recreated genuinely does get a new id.

## 10. Built-in templates

| Template | Panes |
|---|---|
| Solo | Dev |
| Duo | Dev, Reviewer |
| Squad | PM\*, Coder ×2, QA |
| Full team | PM\*, Coder ×2, QA, BuildMaster, Researcher |

\* coordinator

Role intent, encoded in the shipped briefings:

- **PM** — coordinates, delegates, does not write code itself.
- **Coder** — implements features on the current branch.
- **Reviewer** — reads diffs, does not write features.
- **QA** — runs the test suite and reports failures; does not write features.
- **BuildMaster** — owns builds, CI, and dependency health; the only role told to
  run long build commands.
- **Researcher** — reads docs and the web, writes findings, does not modify code.

Geometry: coordinator takes the root pane (left, wide); the rest split right, then
down. All of it is editable TOML — these are defaults, not constraints.

## 11. Error handling

| Condition | Behaviour |
|---|---|
| `herdr` not on PATH | Block. Show herdr's install command. |
| herdr server not running | Start `herdr server` detached, retry once, then block. |
| CLI binary missing | Block launch. Offer install command / switch CLI / drop pane. |
| `gh` missing or logged out | Disable only the New Project flow, with the reason shown. Launching is unaffected. |
| Pane `blocked` after launch | Withhold briefing, mark *needs attention*, surface output, offer manual send. |
| `wait agent-status` timeout | Same as `blocked`. |
| First-run *trust this folder* prompt | **Expected, not an error.** Normal for any CLI's first run in a new repo; Stage 1 clears it. Present as a step to complete, never as a failure. |
| `manual`-tier CLI reaches `idle` | Withhold briefing anyway (§5.1); show the pane and require a human click. |
| herdr older than 0.8.2 | Block at preflight with the version found. IDs compact below 0.8.x and the design assumes they do not. |
| Any step fails mid-plan | **Stop, do not roll back.** A partial team is still useful and tearing it down destroys work. Report the failing step and pane; offer per-pane retry. |
| Stage 1 cancelled | Tear down setup workspace, return to Stage 0. |

The no-rollback rule is deliberate: these panes may already contain running
agents, and automatic cleanup could kill work in progress.

## 12. Testing

**Unit — no herdr required.** Plan generation is pure. Cases: each built-in
template produces the expected step sequence; the coordinator's pane is created
first but its `SendBriefing` step is ordered last; briefing flattening strips
newlines; registry/template merge honours user overrides; `PaneRef` resolution is
correct; a template violating the root-pane or coordinator-index invariants is
rejected at load with a useful message.

**Integration — fake `herdr` binary.** A test double on PATH that echoes canned
JSON and scripts `agent-status` transitions. This makes the interesting sequences
CI-testable rather than manual:

- codex reports `blocked`, then `idle` after a simulated sign-in → briefing fires
- a pane never leaves `working` → times out, briefing withheld, marked
- a `manual`-tier CLI reports `idle` → briefing is **not** sent (the Gemini case)
- a mid-plan failure leaves earlier panes untouched

**Manual smoke, per OS.** Real herdr, real terminal handoff, one full-team launch.
Terminal-opening and the live socket are the parts a fake cannot cover.

## 13. Risks

| Risk | Mitigation |
|---|---|
| herdr's Windows support is preview-only beta | Detect the platform and surface herdr's own beta warning at first run. Windows is the riskier target and should be smoke-tested first, not last. |
| `agent_status` is heuristic and can misread a CLI | **Confirmed real in Phase 0**, not hypothetical: Gemini reported `idle` behind a blocking modal. Contained by the `briefing_trust` tiering in §5.1 — only CLIs we have tested are auto-briefed. Manifests update via `herdr server update-agent-manifests`. |
| Spec assumptions drift from the shipped herdr | The spec was written against 0.7.0 source; 0.8.2-preview ships, and Windows defaults to the preview channel. ID format and compaction behaviour both changed. Re-run the Phase 0 capture against any new minimum version before raising the pin. |
| A pane closed and recreated gets a new id | Coordinator is briefed to match on role labels and re-read via `herdr pane list`. (Bulk compaction is not a concern on 0.8.2 — IDs are monotonic.) |
| Unverifiable flags for 16 CLIs | Ship blank, let users fill and persist. Never guess a permission flag. |
| herdr CLI surface changes | Confined to `herdr_cli`. Pin a minimum herdr version at preflight and fail loudly on an older one. |

## 14. Stack

Tauri v2, Rust backend, React + TypeScript + Vite frontend, `serde` + `toml` for
config. Artifacts: `.msi` (Windows), `.dmg` (macOS).

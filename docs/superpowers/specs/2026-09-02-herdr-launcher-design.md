# herdr launcher — design

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

This removes the blind-typing hazard: a briefing can never be typed into a login
screen, because a login screen is not `idle`.

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
    ReReadPaneIds,
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
- Windows: `%APPDATA%\herdr-launcher\`
- macOS: `~/Library/Application Support/herdr-launcher/`

### 7.1 `registry.toml`

```toml
[claude]
display_name = "Claude Code"
binary       = { windows = "claude.cmd", unix = "claude" }
install_hint = "npm i -g @anthropic-ai/claude-code"
flag_presets = ["--permission-mode bypassPermissions", "--permission-mode acceptEdits", ""]
```

`id` (the table key) matches herdr's detection manifest `id` so that herdr's
sidebar attributes the pane to the right agent.

**Deliberate limitation.** Flag presets ship only where they can be verified.
Every other CLI ships `flag_presets = [""]` plus a free-text field in the UI. A
blank field is honest; a confidently wrong `--dangerously-*` flag could silently
disable someone's sandbox. Users fill theirs in once and it persists.

The `windows`/`unix` binary split exists because most of these CLIs install as npm
shims — `claude.cmd` on Windows, `claude` on macOS.

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

### Stage 1 — Sign-in pass

Runs only for CLIs absent from `[verified]`. Skipped entirely when the cache is
warm, which is the common case after first run.

Auth is per-CLI, not per-pane: three `claude` panes need one login.

1. `herdr workspace create --cwd <project> --label "herdr-launcher setup" --no-focus`
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

**Teardown, then re-read.** Close each setup pane (`herdr pane close`), close the
setup workspace (`herdr workspace close`), and only then run `ReReadPaneIds`.
herdr's docs are explicit that IDs compact when panes close, so Stage 2 must not
hold any ID allocated before teardown.

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

If `wait agent-status` returns `blocked` or times out, the briefing is **withheld**.
The pane stays, marked *needs attention*, with its recent output and a **Send
briefing now** button. This is the same mechanism that protects against an expired
token that the Stage 1 cache wrongly considered fresh.

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
- an explicit warning that pane IDs compact when panes close, and an instruction
  to re-read them with `herdr pane list` and match on the **role label** rather
  than trusting a remembered id

Role labels are the durable handle, which is why `pane rename` runs on every pane
before any briefing is sent.

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
- setup teardown followed by `ReReadPaneIds` → Stage 2 uses post-compaction IDs
- a mid-plan failure leaves earlier panes untouched

**Manual smoke, per OS.** Real herdr, real terminal handoff, one full-team launch.
Terminal-opening and the live socket are the parts a fake cannot cover.

## 13. Risks

| Risk | Mitigation |
|---|---|
| herdr's Windows support is preview-only beta | Detect the platform and surface herdr's own beta warning at first run. Windows is the riskier target and should be smoke-tested first, not last. |
| `agent_status` is heuristic and can misread a CLI | Failure mode is a withheld briefing plus a visible button, never a wrong action. Manifests update via `herdr server update-agent-manifests`. |
| Pane ID compaction | Teardown-then-re-read is enforced in the plan; coordinator briefed to match on role labels. |
| Unverifiable flags for 16 CLIs | Ship blank, let users fill and persist. Never guess a permission flag. |
| herdr CLI surface changes | Confined to `herdr_cli`. Pin a minimum herdr version at preflight and fail loudly on an older one. |

## 14. Stack

Tauri v2, Rust backend, React + TypeScript + Vite frontend, `serde` + `toml` for
config. Artifacts: `.msi` (Windows), `.dmg` (macOS).

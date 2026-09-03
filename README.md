# herdup

A desktop launcher for [herdr](https://herdr.dev) teams.

herdr is a terminal-native agent multiplexer. herdup stands up a whole
multi-agent team in it from a template — pick a project, pick a team shape, and
get a herdr workspace where every pane runs the right CLI with the right flags
and already knows what its job is.

## Install

**Windows.** Download `herdup_0.1.0_x64_en-US.msi` and run it. The installer is
unsigned, so SmartScreen will warn — "More info" → "Run anyway", or build from
source below.

**macOS.** Download `herdup_0.1.0_aarch64.dmg` (Apple silicon) and drag herdup
into Applications. The app is unsigned and not notarized, so macOS will refuse
to open a downloaded copy the first time — allow it under System Settings →
Privacy & Security → "Open Anyway", or build from source below.

**Updating.** herdup checks the release page a few seconds after it opens and
shows a one-line banner when a newer version exists. *Install and restart*
downloads it, verifies it against herdup's signing key, replaces the app and
relaunches. *Check for updates* in the top bar does the same on demand and, if
it cannot check, says why. On macOS the app must be in Applications — a copy
opened straight from the disk image cannot replace itself, and the banner says
so.

You also need:

| | | |
|---|---|---|
| [herdr](https://herdr.dev) | **0.8.2 or newer** | Windows `irm https://herdr.dev/install.ps1 \| iex` · macOS `curl -fsSL https://herdr.dev/install.sh \| sh` |
| At least one agent CLI | e.g. Claude Code | Windows `irm https://claude.ai/install.ps1 \| iex` · macOS `curl -fsSL https://claude.ai/install.sh \| bash` |
| [`gh`](https://cli.github.com) | optional | only for creating new repositories (macOS: `brew install gh`) |

> **herdr's Windows builds are preview-only beta** and track the preview update
> channel. Linux and macOS have stable releases; Windows does not yet. herdup
> says so in its environment check rather than leaving you to find out.

## 30-second start

1. Open herdup.
2. Choose a project folder — or create a new GitHub repository from the same screen.
3. Pick a team: **Solo**, **Duo**, **Squad** (coordinator + 2 coders + QA), or **Full team** (adds BuildMaster and Researcher).
4. herdup checks your environment: are the CLIs installed, are they signed in, is the folder version-controlled.
5. Launch. Each pane starts its agent and receives its role briefing; the coordinator is briefed last with the finished roster.
6. A terminal opens attached to the team.

Not near a GUI? The same thing from a shell:

```
launcher-cli launch --template squad --cwd D:\work\my-project
```

## What it will not do

These are deliberate.

- **It never types into an agent that is waiting on you.** A briefing sent to a
  dialog would answer that dialog. herdup only auto-briefs CLIs whose
  blocked-detection has been verified by hand — today that is Claude Code alone;
  everything else waits for you to look at the pane and release the briefing.
- **It never touches a herdr session you started.** herdup works only in its own
  named session, so your own workspaces cannot be disturbed. If it finds a herdr
  server on a different protocol it reports it and stops, because restarting
  that server would exit your panes.
- **It never launches into a folder that does not exist**, and it warns before
  putting file-editing agents into a repository with uncommitted work, or into a
  folder with no version control at all. Each warning is acknowledged
  individually.
- **It stores no credentials.** GitHub access goes through `gh`, which already
  owns a keychain-backed token.

## Build from source

```bash
cargo install tauri-cli --version "^2" --locked
cd app
cargo tauri build              # → target/release/bundle/msi/*.msi (Windows)
                               #   target/release/bundle/dmg/*.dmg (macOS)
```

Bundling signs the updater artifacts, so `cargo tauri build` needs the private
key in `TAURI_SIGNING_PRIVATE_KEY` (and its password in
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`). Without a key, build the plain app for
local use with `cargo tauri build --no-bundle`. CI holds the release key.

`cargo build` alone produces a binary that opens on *"localhost refused to
connect"* — the dev/production decision is made by the Tauri CLI, so always
build through it.

Tests:

```bash
cargo test --workspace         # 182 tests; no herdr installation required
cd app && npm run test:e2e     # 12 GUI checks under tauri-driver
```

The GUI harness drives the real window by CSS selector and deliberately stops
before completing a launch, so it starts no agents. See
[`app/e2e/README.md`](app/e2e/README.md).

## Status

| Phase | State |
|---|---|
| 0–6 core, CLI harness | done, verified against real herdr |
| 7 desktop app | done, verified by the selector harness |
| 8 GitHub new-repo | done — verified live: created, cloned and deleted a throwaway repo |
| 9 packaging | `.msi` builds and validates; **never installed on a clean machine** |
| 10 in-app update | built; verified end to end on macOS against a local feed; **first real signed release not yet published** |
| macOS | built and run on macOS 26, Apple silicon (2026-09-03): 182 tests, the CLI smoke against herdr 0.8.2, the Terminal.app handoff and the packaged app launching are all verified; **the GUI flow itself has not been driven on a Mac** — tauri-driver has no macOS backend |

A full six-agent team has been launched end to end on Windows: 75 s, all six
briefed, coordinator briefed last with the roster.

On macOS an app opened from Finder starts with launchd's bare PATH, which has
none of the user's CLIs on it. herdup adopts the login shell's PATH at startup
so that herdr, `gh` and the agent CLIs resolve exactly as they do in a terminal.

## Configuration

`%APPDATA%\herdup\` (Windows) or `~/Library/Application Support/herdup/` (macOS):

- `registry.toml` — known CLIs: binary, flags, whether herdup may brief them unattended
- `templates.toml` — team shapes and role briefings
- `settings.toml` — projects root, terminal command, first-run cache

User values merge over the built-ins, so upgrading does not clobber your edits.
`launcher-cli config` prints the merged result.

## Design

- [Spec](docs/superpowers/specs/2026-09-02-herdup-design.md)
- [Implementation plan](docs/superpowers/plans/2026-09-02-herdup-plan.md)
- [Ground truth](docs/notes/2026-09-02-herdr-ground-truth.md) — what herdr actually does, measured
- [Agent API discovery](docs/notes/2026-09-02-agent-api-discovery.md) — why the design changed mid-build

herdup does not modify herdr, and does not install or update it. Readiness
detection is delegated to herdr's own agent API rather than reimplemented:
`agent start` returns only once an agent is ready, and `agent prompt` refuses to
write to one sitting at a dialog.

## License

TBD. Note that herdr itself is AGPL-3.0-or-later; herdup shells out to the
`herdr` binary rather than linking its code, but this deserves a real answer
before the project takes contributors.

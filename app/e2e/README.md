# GUI end-to-end harness

Drives the real herdup window under `tauri-driver`, addressing elements by
`data-testid` **CSS selector** — never by screen position.

```bash
cd app
cargo tauri build --no-bundle    # see "Why the Tauri CLI" below
npm run test:e2e
```

## Why this exists

An earlier attempt to verify the GUI by clicking screen coordinates went wrong
twice. The second time the window had moved between screenshot and click, so the
clicks landed on the wrong window and drove the app through its whole flow —
starting four agents with bypassed permissions inside a directory nobody
intended. No damage resulted (the agents stopped at Claude Code's trust prompt),
but the method was indefensible.

A selector cannot land outside the window. That is the entire point.

## What it covers, and what it deliberately does not

Covered: screen navigation, IPC round trips, plan preview, template selection,
and every guardrail — the project-exists block, the no-version-control warning,
the acknowledgement gate.

**Not covered on purpose: completing a launch.** These tests never click the
final button, so they start no agents and create no herdr session. Launching is
verified in Phase 6 against a real herdr, where it belongs. A UI test that spawns
real agents into a temporary folder would be slow, expensive and hard to clean
up — and it is not what this harness is for.

## Why the Tauri CLI, not `cargo build`

`cargo build --release -p herdup-app` produces a binary that still points at the
Vite dev server: it opens on *"localhost refused to connect"*. The dev/production
decision is made by the Tauri CLI through environment variables that a plain
cargo build never sets. Always build with `cargo tauri build`.

The harness prefers `target/release/herdup-app.exe` and falls back to the debug
binary, which only works with `npm run dev` running alongside.

## Prerequisites

| Tool | Install | Note |
|---|---|---|
| `tauri-driver` | `cargo install tauri-driver --locked` | WebDriver proxy for Tauri |
| `msedgedriver` | [msedgedriver.microsoft.com](https://msedgedriver.microsoft.com) | **Must match the WebView2 version** — check `HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}` |
| `cargo-tauri` | `cargo install tauri-cli --version "^2" --locked` | |

Override discovery with `EDGE_DRIVER` and `TAURI_DRIVER` if they live elsewhere.

## Files

- `webdriver.mjs` — a ~150-line W3C WebDriver client. Hand-rolled rather than
  pulling in WebdriverIO: the surface needed is about eight endpoints, and this
  way no framework version has to track Tauri's or Edge's.
- `run.mjs` — the tests.
- `debug.mjs` — starts the app and dumps the webview's HTML. Useful when a
  selector times out and you need to know what the window is actually showing.

## Notes for future tests

- On failure the harness prints the current step and the app's own error banner.
  A bare selector timeout is much less useful than the message explaining it.
- Processes are killed with `taskkill /T` so `msedgedriver`, which `tauri-driver`
  starts itself, does not survive the run. Do not spawn with `shell: true`; that
  wraps everything in `cmd.exe` and the real children outlive the kill.
- **Every Tauri command that spawns a process must be `async` + `spawn_blocking`.**
  A synchronous one blocks the main thread: the window freezes, and under
  WebDriver it deadlocks with no error at all. This harness found exactly that in
  `run_preflight`.

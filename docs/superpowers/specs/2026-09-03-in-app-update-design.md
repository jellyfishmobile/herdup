# In-app update — design

**Date:** 2026-09-03 · **Status:** design approved in conversation; spec awaiting review.

herdup notices a newer GitHub release, tells the user, and installs it on
request. macOS and Windows. Linux artifacts are built but not verified.

## Decisions already taken

| Decision | Choice | Why |
|---|---|---|
| What the notice does | Downloads and installs in-app, then restarts | Asked for explicitly |
| Where the feed lives | The public GitHub release of `jellyfishmobile/herdup` | The repo goes public later; until then the feed returns 404 and the app stays silent |
| Auth to a private feed | None | Only testers today; they receive installers directly. Keeps "it stores no credentials" true |
| Mechanism | Tauri's updater plugin, driven only from Rust | The plugin owns the fragile part — verify, replace the bundle, relaunch, admin fallback — and the webview keeps zero network or shell permission |
| Manual check | Yes, a "Check for updates" link in the top bar | Testers need a way to force a check and see why one failed |

## Non-goals

- No token handling for a private feed.
- No "skip this version" persistence. "Not now" lasts for the current run.
- No unprompted install. The user always clicks Install.
- No update check in `launcher-cli`.
- No Linux verification.

## Behaviour

**Startup check.** About three seconds after the first render, the app checks
the feed in the background with a ten-second timeout. Any failure — offline,
404 while the repo is private, bad JSON, timeout — is logged and otherwise
silent. Launching is never delayed or blocked by the check.

**Banner.** When a newer version exists, one line appears directly under the
top bar: *herdup 0.2.0 is available*, with **Install and restart** and
**Not now**. Not now hides the banner until the app is next started.

**Manual check.** A *Check for updates* link in the top bar. States, in order:
*checking…* → *up to date (0.1.0)* or the banner above, or
*could not check: <reason>*. The manual path is the only place a check error
is shown.

**Install.** The banner shows download progress as bytes of total when the
total is known, then *installing…*. The plugin verifies the minisign signature
against the embedded public key, replaces the app, and the app restarts into
the new version. On failure the banner shows the reason and the app remains
usable at the old version.

**macOS translocation.** If the running executable's path contains
`/AppTranslocation/`, the app was opened from a quarantined DMG or Downloads
copy and macOS has mounted it read-only somewhere random. The banner then
reads *Move herdup to Applications, then relaunch to install updates*, with no
Install button. Nothing is attempted.

**Windows.** The installer runs in passive mode; the app exits and the MSI
takes over. Unsigned, so a UAC prompt is expected.

## Architecture

### launcher-core — the pure parts, unit tested

New module `crates/launcher-core/src/update.rs`:

- `pub const DEFAULT_ENDPOINT: &str =
  "https://github.com/jellyfishmobile/herdup/releases/latest/download/latest.json"`
- `pub fn endpoint(settings: &Settings) -> String` — the settings override
  when present and non-blank, else the default.
- `pub fn is_translocated(exe: &Path) -> bool` — true when any path component
  is `AppTranslocation`.

`Settings` gains `update_endpoint: Option<String>` (serde default, so existing
files still load; `deny_unknown_fields` stays).

### app/src-tauri — the plugin and two commands

- Dependency `tauri-plugin-updater = "2"`, registered in the builder.
- `AppState` gains `update: Mutex<Option<tauri_plugin_updater::Update>>`.
- `check_for_update(app) -> Result<Option<UpdateDto>, String>` — async.
  Builds the updater with `endpoints([launcher_core::update::endpoint(&settings)])`
  and a ten-second timeout, runs `check()`, stores the found update in state,
  returns `{ version, current_version, notes, translocated }`.
- `install_update(app) -> Result<(), String>` — async. Takes the stored update,
  refuses with a clear message if none or if translocated, calls
  `download_and_install` with progress emitted as the event `update-progress`
  `{ downloaded, total }`, then `app.restart()`.
- Errors from the plugin are mapped to one short sentence each.

No change to `capabilities/default.json`: the webview calls our commands, not
the plugin.

### Frontend

- `api.ts`: `checkForUpdate()`, `installUpdate()`, `onUpdateProgress(cb)`,
  and the `UpdateDto` type.
- `App.tsx`: an `UpdateBanner` rendered between `.topbar` and the error box;
  the *Check for updates* link inside `.topbar`; the delayed startup check in
  an effect. Styles follow the existing `state`/`errbox` classes.

### Config — `app/src-tauri/tauri.conf.json`

```json
"bundle": {
  "createUpdaterArtifacts": true,
  "macOS": { "signingIdentity": "-" }
},
"plugins": {
  "updater": {
    "pubkey": "<contents of herdup.key.pub>",
    "endpoints": ["https://github.com/jellyfishmobile/herdup/releases/latest/download/latest.json"],
    "windows": { "installMode": "passive" }
  }
}
```

Ad-hoc signing is required, not cosmetic: today only the executable is
linker-signed and `codesign --verify` fails on the bundle. The replaced bundle
must carry a valid signature or Apple silicon may refuse to launch it.

### CI — `.github/workflows/release.yml`

- The tauri-action step gets `TAURI_SIGNING_PRIVATE_KEY` and
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` from repository secrets. tauri-action
  then signs the updater artifacts and uploads `latest.json` to the release
  (its default). MSI remains the Windows artifact the feed references.
- The header comment and release notes are refreshed: macOS has now been run
  by a human; the notes mention that the app updates itself from this release
  page.
- The release stays a draft until published by hand. `releases/latest` only
  resolves published, non-prerelease releases, which is the intended gate.

### The signing key — done by the user, never by an agent

```bash
cd app
cargo tauri signer generate -w ~/.tauri/herdup.key      # choose a password
gh secret set TAURI_SIGNING_PRIVATE_KEY < ~/.tauri/herdup.key
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD        # paste the password
cat ~/.tauri/herdup.key.pub                             # goes into tauri.conf.json
```

Store the key file and the password in a password manager. If the private key
is lost, no installed copy can ever accept another update. The private key
never enters the repository or an agent transcript. Implementation of the
config step waits on the public key.

## Error handling

| Situation | Startup check | Manual check / install |
|---|---|---|
| Offline, DNS failure, timeout | silent | *could not check: network unreachable* |
| Feed returns 404 (repo private, or no published release) | silent | *could not check: no published release* |
| Feed JSON malformed | silent | *could not check: the update feed is malformed* |
| Signature does not verify | — | *the download did not verify; nothing was installed* |
| Bundle replace refused (permissions) | — | plugin's admin prompt; on refusal: *could not replace the app: <reason>* |
| Running translocated | banner with move-to-Applications text | same |
| Install clicked with no stored update | — | *check for updates first* |

## Testing

**Unit (launcher-core):** endpoint default; override used; blank override
ignored; settings round-trip with and without the new field; translocation
true for `/private/var/folders/x1/T/AppTranslocation/ABCD/d/herdup.app/Contents/MacOS/herdup-app`
and false for `/Applications/herdup.app/Contents/MacOS/herdup-app`.

**End to end on this Mac (QA):**

1. Generate a throwaway keypair for the test only.
2. Build the app under test with Tauri's `--config` merge setting the
   throwaway public key and `plugins.updater.dangerousInsecureTransportProtocol: true`,
   with `TAURI_SIGNING_PRIVATE_KEY` pointing at the throwaway key. Copy the
   `.app` to `~/Applications` so it is not translocated.
3. Build a second bundle the same way with `version` merged to `0.1.1`. Serve
   its `.app.tar.gz` and a hand-written `latest.json` from a local static
   server on `127.0.0.1`.
4. Set `update_endpoint` in `~/Library/Application Support/herdup/settings.toml`
   to that server's `latest.json`. Launch the 0.1.0 copy.
5. Verify: banner appears after the delay; Install shows progress; the app
   restarts and reports 0.1.1; the manual check now says up to date.
6. Tamper: flip a byte in the served `.app.tar.gz`; verify install is refused
   with the signature message and 0.1.0 still runs.
7. Remove the override; verify the startup check against the real endpoint is
   silent and the manual check reports *no published release*.
8. Optional: open a quarantined copy from a DMG and verify the translocation
   text.

Release builds never set the insecure-transport flag; it exists only in QA's
merged config.

**Not verified in this effort:** Windows install and restart, Linux, and the
first publish of a real signed release. Each is flagged in the plan doc.

## Risks

- Gatekeeper after an in-place replace: the new bundle is written by the app,
  not a browser, so it carries no quarantine attribute and should open without
  a second "Open Anyway". QA step 5 is the check.
- The `no_console_flash` test enforces `hidden_command` for our own spawns; the
  plugin spawns `msiexec` itself on Windows. Out of our control and expected.
- `app.restart()` immediately after the bundle swap is the plugin's documented
  flow; if it misbehaves on macOS 26 the fallback is *installed; quit and
  reopen herdup*.

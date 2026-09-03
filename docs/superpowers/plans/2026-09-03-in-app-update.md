# In-app update Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** herdup notices a newer published GitHub release, shows a banner, and on request downloads, verifies, installs it and restarts.

**Architecture:** Tauri's updater plugin does verify, install and relaunch, driven only from two Rust commands in the app crate. The pure decisions — which feed URL, whether the running copy is translocated — live in launcher-core with unit tests. The webview renders a banner and calls the commands; it gets no new permissions. The feed is this repo's public latest release; while the repo is private the startup check fails silently.

**Tech Stack:** Rust 2021, Tauri 2, `tauri-plugin-updater` 2, React 18 + TypeScript (strict, no unused locals), GitHub Actions with `tauri-apps/tauri-action`.

**Spec:** `docs/superpowers/specs/2026-09-03-in-app-update-design.md`

**Working agreement.** Two coders share one checkout on `main`. Commit with explicit paths — `git commit -m "…" -- <files>` — so a sibling's staged files never ride along. Run `cargo fmt --all` before every commit. Every commit message ends with:

```
Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01DF5BCBNLmiW4vhZVyqJXLd
```

---

## File structure

| File | Responsibility |
|---|---|
| `crates/launcher-core/src/settings.rs` (modify) | one new optional field, `update_endpoint` |
| `crates/launcher-core/src/update.rs` (create) | the feed URL decision and the translocation check; nothing else |
| `crates/launcher-core/src/lib.rs` (modify) | `pub mod update;` |
| `crates/launcher-core/tests/update.rs` (create) | tests for both of the above |
| `app/src-tauri/Cargo.toml` (modify) | the plugin dependency |
| `app/src-tauri/src/lib.rs` (modify) | plugin registration, `AppState.update`, two DTOs, the error mapper, two commands |
| `app/src/api.ts` (modify) | two types, two calls, one listener |
| `app/src/App.tsx` (modify) | startup check, topbar link, `UpdateBanner` |
| `app/src/styles.css` (modify) | `.updatebar`, `.topbar-right`, `.linkbtn` |
| `.github/workflows/release.yml` (modify) | signing secrets into the build step; refreshed notes |
| `README.md`, `docs/superpowers/plans/2026-09-02-herdup-plan.md` (modify) | user-facing and status docs |
| `app/src-tauri/tauri.conf.json` (modify, last) | updater artifacts, ad-hoc signing, plugin config with the real public key |

Task order and ownership: Tasks 1–2 first (core, coder-1). Task 4 (frontend, coder-2) can run alongside them. Task 3 (app Rust, coder-2) needs Tasks 1–2 merged. Task 5 (CI + docs, coder-1) any time after Task 3. Task 6 waits for the user's public key. Task 7 is QA and needs Tasks 1–5 in the tree; it does not need Task 6.

---

### Task 1: `Settings.update_endpoint`

**Files:**
- Modify: `crates/launcher-core/src/settings.rs:22-35`
- Test: `crates/launcher-core/tests/update.rs` (create)

- [ ] **Step 1: Write the failing tests**

Create `crates/launcher-core/tests/update.rs`:

```rust
//! Where herdup looks for a newer version of itself, and whether it can
//! replace itself where it is running.

use launcher_core::settings::Settings;
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("herdup-update-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn settings_without_the_field_still_load() {
    let s = Settings::from_toml("terminal = \"wt\"\n", "settings.toml").expect("parses");
    assert_eq!(s.update_endpoint, None);
    assert_eq!(s.terminal.as_deref(), Some("wt"));
}

#[test]
fn the_update_endpoint_round_trips() {
    let dir = scratch("roundtrip");
    let mut s = Settings::default();
    s.update_endpoint = Some("http://127.0.0.1:8765/latest.json".into());
    s.save_to(&dir).expect("saved");
    let back = Settings::load_from(Some(&dir));
    assert_eq!(back.update_endpoint.as_deref(), Some("http://127.0.0.1:8765/latest.json"));
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

#[test]
fn an_unset_endpoint_is_not_written() {
    let dir = scratch("unset");
    Settings::default().save_to(&dir).expect("saved");
    let text = std::fs::read_to_string(dir.join("settings.toml")).expect("read");
    assert!(!text.contains("update_endpoint"), "{text}");
    std::fs::remove_dir_all(&dir).expect("cleanup");
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p launcher-core --test update`
Expected: compile error, `no field `update_endpoint` on type `Settings``.

- [ ] **Step 3: Add the field**

In `crates/launcher-core/src/settings.rs`, inside `pub struct Settings`, after the `terminal` field:

```rust
    /// Where to look for a newer herdup. Unset means the public GitHub release
    /// feed; QA points this at a local server. Never a credential.
    #[serde(default)]
    pub update_endpoint: Option<String>,
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p launcher-core --test update`
Expected: `test result: ok. 3 passed`.

If `an_unset_endpoint_is_not_written` fails because `toml` wrote `update_endpoint = ` for `None`, the existing `projects_root`/`terminal` fields would already fail the same way; check with `git stash` before changing anything, and if they do, add `skip_serializing_if = "Option::is_none"` to all three.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git commit -m "Add the update_endpoint setting" -- crates/launcher-core/src/settings.rs crates/launcher-core/tests/update.rs
```

---

### Task 2: `launcher_core::update`

**Files:**
- Create: `crates/launcher-core/src/update.rs`
- Modify: `crates/launcher-core/src/lib.rs:12-23` (module list)
- Test: `crates/launcher-core/tests/update.rs` (append)

- [ ] **Step 1: Write the failing tests**

Append to `crates/launcher-core/tests/update.rs`:

```rust
use launcher_core::update::{endpoint, is_translocated, DEFAULT_ENDPOINT};
use std::path::Path;

#[test]
fn the_default_feed_is_the_public_latest_release() {
    assert_eq!(
        DEFAULT_ENDPOINT,
        "https://github.com/jellyfishmobile/herdup/releases/latest/download/latest.json"
    );
    assert_eq!(endpoint(&Settings::default()), DEFAULT_ENDPOINT);
}

#[test]
fn a_settings_override_replaces_the_feed() {
    let s = Settings {
        update_endpoint: Some("  http://127.0.0.1:8765/latest.json\n".into()),
        ..Settings::default()
    };
    assert_eq!(endpoint(&s), "http://127.0.0.1:8765/latest.json");
}

#[test]
fn a_blank_override_is_ignored() {
    for blank in ["", "   ", "\n"] {
        let s = Settings {
            update_endpoint: Some(blank.into()),
            ..Settings::default()
        };
        assert_eq!(endpoint(&s), DEFAULT_ENDPOINT, "{blank:?}");
    }
}

#[test]
fn a_quarantined_copy_is_translocated() {
    assert!(is_translocated(Path::new(
        "/private/var/folders/x1/abc/T/AppTranslocation/0A1B-2C3D/d/herdup.app/Contents/MacOS/herdup-app"
    )));
}

#[test]
fn an_installed_copy_is_not_translocated() {
    assert!(!is_translocated(Path::new(
        "/Applications/herdup.app/Contents/MacOS/herdup-app"
    )));
    assert!(!is_translocated(Path::new(
        "/Users/me/AppTranslocationNotes/herdup-app"
    )));
    assert!(!is_translocated(Path::new(r"C:\Program Files\herdup\herdup-app.exe")));
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p launcher-core --test update`
Expected: compile error, `could not find `update` in `launcher_core``.

- [ ] **Step 3: Write the module**

Create `crates/launcher-core/src/update.rs`:

```rust
//! Where herdup looks for a newer version of itself.
//!
//! The mechanics — fetch, verify, replace, relaunch — belong to Tauri's
//! updater plugin in the app crate. This module holds only the two decisions
//! that are worth testing without a GUI: which feed to ask, and whether the
//! running copy is somewhere macOS will let it replace itself.

use crate::settings::Settings;
use std::path::Path;

/// The public feed tauri-action publishes with every release.
pub const DEFAULT_ENDPOINT: &str =
    "https://github.com/jellyfishmobile/herdup/releases/latest/download/latest.json";

/// The feed to ask: the settings override when present and non-blank, else
/// [`DEFAULT_ENDPOINT`].
pub fn endpoint(settings: &Settings) -> String {
    match settings.update_endpoint.as_deref().map(str::trim) {
        Some(url) if !url.is_empty() => url.to_string(),
        _ => DEFAULT_ENDPOINT.to_string(),
    }
}

/// Is this executable running from macOS app translocation?
///
/// Opening a quarantined app straight from a DMG or the Downloads folder makes
/// macOS mount a read-only copy under a random `AppTranslocation` path. The
/// bundle cannot be replaced there, so the updater must tell the user to move
/// the app to Applications rather than try and fail.
pub fn is_translocated(exe: &Path) -> bool {
    exe.components()
        .any(|c| c.as_os_str() == "AppTranslocation")
}
```

Add to `crates/launcher-core/src/lib.rs`, in alphabetical position after `pub mod terminal;`:

```rust
pub mod update;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p launcher-core --test update`
Expected: `test result: ok. 8 passed`.

- [ ] **Step 5: Lint and commit**

```bash
cargo fmt --all
cargo clippy -p launcher-core --all-targets -- -D warnings
git commit -m "Add the update module: feed endpoint and translocation check" -- crates/launcher-core/src/update.rs crates/launcher-core/src/lib.rs crates/launcher-core/tests/update.rs
```

---

### Task 3: the two Tauri commands

**Files:**
- Modify: `app/src-tauri/Cargo.toml` (dependencies)
- Modify: `app/src-tauri/src/lib.rs` — imports (`:9-21`), `AppState` (`:228-233`), new DTOs and commands (after `default_projects_root`), builder (`:935-940`)

There is no unit-test harness for the Tauri layer; the rule in this repo is that it stays thin and everything decidable lives in launcher-core (Tasks 1–2). Verification for this task is: it compiles under clippy `-D warnings`, and Task 7 exercises it.

- [ ] **Step 1: Add the dependency**

In `app/src-tauri/Cargo.toml` under `[dependencies]`, after `tauri-plugin-dialog = "2"`:

```toml
tauri-plugin-updater = "2"
```

Run: `cargo check -p herdup-app`
Expected: succeeds (the plugin compiles; nothing uses it yet).

- [ ] **Step 2: Register the plugin and extend state**

In `app/src-tauri/src/lib.rs`, add to the imports:

```rust
use tauri_plugin_updater::UpdaterExt;
```

Change `AppState`:

```rust
#[derive(Default)]
pub struct AppState {
    /// The most recent launch, so a held briefing can be released later.
    outcome: Mutex<Option<Outcome>>,
    first_run: Mutex<Option<FirstRunSession>>,
    /// The update found by the last check, so Install can act on it.
    update: Mutex<Option<tauri_plugin_updater::Update>>,
}
```

In `run()`, add the plugin line directly after `.plugin(tauri_plugin_dialog::init())`:

```rust
        .plugin(tauri_plugin_updater::Builder::new().build())
```

- [ ] **Step 3: Add the DTOs and the error mapper**

After `fn default_projects_root()` in `app/src-tauri/src/lib.rs`:

```rust
/// Result of asking the release feed. `update` is `None` when this copy is
/// current. `current_version` is always present so the UI can say so.
#[derive(Serialize)]
pub struct UpdateCheckDto {
    current_version: String,
    update: Option<UpdateDto>,
}

#[derive(Serialize)]
pub struct UpdateDto {
    version: String,
    notes: Option<String>,
    /// macOS mounted this copy read-only from a quarantined DMG or download;
    /// the bundle cannot be replaced until it is moved to Applications.
    translocated: bool,
}

#[derive(Clone, Serialize)]
pub struct UpdateProgressDto {
    downloaded: u64,
    total: Option<u64>,
    installing: bool,
}

/// One short sentence per plugin failure. The manual check shows these; the
/// startup check drops them.
///
/// The plugin's error enum may gain variants; the final arm keeps this
/// compiling. If a named variant below does not exist in the resolved plugin
/// version, delete that arm — do not guess a replacement.
fn describe_update_error(e: tauri_plugin_updater::Error) -> String {
    use tauri_plugin_updater::Error as E;
    match e {
        E::ReleaseNotFound => "no published release".into(),
        E::Network(msg) if msg.contains("404") => "no published release".into(),
        E::Network(msg) => format!("network: {msg}"),
        E::Reqwest(err) => format!("network unreachable: {err}"),
        E::InsecureTransportProtocol => "the update endpoint must use https".into(),
        E::Minisign(_) | E::SignatureUtf8(_) | E::Base64(_) => {
            "the download did not verify; nothing was installed".into()
        }
        E::Serialization(_)
        | E::InvalidUpdaterFormat
        | E::TargetNotFound(_)
        | E::TargetsNotFound(_) => "the update feed is malformed".into(),
        E::AuthenticationFailed => {
            "administrator approval was refused; nothing was installed".into()
        }
        E::Io(err) => format!("could not replace the app: {err}"),
        other => other.to_string(),
    }
}

fn running_translocated() -> bool {
    std::env::current_exe()
        .map(|exe| launcher_core::update::is_translocated(&exe))
        .unwrap_or(false)
}
```

- [ ] **Step 4: Add the commands**

Directly after the block above:

```rust
/// Ask the release feed whether a newer herdup exists.
///
/// Never blocks launching: the UI calls this a few seconds after first paint
/// and ignores errors; only the "Check for updates" link shows them. The
/// endpoint comes from settings so QA can point it at a local server.
#[tauri::command]
async fn check_for_update(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<UpdateCheckDto, String> {
    let current_version = app.package_info().version.to_string();
    let endpoint = launcher_core::update::endpoint(&Settings::load());
    let url = tauri::Url::parse(&endpoint)
        .map_err(|e| format!("bad update endpoint {endpoint}: {e}"))?;
    let updater = app
        .updater_builder()
        .endpoints(vec![url])
        .map_err(describe_update_error)?
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(describe_update_error)?;
    let found = updater.check().await.map_err(describe_update_error)?;
    let update = found.as_ref().map(|u| UpdateDto {
        version: u.version.clone(),
        notes: u.body.clone(),
        translocated: running_translocated(),
    });
    *state.update.lock().map_err(|e| e.to_string())? = found;
    Ok(UpdateCheckDto {
        current_version,
        update,
    })
}

/// Download, verify against the embedded public key, replace the app, restart.
///
/// Progress goes out as `update-progress` events. On any failure the app is
/// left untouched at the old version and the reason is returned.
#[tauri::command]
async fn install_update(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let update = state.update.lock().map_err(|e| e.to_string())?.clone();
    let Some(update) = update else {
        return Err("check for updates first".into());
    };
    if running_translocated() {
        return Err("Move herdup to Applications, then relaunch to install updates".into());
    }
    let on_chunk = {
        let app = app.clone();
        let mut downloaded: u64 = 0;
        move |chunk: usize, total: Option<u64>| {
            downloaded += chunk as u64;
            let _ = app.emit(
                "update-progress",
                UpdateProgressDto {
                    downloaded,
                    total,
                    installing: false,
                },
            );
        }
    };
    let on_downloaded = {
        let app = app.clone();
        move || {
            let _ = app.emit(
                "update-progress",
                UpdateProgressDto {
                    downloaded: 0,
                    total: None,
                    installing: true,
                },
            );
        }
    };
    update
        .download_and_install(on_chunk, on_downloaded)
        .await
        .map_err(describe_update_error)?;
    app.restart()
}
```

Register both in `tauri::generate_handler![ … ]`, after `default_projects_root`:

```rust
            check_for_update,
            install_update,
```

- [ ] **Step 5: Verify it compiles clean**

Run:
```bash
cargo fmt --all
cargo clippy -p herdup-app --all-targets -- -D warnings
cargo test --workspace
```
Expected: clippy `Finished` with no warnings; all tests pass.

If `Update` is not `Send + Sync` and `AppState` fails to be managed, wrap the field as `Mutex<Option<Arc<Update>>>` and adjust the clone. If `app.restart()` does not type-check as the tail expression, write `app.restart();` on its own line followed by nothing — its return type is `!`.

- [ ] **Step 6: Commit**

```bash
git commit -m "Add check_for_update and install_update over Tauri's updater plugin" -- app/src-tauri/Cargo.toml app/src-tauri/src/lib.rs Cargo.lock
```

Note: until Task 6 lands, the app **panics at startup** because the plugin's config requires a public key. To run it before then, merge a throwaway key in: see Task 7 step 1–2 and use `cargo tauri dev --config <that file>`.

---

### Task 4: the banner and the link

**Files:**
- Modify: `app/src/api.ts` (types after `CreatedRepo`, calls inside `api`)
- Modify: `app/src/App.tsx` (`App()` state/effects `:68-135`, the topbar `:219-228`, new component at the end)
- Modify: `app/src/styles.css` (append)

The frontend has no unit tests; `npm run build` runs `tsc --noEmit` under `strict` with unused locals and parameters rejected, which is the check here. Task 7 exercises the behaviour.

- [ ] **Step 1: API types and calls**

In `app/src/api.ts`, after `export type CreatedRepo …`:

```ts
/// A newer herdup found on the release feed.
export type UpdateInfo = {
  version: string;
  notes: string | null;
  /// macOS mounted this copy read-only from a quarantined download; it must
  /// be moved to Applications before it can replace itself.
  translocated: boolean;
};

export type UpdateCheck = {
  current_version: string;
  update: UpdateInfo | null;
};

export type UpdateProgress = {
  downloaded: number;
  total: number | null;
  installing: boolean;
};
```

Inside `export const api = { … }`, after `onProgress`:

```ts
  checkForUpdate: () => invoke<UpdateCheck>("check_for_update"),
  installUpdate: () => invoke<void>("install_update"),
  onUpdateProgress: (fn: (p: UpdateProgress) => void) =>
    listen<UpdateProgress>("update-progress", (e) => fn(e.payload)),
```

- [ ] **Step 2: State and the startup check in `App()`**

Add to the imports at the top of `app/src/App.tsx` whatever of `UpdateInfo`, `UpdateProgress` is not already imported from `./api` (match the existing import line's style).

Inside `App()`, after the existing `useState` declarations:

```tsx
  // Self-update. The startup check waits until the window has painted and is
  // silent on failure; only the topbar link reports why a check failed.
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  const [updateNote, setUpdateNote] = useState<UpdateNote>({ kind: "idle" });
  const [updateDismissed, setUpdateDismissed] = useState(false);

  useEffect(() => {
    const t = window.setTimeout(() => {
      api
        .checkForUpdate()
        .then((r) => r.update && setUpdate(r.update))
        .catch(() => {});
    }, 3000);
    return () => window.clearTimeout(t);
  }, []);

  const checkForUpdateNow = useCallback(() => {
    setUpdateNote({ kind: "checking" });
    setUpdateDismissed(false);
    api
      .checkForUpdate()
      .then((r) => {
        setUpdate(r.update);
        setUpdateNote(r.update ? { kind: "idle" } : { kind: "current", version: r.current_version });
      })
      .catch((e) => setUpdateNote({ kind: "failed", reason: String(e) }));
  }, []);
```

At module level (near the other top-level types/helpers in `App.tsx`):

```tsx
type UpdateNote =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "current"; version: string }
  | { kind: "failed"; reason: string };
```

- [ ] **Step 3: The topbar link and the banner slot**

Replace the topbar block in `App()`'s return:

```tsx
      <div className="topbar">
        <span className="mark">herdup</span>
        <span className="topbar-right">
          <button
            className="linkbtn"
            onClick={checkForUpdateNow}
            disabled={updateNote.kind === "checking"}
            data-testid="check-updates"
          >
            {updateNote.kind === "checking" ? "checking…" : "Check for updates"}
          </button>
          <span className="dots" aria-hidden>
            <i className={dot === 1 ? "on" : "done"} />
            <i className={dot === 2 ? "on" : ""} />
          </span>
        </span>
      </div>

      {updateNote.kind === "current" && (
        <p className="state ok" style={{ marginBottom: 14 }} data-testid="update-current">
          up to date ({updateNote.version})
        </p>
      )}
      {updateNote.kind === "failed" && (
        <p className="state warn" style={{ marginBottom: 14 }} data-testid="update-failed">
          could not check: {updateNote.reason}
        </p>
      )}
      {update && !updateDismissed && (
        <UpdateBanner update={update} onDismiss={() => setUpdateDismissed(true)} />
      )}
```

- [ ] **Step 4: The banner component**

At the end of `app/src/App.tsx`:

```tsx
type UpdateBannerProps = { update: UpdateInfo; onDismiss: () => void };

/// One line under the topbar. Install shows download progress here, then the
/// app restarts on its own; a failure shows its reason and leaves the buttons.
function UpdateBanner({ update, onDismiss }: UpdateBannerProps) {
  const [progress, setProgress] = useState<UpdateProgress | null>(null);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);

  useEffect(() => {
    const unlisten = api.onUpdateProgress(setProgress);
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  const install = () => {
    setBusy(true);
    setFailure(null);
    api.installUpdate().catch((e) => {
      setFailure(String(e));
      setBusy(false);
      setProgress(null);
    });
  };

  let line: string;
  if (failure) line = failure;
  else if (progress?.installing) line = "installing…";
  else if (progress)
    line = progress.total
      ? `downloading ${mb(progress.downloaded)} of ${mb(progress.total)} MB`
      : `downloading ${mb(progress.downloaded)} MB`;
  else if (update.translocated) line = "Move herdup to Applications, then relaunch to install updates";
  else line = `herdup ${update.version} is available`;

  return (
    <div className="updatebar" role="status" data-testid="update-banner">
      <span className="grow">{line}</span>
      {!update.translocated && !busy && (
        <button className="btn solid" onClick={install} data-testid="update-install">
          Install and restart
        </button>
      )}
      {!busy && (
        <button className="btn quiet" onClick={onDismiss} data-testid="update-dismiss">
          Not now
        </button>
      )}
    </div>
  );
}

const mb = (bytes: number) => (bytes / 1_048_576).toFixed(1);
```

- [ ] **Step 5: Styles**

Append to `app/src/styles.css`:

```css
/* --- self-update ----------------------------------------------------------- */

.topbar-right { display: flex; align-items: center; gap: 14px; }
.linkbtn {
  background: none; border: 0; padding: 0; color: var(--faint);
  font-size: 10.5px; letter-spacing: 0.04em; cursor: pointer;
  transition: color 160ms ease-out;
}
.linkbtn:hover:not(:disabled) { color: var(--ink); }
.linkbtn:disabled { cursor: default; }

.updatebar {
  display: flex; align-items: center; gap: 10px; margin-bottom: 16px; padding: 10px 13px;
  border: 1px solid var(--acc-line); background: var(--acc-soft); border-radius: var(--radius);
  font-size: 13px;
}
.updatebar .grow { flex: 1; min-width: 0; }
```

- [ ] **Step 6: Verify it type-checks and builds**

Run: `cd app && npm run build`
Expected: `tsc` prints nothing; vite reports `✓ built`.

- [ ] **Step 7: Commit**

```bash
git commit -m "Show an update banner and a Check for updates link" -- app/src/api.ts app/src/App.tsx app/src/styles.css
```

---

### Task 5: CI secrets and the docs

**Files:**
- Modify: `.github/workflows/release.yml` (header comment `:1-8`, the `tauri-action` step env and `releaseBody`)
- Modify: `README.md` (install section, build-from-source, status table)
- Modify: `docs/superpowers/plans/2026-09-02-herdup-plan.md` (new Phase 10, risk register, checklist)

- [ ] **Step 1: The workflow**

Replace the header comment:

```yaml
# Build installers for every platform and attach them to a GitHub Release.
#
# Windows and macOS have been run by a human; Linux artifacts are built here
# but have not been exercised. The release notes say so. Do not quietly drop
# that caveat.
#
# The build also signs updater artifacts and uploads latest.json, which the
# app reads to offer in-app updates. That needs the two TAURI_SIGNING_* secrets;
# without them the build step fails rather than shipping unsigned updates.
#
# Triggered by pushing a tag (`git tag v0.1.0 && git push origin v0.1.0`) or by
# hand from the Actions tab. The release is created as a draft; publish it by
# hand after opening the installers once.
```

In the `tauri-apps/tauri-action@v0` step, extend `env`:

```yaml
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
```

Replace `releaseBody`:

```yaml
          releaseBody: |
            Installers for Windows, macOS and Linux.

            **herdup needs [herdr](https://github.com/herdr-dev/herdr) 0.8.2 or newer
            already installed**, plus at least one agent CLI (Claude Code, Codex,
            Gemini CLI, and others). It launches them; it does not bundle them.

            Nothing here is code-signed by Apple or Microsoft, so Windows SmartScreen
            and macOS Gatekeeper will both warn on first run. Once installed, herdup
            offers later releases from inside the app and verifies them against its
            own signing key.

            Tested on Windows and macOS (Apple silicon). The Linux build is produced
            by CI but has not been run by a human — please report what breaks.
```

Run: `python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/release.yml')); print('yaml ok')"` (or `ruby -ryaml -e 'YAML.load_file(".github/workflows/release.yml"); puts "yaml ok"'` if PyYAML is absent).
Expected: `yaml ok`.

- [ ] **Step 2: README**

After the macOS install paragraph (the one starting `**macOS.** Download`), add:

```markdown
**Updating.** herdup checks the release page a few seconds after it opens and
shows a one-line banner when a newer version exists. *Install and restart*
downloads it, verifies it against herdup's signing key, replaces the app and
relaunches. *Check for updates* in the top bar does the same on demand and, if
it cannot check, says why. On macOS the app must be in Applications — a copy
opened straight from the disk image cannot replace itself, and the banner says
so.
```

In *Build from source*, after the `cargo tauri build` block, add:

```markdown
Bundling signs the updater artifacts, so `cargo tauri build` needs the private
key in `TAURI_SIGNING_PRIVATE_KEY` (and its password in
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`). Without a key, build the plain app for
local use with `cargo tauri build --no-bundle`. CI holds the release key.
```

In the status table, add a row after `9 packaging`:

```markdown
| 10 in-app update | built; verified end to end on macOS against a local feed; **first real signed release not yet published** |
```

- [ ] **Step 3: Plan doc**

In `docs/superpowers/plans/2026-09-02-herdup-plan.md`, before `## Manual verification checklist`, add:

```markdown
## Phase 10 — in-app update · M

**Do** — Tauri's updater plugin driven from two Rust commands; a banner and a
*Check for updates* link; `update_endpoint` setting; signed updater artifacts
and `latest.json` from CI. Design:
[`docs/superpowers/specs/2026-09-03-in-app-update-design.md`](../specs/2026-09-03-in-app-update-design.md).

**Tests** — endpoint resolution and translocation detection as pure functions;
settings round trip. The Tauri layer is verified by QA against a local feed.

**Exit — BUILT 2026-09-03; SEE CHECKBOXES.**

- [ ] macOS end to end against a local feed: banner, install with progress,
      restart into the new version, tamper refused, silent when the feed 404s.
- [ ] Windows: unverified — no Windows machine in this effort.
- [ ] First real signed release published and an installed copy updated from it.
```

In the risk register table, add:

```markdown
| Signing key lost | 10 | No installed copy can ever update again. Key and password live in the owner's password manager; only the public key is in the repo. |
```

In the manual verification checklist, add:

```markdown
- [ ] Update from a published release on each OS: banner, install, restart, new version shown.
```

- [ ] **Step 4: Commit**

```bash
git commit -m "Sign updater artifacts in CI and document in-app update" -- .github/workflows/release.yml README.md docs/superpowers/plans/2026-09-02-herdup-plan.md
```

---

### Task 6: the real public key — waits for the user

**Files:**
- Modify: `app/src-tauri/tauri.conf.json`

Do not start until the coordinator hands over the contents of `~/.tauri/herdup.key.pub`. Never put a throwaway key here.

- [ ] **Step 1: Config**

Replace the `bundle` object and add `plugins` in `app/src-tauri/tauri.conf.json`:

```json
  "bundle": {
    "active": true,
    "targets": ["msi", "nsis", "dmg", "deb", "appimage"],
    "createUpdaterArtifacts": true,
    "macOS": { "signingIdentity": "-" },
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ]
  },
  "plugins": {
    "updater": {
      "pubkey": "PASTE THE ONE-LINE CONTENTS OF herdup.key.pub HERE",
      "endpoints": [
        "https://github.com/jellyfishmobile/herdup/releases/latest/download/latest.json"
      ],
      "windows": { "installMode": "passive" }
    }
  }
```

`signingIdentity: "-"` is ad-hoc signing. Today only the executable is linker-signed and `codesign --verify` fails on the bundle; a replaced bundle must carry a valid signature or Apple silicon may refuse to launch it.

- [ ] **Step 2: Verify the app starts and the check is honest**

Run: `cd app && cargo tauri dev`
Expected: the window opens; nothing appears for the startup check; clicking *Check for updates* shows `could not check: no published release` (the repo is private). Quit.

- [ ] **Step 3: Commit**

```bash
git commit -m "Configure the updater: public key, feed, signed artifacts, ad-hoc macOS signing" -- app/src-tauri/tauri.conf.json
```

---

### Task 7: QA — end to end on this Mac

Needs Tasks 1–5 in the working tree. Does not need Task 6: every build here merges a throwaway key. `S` below is your scratchpad directory. **Back up and restore the user's real settings file**, `~/Library/Application Support/herdup/settings.toml`; it is shared with the herdup they have installed.

Non-HTTPS endpoints are accepted only in debug builds, so every bundle here is built with `--debug`. Release builds refuse them, which is the intended fail-closed behaviour.

- [ ] **Step 1: Throwaway key**

```bash
cd app
cargo tauri signer generate --ci -w "$S/qa.key"
cat "$S/qa.key.pub"
export TAURI_SIGNING_PRIVATE_KEY="$(cat "$S/qa.key")"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
```

- [ ] **Step 2: Merge configs**

Write `$S/qa.conf.json` (fill in the public key):

```json
{
  "bundle": { "createUpdaterArtifacts": true, "macOS": { "signingIdentity": "-" } },
  "plugins": {
    "updater": {
      "pubkey": "<contents of qa.key.pub>",
      "endpoints": ["https://github.com/jellyfishmobile/herdup/releases/latest/download/latest.json"]
    }
  }
}
```

Write `$S/qa-next.conf.json` as the same object plus `"version": "0.1.1"` at the top level.

- [ ] **Step 3: Build both versions**

```bash
cd app
cargo tauri build --debug --config "$S/qa.conf.json"
mkdir -p "$S/v0" && cp -R ../target/debug/bundle/macos/herdup.app "$S/v0/"
cargo tauri build --debug --config "$S/qa-next.conf.json"
mkdir -p "$S/feed"
cp ../target/debug/bundle/macos/herdup.app.tar.gz ../target/debug/bundle/macos/herdup.app.tar.gz.sig "$S/feed/"
plutil -p ../target/debug/bundle/macos/herdup.app/Contents/Info.plist | grep CFBundleShortVersionString   # expect 0.1.1
codesign --verify --deep --strict "$S/v0/herdup.app" && echo "v0 signature valid"
```

- [ ] **Step 4: Feed**

Write `$S/feed/latest.json`:

```json
{
  "version": "0.1.1",
  "notes": "QA build",
  "pub_date": "2026-09-03T00:00:00Z",
  "platforms": {
    "darwin-aarch64": {
      "signature": "<contents of herdup.app.tar.gz.sig>",
      "url": "http://127.0.0.1:8765/herdup.app.tar.gz"
    }
  }
}
```

Serve it in the background: `python3 -m http.server 8765 --bind 127.0.0.1 -d "$S/feed" &`

- [ ] **Step 5: Install the 0.1.0 copy where it can replace itself**

```bash
test -e ~/Applications/herdup.app && echo "STOP: ~/Applications/herdup.app exists" || mkdir -p ~/Applications
cp -R "$S/v0/herdup.app" ~/Applications/herdup.app
cp ~/Library/Application\ Support/herdup/settings.toml "$S/settings.backup.toml" 2>/dev/null || true
printf 'update_endpoint = "http://127.0.0.1:8765/latest.json"\n' >> ~/Library/Application\ Support/herdup/settings.toml
open ~/Applications/herdup.app
```

- [ ] **Step 6: Observe the banner and install**

Within about five seconds take a screenshot and read it: `screencapture -x "$S/banner.png"`. Expected: the banner reads *herdup 0.1.1 is available*.

Click *Install and restart*. Try accessibility scripting first:

```bash
osascript -e 'tell application "System Events" to tell process "herdup" to click (first button of entire contents of window 1 whose name is "Install and restart")'
```

If that errors, ask the coordinator to have the user click it; do not work around it. Expected: progress text, then the app quits and reopens. Verify:

```bash
plutil -p ~/Applications/herdup.app/Contents/Info.plist | grep CFBundleShortVersionString   # 0.1.1
ps -axo command | grep -c '[h]erdup.app/Contents/MacOS/herdup-app'                          # at least 1
```

Click *Check for updates* (same osascript pattern, name `Check for updates`) and screenshot: expected *up to date (0.1.1)*.

- [ ] **Step 7: Tamper**

```bash
pkill -f 'Applications/herdup.app/Contents/MacOS/herdup-app'
rm -rf ~/Applications/herdup.app && cp -R "$S/v0/herdup.app" ~/Applications/herdup.app
printf '\x00' | dd of="$S/feed/herdup.app.tar.gz" bs=1 seek=100 count=1 conv=notrunc
open ~/Applications/herdup.app
```

Click *Install and restart*. Expected: the banner reads *the download did not verify; nothing was installed*, both buttons return, and the running app is still 0.1.0.

- [ ] **Step 8: Real feed is silent**

Restore the settings file from the backup (or delete the appended line), relaunch, wait ten seconds, screenshot: no banner. Click *Check for updates*: expected *could not check: no published release* (or *network unreachable* if offline — say which).

- [ ] **Step 9: Optional — translocation text**

`xattr -w com.apple.quarantine "0083;00000000;Safari;" "$S/v0/herdup.app"` then open it from there with `open`. Expected: the banner reads *Move herdup to Applications, then relaunch to install updates* and has no Install button.

- [ ] **Step 10: Clean up and report**

```bash
pkill -f 'Applications/herdup.app/Contents/MacOS/herdup-app'; kill %1   # the http.server
rm -rf ~/Applications/herdup.app
```

Confirm the user's `/Applications/herdup.app` and their settings file are untouched. Report every step with the command, what the screenshot showed, and what was not verified.

---

## Self-review

**Spec coverage.** Startup delay and silence: Task 4 step 2. Ten-second timeout: Task 3. Banner, Not now, manual check states: Task 4. Install with progress, verify, restart: Task 3 + 4. Translocation: Tasks 2, 3, 4. Windows passive mode, updater artifacts, ad-hoc signing, public key, endpoint: Task 6. Settings override: Task 1. CI secrets and notes: Task 5. Key custody: Task 6 preamble and the spec. Error table: `describe_update_error`. Testing: Tasks 1–2 unit, Task 7 QA. Not-verified list: Task 5 plan doc.

**Type consistency.** `UpdateCheckDto { current_version, update }` ↔ `UpdateCheck`; `UpdateDto { version, notes, translocated }` ↔ `UpdateInfo`; `UpdateProgressDto { downloaded, total, installing }` ↔ `UpdateProgress`; event name `update-progress` in both; command names `check_for_update` / `install_update` in both.

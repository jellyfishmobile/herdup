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
    let s = Settings {
        update_endpoint: Some("http://127.0.0.1:8765/latest.json".into()),
        ..Settings::default()
    };
    s.save_to(&dir).expect("saved");
    let back = Settings::load_from(Some(&dir));
    assert_eq!(
        back.update_endpoint.as_deref(),
        Some("http://127.0.0.1:8765/latest.json")
    );
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
    assert!(!is_translocated(Path::new(
        r"C:\Program Files\herdup\herdup-app.exe"
    )));
}

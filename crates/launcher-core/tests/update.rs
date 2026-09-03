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

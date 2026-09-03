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

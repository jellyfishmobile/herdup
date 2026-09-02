//! Guards against console windows flashing on Windows.
//!
//! herdup shells out constantly — every herdr call, every `git`/`gh`/`where`
//! probe, and a poll loop that fires twice every two seconds. A GUI process
//! spawning a console program on Windows flashes a console window each time,
//! which made the app strobe black rectangles while it worked.
//!
//! The fix is `CREATE_NO_WINDOW` on every spawn, applied in one place. This test
//! exists because that is exactly the kind of thing a later change silently
//! reintroduces: a new `Command::new` looks perfectly correct in review.

use std::path::Path;

/// Files allowed to construct a `Command` directly, and why.
const ALLOWED: &[(&str, &str)] = &[
    ("proc.rs", "defines the hidden-command helper itself"),
    (
        "terminal.rs",
        "the terminal handoff must show a window — that is its whole purpose",
    ),
    (
        "mod.rs",
        "start_server uses DETACHED_PROCESS, which already creates no console",
    ),
];

fn source_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read src") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            source_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn every_spawn_goes_through_the_hidden_command_helper() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    source_files(&src, &mut files);
    assert!(files.len() > 5, "expected to find the source tree");

    let mut offenders = Vec::new();
    for file in &files {
        let name = file.file_name().unwrap().to_string_lossy().into_owned();
        if ALLOWED.iter().any(|(allowed, _)| *allowed == name) {
            continue;
        }
        let text = std::fs::read_to_string(file).expect("read source");
        for (n, line) in text.lines().enumerate() {
            if line.contains("Command::new") {
                offenders.push(format!("{}:{}: {}", name, n + 1, line.trim()));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "these spawn a process without CREATE_NO_WINDOW, which flashes a console \
         window on Windows. Use `crate::proc::hidden_command` instead:\n  {}",
        offenders.join("\n  ")
    );
}

#[test]
fn the_helper_actually_sets_the_flag_on_windows() {
    // Cheap but real: if the constant were wrong or the cfg dropped, the source
    // would no longer name it.
    let helper = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/proc.rs"))
        .expect("read proc.rs");
    assert!(
        helper.contains("0x0800_0000"),
        "CREATE_NO_WINDOW value missing"
    );
    assert!(helper.contains("creation_flags"), "flag never applied");
    assert!(helper.contains("cfg(windows)"), "not gated to Windows");
}

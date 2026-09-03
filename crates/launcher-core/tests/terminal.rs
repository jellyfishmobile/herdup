//! Terminal handoff command construction.
//!
//! Both platforms' shapes are tested from either platform: argv building is
//! pure, and only the actual spawn is platform-bound.

use launcher_core::terminal::{handoff, Style};
use std::path::{Path, PathBuf};

fn spacey() -> PathBuf {
    PathBuf::from("C:\\Users\\me\\My Projects\\a b")
}

#[test]
fn windows_terminal_passes_the_project_as_its_own_argument() {
    let h = handoff(
        Path::new("D:\\work\\herdup"),
        Some("herdup"),
        Style::WindowsTerminal,
        None,
    );
    assert_eq!(h.program, "wt.exe");
    assert_eq!(
        h.args,
        vec!["-d", "D:\\work\\herdup", "herdr", "--session", "herdup"]
    );
    assert!(h.script.is_none());
}

#[test]
fn a_windows_path_with_spaces_stays_one_argument() {
    // No shell is involved, so a space in a path needs no quoting — but it must
    // not be split either.
    let h = handoff(&spacey(), None, Style::WindowsTerminal, None);
    assert_eq!(h.args[1], "C:\\Users\\me\\My Projects\\a b");
    assert_eq!(h.args.len(), 3, "no stray arguments from the space");
    assert_eq!(h.args[2], "herdr");
}

#[test]
fn the_powershell_fallback_sets_the_directory_rather_than_typing_cd() {
    // Typing `cd <path>` into a shell would reintroduce quoting problems.
    let h = handoff(&spacey(), Some("herdup"), Style::WindowsPowerShell, None);
    assert_eq!(h.program, "powershell.exe");
    assert_eq!(
        h.args,
        vec!["-NoExit", "-Command", "herdr", "--session", "herdup"]
    );
    assert_eq!(h.cwd.as_deref(), Some(spacey().as_path()));
    assert!(!h.args.iter().any(|a| a.contains("cd ")));
}

#[test]
fn no_session_means_no_session_flag() {
    let h = handoff(Path::new("/tmp/p"), None, Style::WindowsTerminal, None);
    assert_eq!(h.args, vec!["-d", "/tmp/p", "herdr"]);
}

#[test]
fn macos_writes_a_launcher_script_and_opens_it() {
    let h = handoff(
        Path::new("/Users/me/work/herdup"),
        Some("herdup"),
        Style::MacOpen,
        None,
    );
    assert_eq!(h.program, "open");
    assert_eq!(h.args, vec!["-a", "Terminal"]);

    let script = h.script.expect("script generated");
    assert!(script.starts_with("#!/bin/sh"));
    assert!(script.contains("cd '/Users/me/work/herdup'"));
    assert!(script.contains("exec herdr --session 'herdup'"));
}

#[test]
fn a_macos_path_with_a_quote_is_escaped_exactly_once() {
    // Single-quote escaping is the only rule this module needs, which is why
    // the script exists instead of nesting shell quoting inside AppleScript.
    let h = handoff(
        Path::new("/Users/me/Bob's Projects/app"),
        None,
        Style::MacOpen,
        None,
    );
    let script = h.script.expect("script");
    assert!(
        script.contains(r"cd '/Users/me/Bob'\''s Projects/app'"),
        "unexpected escaping:\n{script}"
    );
}

#[test]
fn a_macos_path_with_spaces_is_quoted() {
    let h = handoff(
        Path::new("/Users/me/My Work/app"),
        None,
        Style::MacOpen,
        None,
    );
    assert!(h.script.unwrap().contains("cd '/Users/me/My Work/app'"));
}

#[test]
fn the_terminal_override_replaces_the_program_or_app() {
    let win = handoff(
        Path::new("D:\\p"),
        None,
        Style::WindowsTerminal,
        Some("mywt.exe"),
    );
    assert_eq!(win.program, "mywt.exe");
    assert_eq!(win.args[0], "-d", "override keeps the platform's shape");

    let mac = handoff(Path::new("/p"), None, Style::MacOpen, Some("iTerm"));
    assert_eq!(mac.program, "open");
    assert_eq!(mac.args, vec!["-a", "iTerm"]);
}

#[test]
fn display_gives_something_a_user_can_paste() {
    let h = handoff(
        Path::new("D:\\work\\herdup"),
        Some("herdup"),
        Style::WindowsTerminal,
        None,
    );
    assert_eq!(
        h.display(),
        "wt.exe -d D:\\work\\herdup herdr --session herdup"
    );
}

#[test]
fn the_platform_default_matches_the_host() {
    let style = Style::platform_default();
    if cfg!(windows) {
        assert_eq!(style, Style::WindowsTerminal);
    } else {
        assert_eq!(style, Style::MacOpen);
    }
}

// ---------------------------------------------------------------------------
// Linux
//
// Untested on a real desktop: CI builds the artifact but nobody has run it.
// These pin the command shape so the guess is at least a documented one.
// ---------------------------------------------------------------------------

#[test]
fn linux_passes_a_script_to_the_terminal_alternative() {
    let h = handoff(
        Path::new("/home/dev/my app"),
        Some("herdup"),
        Style::LinuxTerminal,
        None,
    );
    assert_eq!(h.program, "x-terminal-emulator");
    assert_eq!(h.args, vec!["-e"]);
    assert_eq!(h.cwd.as_deref(), Some(Path::new("/home/dev/my app")));
    let script = h.script.expect("linux uses a launcher script");
    assert!(script.contains("'/home/dev/my app'"), "{script}");
    assert!(script.contains("exec herdr --session 'herdup'"), "{script}");
}

#[test]
fn a_linux_terminal_override_replaces_the_program() {
    let h = handoff(
        Path::new("/tmp/p"),
        None,
        Style::LinuxTerminal,
        Some("kitty"),
    );
    assert_eq!(h.program, "kitty");
}

// ---------------------------------------------------------------------------
// Reaping
//
// `open` returns as soon as the terminal is up, and dropping its Child unreaped
// left a zombie under the app after every handoff on macOS. The same helper
// takes the herdr server, which is meant to outlive the call that starts it.
// ---------------------------------------------------------------------------

#[cfg(unix)]
mod reaping {
    use launcher_core::terminal::reap_in_background;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    fn spawn(program: &str, args: &[&str]) -> Child {
        Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn")
    }

    /// `ps` prints nothing for a reaped pid and `Z…` for a zombie. Reaping may
    /// happen off the calling thread, so allow it a moment.
    fn assert_reaped(pid: u32) {
        let start = Instant::now();
        let mut stat = String::new();
        while start.elapsed() < Duration::from_secs(3) {
            let out = Command::new("ps")
                .args(["-o", "stat=", "-p", &pid.to_string()])
                .output()
                .expect("ps");
            stat = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if stat.is_empty() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("pid {pid} still exists three seconds on; ps stat {stat:?}");
    }

    #[test]
    fn a_child_that_exits_at_once_leaves_no_zombie() {
        let child = spawn("true", &[]);
        let pid = child.id();
        reap_in_background(child, Duration::from_millis(500));
        assert_reaped(pid);
    }

    #[test]
    fn a_child_that_outlives_the_grace_leaves_no_zombie() {
        let child = spawn("sleep", &["0.2"]);
        let pid = child.id();
        reap_in_background(child, Duration::ZERO);
        assert_reaped(pid);
    }
}

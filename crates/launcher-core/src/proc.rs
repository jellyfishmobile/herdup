//! Spawning helpers.
//!
//! herdup shells out constantly — every herdr call, every `git`/`gh`/`where`
//! probe, and a poll loop that runs twice every two seconds. On Windows a GUI
//! process spawning a console program flashes a console window each time, so
//! without care the app strobes black rectangles across the screen while it
//! works.
//!
//! Every spawn therefore goes through [`hidden_command`], and a test enforces
//! that nothing constructs a bare `Command` behind its back.

use std::ffi::OsStr;
use std::io::{ErrorKind, Read};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Windows: run the child without creating a console window.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A command that never shows a console window, with stdin closed.
///
/// stdin is null because nothing herdup spawns should ever wait for input: a
/// child blocking on a prompt herdup cannot answer would hang the caller with
/// nothing on screen to explain it.
pub(crate) fn hidden_command(program: impl AsRef<OsStr>) -> Command {
    let mut cmd = Command::new(program);
    cmd.stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// How long the login-shell PATH probe may take, start to finish.
///
/// It runs before the first window opens, so every millisecond of it is a
/// blank screen. An interactive login zsh starts in well under 100 ms on an
/// idle machine; a heavy one (oh-my-zsh with plugins, nvm, conda, brew
/// shellenv) takes one to two seconds on a cold cache. Five seconds covers
/// those with room for a loaded machine, and caps the blank screen when an rc
/// file is genuinely stuck.
const LOGIN_SHELL_DEADLINE: Duration = Duration::from_secs(5);

/// The most output the probe reads before giving up.
///
/// The PATH is a few kilobytes at most. An rc file that streams output (a
/// runaway `yes`, a log tail) must not become unbounded memory in a GUI
/// process.
const LOGIN_SHELL_OUTPUT_CAP: usize = 1 << 20;

/// The PATH the user's login shell would have.
///
/// A macOS app launched from Finder or the Dock inherits launchd's PATH — the
/// four system directories and nothing else — so `which herdr` fails inside the
/// GUI even though it works in every terminal the user has ever opened. Ask the
/// user's shell for the PATH it builds (`-ilc`: login *and* interactive, because
/// tools tell people to edit `.zshrc` as often as `.zprofile`) and use that.
///
/// The shell prints the PATH between unique markers and only the text between
/// them counts, so rc-file banners (with or without a trailing newline),
/// `.zlogout` output and `clear` sequences can neither displace nor corrupt it.
/// Its stdout is read until the end marker arrives, never until EOF: an rc file
/// that starts a background job (`sleep 30 &`, `nohup ... &`) leaves that job
/// holding the pipe after the shell has exited, and waiting for EOF would hang
/// the app for as long as the job runs. The shell is killed as soon as the PATH
/// is in — its login setup is finished by then, and a slow `.zlogout` is not
/// worth a blank screen — and a shell that never prints it is killed at
/// [`LOGIN_SHELL_DEADLINE`], which bounds the whole probe.
///
/// Returns `None` when the shell cannot be run, does not print the markers in
/// time, or prints something between them that is not a PATH; the caller then
/// keeps the PATH it already has. `csh` and `tcsh` are explicitly unsupported:
/// they cannot take `-ilc` (for them `-l` must be the only flag), so they
/// return `None` without being run. Bourne-style shells (zsh, bash, anything
/// else that accepts `-ilc`) work; fish prints its PATH space-separated and is
/// rejected by the format check.
pub fn login_shell_path() -> Option<String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    login_shell_path_of(&shell, &[], LOGIN_SHELL_DEADLINE)
}

/// [`login_shell_path`] for a given shell, extra environment and deadline.
///
/// Scoped so tests can point a shell at a scratch `ZDOTDIR` without touching
/// the process environment, the same way `HerdrCli::with_env` does.
fn login_shell_path_of(shell: &str, env: &[(&str, &OsStr)], deadline: Duration) -> Option<String> {
    if is_csh_family(shell) {
        return None;
    }
    let markers = Markers::fresh();
    let mut cmd = hidden_command(shell);
    cmd.args(["-ilc", &markers.script()])
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for (key, value) in env {
        cmd.env(key, value);
    }
    let mut child = cmd.spawn().ok()?;
    let deadline = Instant::now() + deadline;

    // The reader owns the pipe. It stops at the end marker and drops the pipe
    // then, whatever else may still hold the write end. If it is still blocked
    // at the deadline, it is left parked: it will drop the pipe on EOF, which
    // arrives once whatever holds the write end exits.
    let text = child.stdout.take().and_then(|stdout| {
        let end = markers.end.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(read_until_marker(stdout, end.as_bytes(), deadline));
        });
        rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .ok()
    });
    let Some(text) = text else {
        // Nothing back by the deadline: a shell stuck in an rc file. The
        // deadline is spent anyway, and SIGKILL makes the wait immediate.
        let _ = child.kill();
        let _ = child.wait();
        return None;
    };
    kill_and_reap_detached(child);
    path_between_markers(&text, &markers).map(str::to_string)
}

/// `csh` and `tcsh` reject `-ilc` (`-l` must be their only flag), so a
/// `$SHELL` naming one of them is refused rather than run.
fn is_csh_family(shell: &str) -> bool {
    matches!(
        Path::new(shell).file_name().and_then(OsStr::to_str),
        Some("csh" | "tcsh")
    )
}

/// Read `stdout` until `end` has been seen, EOF, the deadline, or the cap.
///
/// Returns whatever was read and leaves the parsing to the caller. Stopping at
/// the marker is what lets the pipe be closed while a background job still
/// holds its write end; stopping at the deadline is what keeps a runaway rc
/// file from filling memory after the caller has given up.
fn read_until_marker(mut stdout: impl Read, end: &[u8], deadline: Instant) -> String {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match stdout.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
        if contains(&buf, end) || buf.len() > LOGIN_SHELL_OUTPUT_CAP || Instant::now() >= deadline {
            break;
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// Kill the shell and reap it off the calling thread.
///
/// Used once the reader has sent something back, which means the login setup
/// is finished: the markers print after every rc file has run, and EOF means
/// the shell has already exited. Whatever it may still be doing — `.zlogout`,
/// a slow logout hook — is not worth a blank screen, so skipping it is the
/// accepted cost. The wait runs on its own thread so a shell that is slow to
/// die never delays the caller, and no zombie is left behind. Only the shell is
/// killed: a job it left behind is the user's, and whatever it holds open is
/// released when it exits.
fn kill_and_reap_detached(mut child: Child) {
    let _ = child.kill();
    std::thread::spawn(move || {
        let _ = child.wait();
    });
}

/// Unique markers bracketing the PATH in the shell's output.
struct Markers {
    begin: String,
    end: String,
}

impl Markers {
    /// Markers no rc file could print by accident: unique to this process and
    /// this call.
    fn fresh() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        Self::with_nonce(&format!("{:x}-{nanos:x}", std::process::id()))
    }

    /// Markers built from a caller-chosen nonce; tests use a fixed one.
    fn with_nonce(nonce: &str) -> Self {
        Self {
            begin: format!("<herdup-path-{nonce}>"),
            end: format!("</herdup-path-{nonce}>"),
        }
    }

    /// The command the shell runs: the PATH between the markers, nothing else.
    ///
    /// No newline is needed on either side — the parser does not think in
    /// lines — and the markers contain nothing a shell or `printf` treats
    /// specially inside single quotes.
    fn script(&self) -> String {
        format!("printf '%s%s%s' '{}' \"$PATH\" '{}'", self.begin, self.end)
    }
}

/// The text between the markers, if it looks like a PATH.
///
/// Everything outside the markers is ignored: rc banners before them, a banner
/// without a trailing newline that would otherwise be glued onto the front,
/// and `.zlogout` output (a `clear`, an `echo`) after them. "Looks like a
/// PATH" means it starts at a root and has at least one separator — a shell
/// that prints its list some other way (fish joins with spaces) is rejected
/// rather than installed as a PATH nothing can resolve.
fn path_between_markers<'a>(text: &'a str, markers: &Markers) -> Option<&'a str> {
    let after_begin = &text[text.find(&markers.begin)? + markers.begin.len()..];
    let path = &after_begin[..after_begin.find(&markers.end)?];
    (path.starts_with('/') && path.contains(':')).then_some(path)
}

#[cfg(test)]
mod tests {
    use super::{path_between_markers, Markers};

    const PATH: &str = "/Users/me/.local/bin:/opt/homebrew/bin:/usr/bin";

    fn markers() -> Markers {
        Markers::with_nonce("test")
    }

    fn wrapped(path: &str) -> String {
        let m = markers();
        format!("{}{path}{}", m.begin, m.end)
    }

    #[test]
    fn the_script_prints_only_the_path_between_the_markers() {
        assert_eq!(
            markers().script(),
            "printf '%s%s%s' '<herdup-path-test>' \"$PATH\" '</herdup-path-test>'"
        );
    }

    #[test]
    fn takes_the_path_between_the_markers_so_rc_banners_are_skipped() {
        let out = format!("Welcome back!\n{}\n\n", wrapped(PATH));
        assert_eq!(path_between_markers(&out, &markers()), Some(PATH));
    }

    #[test]
    fn logout_output_after_the_end_marker_is_ignored() {
        // QA shape 2: `clear` in .zlogout, or a zshexit hook that echoes. The
        // old last-line parser saw the escape sequence and returned None.
        let cleared = format!("{}\x1b[3J\x1b[H\x1b[2J", wrapped(PATH));
        assert_eq!(path_between_markers(&cleared, &markers()), Some(PATH));
        let echoed = format!("{}\nbye\n", wrapped(PATH));
        assert_eq!(path_between_markers(&echoed, &markers()), Some(PATH));
    }

    #[test]
    fn a_banner_without_a_trailing_newline_does_not_join_the_path() {
        // QA shape 3: `printf 'Loading nvm...'` in .zshrc glued itself onto the
        // PATH line, which then no longer started with `/`.
        let out = format!("Loading nvm...{}", wrapped(PATH));
        assert_eq!(path_between_markers(&out, &markers()), Some(PATH));
    }

    #[test]
    fn rejects_output_that_is_not_a_path() {
        let m = markers();
        assert_eq!(path_between_markers("", &m), None);
        assert_eq!(path_between_markers("zsh: command not found\n", &m), None);
        // Markers present, but fish's space-joined list between them.
        assert_eq!(
            path_between_markers(&wrapped("/usr/bin /bin /usr/sbin"), &m),
            None
        );
        // A PATH with no markers around it is not trusted either.
        assert_eq!(path_between_markers(PATH, &m), None);
        // The shell died (or was killed) before the end marker.
        let cut = format!("{}{PATH}", m.begin);
        assert_eq!(path_between_markers(&cut, &m), None);
    }

    #[test]
    fn a_fresh_nonce_is_never_reused() {
        assert_ne!(Markers::fresh().begin, Markers::fresh().begin);
    }

    #[test]
    fn csh_family_is_refused_without_being_run() {
        use super::{is_csh_family, login_shell_path_of};
        use std::time::Duration;
        assert!(is_csh_family("/bin/tcsh"));
        assert!(is_csh_family("/bin/csh"));
        assert!(!is_csh_family("/bin/zsh"));
        assert!(!is_csh_family("/usr/local/bin/bash"));
        // Runs instantly whether or not tcsh is installed.
        assert_eq!(
            login_shell_path_of("/bin/tcsh", &[], Duration::from_secs(5)),
            None
        );
    }

    /// Against a real zsh, with the rc files QA used to break the probe.
    #[cfg(unix)]
    mod real_zsh {
        use super::super::{hidden_command, login_shell_path_of, LOGIN_SHELL_DEADLINE};
        use std::path::{Path, PathBuf};
        use std::process::Stdio;
        use std::time::{Duration, Instant};

        const ZSH: &str = "/bin/zsh";

        /// A scratch `ZDOTDIR`. Any `sleep` its rc file recorded in `sleep.pid`
        /// is killed on drop — even when an assertion fails — so no test leaves
        /// a stray process behind.
        struct Scratch {
            dir: PathBuf,
        }

        impl Scratch {
            fn new(name: &str) -> Self {
                let dir =
                    std::env::temp_dir().join(format!("herdup-proc-{}-{name}", std::process::id()));
                std::fs::create_dir_all(&dir).expect("temp dir");
                Self { dir }
            }

            fn write(&self, file: &str, text: &str) -> &Self {
                std::fs::write(self.dir.join(file), text).expect("write rc file");
                self
            }

            /// An rc line that starts `sleep 30` in the background and records
            /// its pid for cleanup.
            fn background_sleep(&self) -> String {
                format!(
                    "sleep 30 &\necho $! > '{}'\n",
                    self.dir.join("sleep.pid").display()
                )
            }

            /// An rc line that records the shell's own pid.
            fn record_shell_pid(&self) -> String {
                format!("echo $$ > '{}'\n", self.dir.join("shell.pid").display())
            }

            /// The recorded shell is neither running nor a zombie: it was
            /// killed and reaped. Reaping happens off the calling thread, so
            /// allow it a moment.
            fn assert_shell_gone(&self) {
                let pid = std::fs::read_to_string(self.dir.join("shell.pid"))
                    .expect("the rc file recorded the shell's pid");
                let pid = pid.trim();
                let start = Instant::now();
                while start.elapsed() < Duration::from_secs(2) {
                    let exists = hidden_command("kill")
                        .args(["-0", pid])
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status()
                        .is_ok_and(|status| status.success());
                    if !exists {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                panic!("shell {pid} still exists two seconds after the probe returned");
            }

            fn env(&self) -> [(&'static str, &std::ffi::OsStr); 1] {
                [("ZDOTDIR", self.dir.as_os_str())]
            }
        }

        impl Drop for Scratch {
            fn drop(&mut self) {
                if let Ok(pid) = std::fs::read_to_string(self.dir.join("sleep.pid")) {
                    let _ = hidden_command("kill")
                        .arg(pid.trim())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status();
                }
                let _ = std::fs::remove_dir_all(&self.dir);
            }
        }

        fn zsh_present() -> bool {
            let present = Path::new(ZSH).exists();
            if !present {
                eprintln!("skipping: {ZSH} is not installed");
            }
            present
        }

        fn probe(scratch: &Scratch, deadline: Duration) -> (Option<String>, Duration) {
            let start = Instant::now();
            let path = login_shell_path_of(ZSH, &scratch.env(), deadline);
            (path, start.elapsed())
        }

        #[test]
        fn returns_the_path_while_a_background_job_holds_the_pipe() {
            if !zsh_present() {
                return;
            }
            // QA shape 1: `sleep 30 &` in .zshrc. The job inherits stdout and
            // keeps it open after zsh exits, so waiting for EOF hung the app for
            // the full 30 seconds before any window opened.
            let scratch = Scratch::new("bg");
            scratch.write(".zshrc", &scratch.background_sleep());
            let (path, elapsed) = probe(&scratch, LOGIN_SHELL_DEADLINE);
            let path = path.expect("the marker arrives long before the job matters");
            assert!(
                path.starts_with('/') && path.contains(':'),
                "not a PATH: {path}"
            );
            assert!(
                elapsed < LOGIN_SHELL_DEADLINE,
                "took {elapsed:?}: waited for the background job"
            );
        }

        #[test]
        fn logout_output_does_not_displace_the_path() {
            if !zsh_present() {
                return;
            }
            let scratch = Scratch::new("zlogout");
            scratch
                .write(".zshrc", "")
                .write(".zlogout", "clear\necho bye\n");
            let (path, _) = probe(&scratch, LOGIN_SHELL_DEADLINE);
            assert!(
                path.as_deref().is_some_and(|p| p.starts_with('/')),
                "{path:?}"
            );
        }

        #[test]
        fn a_slow_logout_hook_does_not_delay_the_caller() {
            if !zsh_present() {
                return;
            }
            // The PATH is out in tens of milliseconds; zsh then spends two
            // seconds in .zlogout. The caller must not wait for that. `exec` so
            // the sleep *is* the shell process: the kill that frees the caller
            // also ends the sleep, so nothing outlives the test (a plain
            // `sleep 2` would be orphaned by the kill and linger two seconds).
            let scratch = Scratch::new("zlogout-slow");
            scratch
                .write(".zshrc", &scratch.record_shell_pid())
                .write(".zlogout", "exec sleep 2\n");
            let (path, elapsed) = probe(&scratch, LOGIN_SHELL_DEADLINE);
            assert!(
                path.as_deref().is_some_and(|p| p.starts_with('/')),
                "{path:?}"
            );
            assert!(
                elapsed < Duration::from_millis(500),
                "took {elapsed:?}: waited for .zlogout"
            );
            scratch.assert_shell_gone();
        }

        #[test]
        fn a_banner_without_a_trailing_newline_is_harmless() {
            if !zsh_present() {
                return;
            }
            let scratch = Scratch::new("banner");
            scratch.write(".zshrc", "printf 'Loading nvm...'\n");
            let (path, _) = probe(&scratch, LOGIN_SHELL_DEADLINE);
            assert!(
                path.as_deref().is_some_and(|p| p.starts_with('/')),
                "{path:?}"
            );
        }

        #[test]
        fn a_shell_stuck_in_its_rc_file_is_killed_at_the_deadline() {
            if !zsh_present() {
                return;
            }
            // `wait` blocks zsh on the background sleep, so the markers never
            // come. The probe must give up at the deadline, not at 30 seconds.
            let scratch = Scratch::new("stuck");
            scratch.write(".zshrc", &format!("{}wait\n", scratch.background_sleep()));
            let deadline = Duration::from_millis(500);
            let (path, elapsed) = probe(&scratch, deadline);
            assert_eq!(path, None);
            assert!(
                elapsed < Duration::from_secs(3),
                "took {elapsed:?}: the stuck shell was not killed at the deadline"
            );
        }
    }
}

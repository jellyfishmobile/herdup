//! Handing off to a real terminal attached to herdr.
//!
//! The last thing herdup does. Everything up to here happened headlessly
//! against the herdr server; this opens a terminal so the user sees the agents'
//! own terminals, which is herdr's whole premise.
//!
//! Building the command is separated from running it so the argv can be tested
//! on both platforms from either one.

use std::path::{Path, PathBuf};

/// What herdup will spawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handoff {
    pub program: String,
    pub args: Vec<String>,
    /// Working directory for the spawned process.
    pub cwd: Option<PathBuf>,
    /// macOS only: a launcher script written to disk, passed as the final
    /// argument. Keeps path quoting to a single well-defined rule instead of
    /// nesting shell quoting inside AppleScript quoting.
    pub script: Option<String>,
}

impl Handoff {
    /// A copy-pasteable form, for when spawning fails or the user declines.
    pub fn display(&self) -> String {
        let mut parts = vec![self.program.clone()];
        parts.extend(self.args.iter().cloned());
        parts.join(" ")
    }
}

/// Which shape of handoff to build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// `wt.exe -d <project> herdr …`
    WindowsTerminal,
    /// `powershell.exe -NoExit -Command herdr …`, with the cwd set on the process.
    WindowsPowerShell,
    /// `open -a <App> <script>`
    MacOpen,
    /// `x-terminal-emulator -e <script>`, with the cwd set on the process.
    ///
    /// Linux has no single terminal, so this goes through Debian's
    /// `x-terminal-emulator` alternative by default and takes an override for
    /// everything else. Untested on a real desktop — see DESIGN.md.
    LinuxTerminal,
}

impl Style {
    /// The default for the machine herdup is running on.
    pub fn platform_default() -> Style {
        if cfg!(windows) {
            Style::WindowsTerminal
        } else if cfg!(target_os = "macos") {
            Style::MacOpen
        } else {
            Style::LinuxTerminal
        }
    }
}

/// Build the handoff command.
///
/// `terminal_override` replaces the program (Windows) or the application name
/// (macOS) while keeping that platform's shape. On Windows that means an
/// override must accept `-d <dir> <command>` the way Windows Terminal does; if
/// yours does not, leave it unset and launch herdr yourself.
pub fn handoff(
    project: &Path,
    session: Option<&str>,
    style: Style,
    terminal_override: Option<&str>,
) -> Handoff {
    // Every argument is its own argv element, so no path is ever parsed by a
    // shell. Paths with spaces need no special handling.
    let mut herdr_args: Vec<String> = Vec::new();
    if let Some(session) = session {
        herdr_args.push("--session".into());
        herdr_args.push(session.to_string());
    }

    match style {
        Style::WindowsTerminal => {
            let mut args = vec![
                "-d".to_string(),
                project.to_string_lossy().into_owned(),
                "herdr".to_string(),
            ];
            args.extend(herdr_args);
            Handoff {
                program: terminal_override.unwrap_or("wt.exe").to_string(),
                args,
                cwd: None,
                script: None,
            }
        }
        Style::WindowsPowerShell => {
            // PowerShell rejoins everything after -Command, and the working
            // directory is set on the process rather than typed as a `cd`.
            let mut args = vec![
                "-NoExit".to_string(),
                "-Command".to_string(),
                "herdr".to_string(),
            ];
            args.extend(herdr_args);
            Handoff {
                program: terminal_override.unwrap_or("powershell.exe").to_string(),
                args,
                cwd: Some(project.to_path_buf()),
                script: None,
            }
        }
        Style::MacOpen => {
            let app = terminal_override.unwrap_or("Terminal");
            Handoff {
                program: "open".to_string(),
                args: vec!["-a".to_string(), app.to_string()],
                cwd: None,
                script: Some(launcher_script(project, session)),
            }
        }
        Style::LinuxTerminal => {
            // Same trick as macOS: the project path lives in a script file
            // rather than being interpolated through an unknown terminal's own
            // argument parsing. `-e <file>` is the one flag essentially every
            // emulator agrees on.
            Handoff {
                program: terminal_override
                    .unwrap_or("x-terminal-emulator")
                    .to_string(),
                args: vec!["-e".to_string()],
                cwd: Some(project.to_path_buf()),
                script: Some(launcher_script(project, session)),
            }
        }
    }
}

/// The macOS launcher script.
///
/// Terminal.app runs a script file given to `open`, so the project path lives
/// inside a file we write rather than being interpolated through AppleScript
/// *and* the shell. One escaping rule, applied once.
fn launcher_script(project: &Path, session: Option<&str>) -> String {
    let session_args = match session {
        Some(s) => format!(" --session {}", shell_single_quote(s)),
        None => String::new(),
    };
    format!(
        "#!/bin/sh\n\
         # Generated by herdup. Safe to delete.\n\
         cd {} || exit 1\n\
         exec herdr{}\n",
        shell_single_quote(&project.to_string_lossy()),
        session_args
    )
}

/// POSIX single-quoting: wrap in single quotes, and close/escape/reopen for any
/// embedded single quote. The only escaping rule this module needs.
fn shell_single_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// Spawn the handoff, writing the script first when there is one.
pub fn open(handoff: &Handoff) -> std::io::Result<std::process::Child> {
    let mut command = std::process::Command::new(&handoff.program);
    command.args(&handoff.args);

    if let Some(script) = &handoff.script {
        let path = std::env::temp_dir().join(format!("herdup-open-{}.sh", std::process::id()));
        std::fs::write(&path, script)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
        }
        command.arg(&path);
    }

    if let Some(cwd) = &handoff.cwd {
        command.current_dir(cwd);
    }
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    command.spawn()
}

/// Best-effort handoff with the documented Windows fallback.
///
/// Windows Terminal ships with Windows 11 but can be absent or unavailable, and
/// failing to open a terminal must not make a successful launch look failed —
/// the team is already running either way.
pub fn open_with_fallback(
    project: &Path,
    session: Option<&str>,
    terminal_override: Option<&str>,
) -> Result<Handoff, Handoff> {
    let primary = handoff(
        project,
        session,
        Style::platform_default(),
        terminal_override,
    );
    if open(&primary).is_ok() {
        return Ok(primary);
    }
    if cfg!(windows) {
        let fallback = handoff(project, session, Style::WindowsPowerShell, None);
        if open(&fallback).is_ok() {
            return Ok(fallback);
        }
        return Err(fallback);
    }
    Err(primary)
}

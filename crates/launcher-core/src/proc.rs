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
use std::process::{Command, Stdio};

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

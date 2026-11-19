//! The operating-system levers. Every change here can be undone by a normal user account.

use serde::{Deserialize, Serialize};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::*;

/// What a process looked like before Spry touched it, so it can be put back exactly.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
pub struct Saved {
    pub a: i64,
    pub b: i64,
}

/// A command that never flashes a console window on Windows.
pub fn command<S: AsRef<std::ffi::OsStr>>(program: S) -> std::process::Command {
    #[allow(unused_mut)]
    let mut cmd = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd
}

pub fn open_url(url: &str) {
    let _ = if cfg!(windows) {
        command("rundll32").args(["url.dll,FileProtocolHandler", url]).spawn()
    } else if cfg!(target_os = "macos") {
        command("open").arg(url).spawn()
    } else {
        command("xdg-open").arg(url).spawn()
    };
}

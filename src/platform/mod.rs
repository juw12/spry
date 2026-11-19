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

/// Which processor threads eased apps keep off while the machine is under pressure, so the
/// app in front always finds a free one. `cores` lists the usable threads by physical core,
/// best cores first. About a quarter is kept free, in whole cores, and never more than half.
pub fn keep_free(cores: &[Vec<u32>]) -> Vec<u32> {
    let total: usize = cores.iter().map(Vec::len).sum();
    if total < 2 {
        return Vec::new();
    }
    let (want, most) = ((total / 4).max(1), total / 2);
    let mut free = Vec::new();
    for core in cores {
        if free.len() >= want {
            break;
        }
        let room = most - free.len();
        free.extend(core.iter().copied().take(room));
    }
    free
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

/// Starts the program at normal priority, without a console window. Windows hands a low
/// priority on to child processes, and a stand-in for a normal app must not start low.
pub fn at_normal_priority(cmd: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000 | 0x20); // CREATE_NO_WINDOW | NORMAL_PRIORITY_CLASS
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

#[cfg(test)]
mod tests {
    use super::keep_free;

    #[test]
    fn a_quarter_of_the_processor_is_kept_free_in_whole_cores() {
        let pairs = |n: u32| (0..n).map(|c| vec![2 * c, 2 * c + 1]).collect::<Vec<_>>();
        let singles = |n: u32| (0..n).map(|c| vec![c]).collect::<Vec<_>>();
        assert!(keep_free(&singles(1)).is_empty(), "one thread: nothing can be kept free");
        assert_eq!(keep_free(&pairs(1)), [0], "one core with two threads: never more than half");
        assert_eq!(keep_free(&singles(2)), [0]);
        assert_eq!(keep_free(&pairs(2)), [0, 1], "two cores, four threads: one whole core");
        assert_eq!(keep_free(&singles(4)), [0]);
        assert_eq!(keep_free(&pairs(4)), [0, 1]);
        assert_eq!(keep_free(&pairs(8)), [0, 1, 2, 3]);
    }
}

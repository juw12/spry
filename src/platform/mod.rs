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

/// How hard a held process is held.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hold {
    /// The app in front rests: the process runs as it would without Spry. (Linux keeps the idle
    /// disk priority, which a normal user could not raise again.)
    Lent,
    Eased,
    /// The whole machine is under pressure: also off the threads kept free for the app in front.
    Deep,
    /// Saver mode: as `Deep`, and on a few processor threads only (see `few`).
    Saver,
}

pub fn hold(pid: u32, saved: &Saved, how: Hold) -> std::io::Result<()> {
    match how {
        Hold::Lent => lend(pid, saved),
        _ => apply(pid, saved, how),
    }
}

/// Which processor threads eased apps keep off while the machine is under pressure, and all
/// the time in Saver mode, so the
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

/// The processor threads held apps keep to in Saver mode: about a quarter of them, at least
/// one, from the weakest cores, and never one that is kept free. Two busy apps on four threads
/// otherwise fill half the processor however they are held. Empty where nothing is kept free.
pub fn few(cores: &[Vec<u32>]) -> Vec<u32> {
    let free = keep_free(cores);
    if free.is_empty() {
        return Vec::new();
    }
    let total: usize = cores.iter().map(Vec::len).sum();
    let mut rest: Vec<u32> = cores.iter().flatten().copied().filter(|thread| !free.contains(thread)).collect();
    rest.split_off(rest.len().saturating_sub((total / 4).max(1)))
}

/// Wayland itself does not tell a program which window is in front, but three compositors
/// answer that question over their own socket. These read the process id out of each answer.
#[cfg(any(test, target_os = "linux"))]
pub mod wayland {
    use serde_json::Value;

    fn pid(window: &Value) -> Option<u32> {
        window.get("pid")?.as_u64().and_then(|p| u32::try_from(p).ok()).filter(|p| *p > 1)
    }

    /// sway: the answer to GET_TREE; the focused node is somewhere in the tree.
    pub fn sway(tree: &Value) -> Option<u32> {
        fn focused(node: &Value) -> Option<&Value> {
            if node["focused"] == true {
                return Some(node);
            }
            ["nodes", "floating_nodes"].iter().filter_map(|key| node[*key].as_array()).flatten().find_map(focused)
        }
        focused(tree).and_then(pid)
    }

    /// Hyprland: the answer to `j/activewindow` (an empty object when no window has the focus).
    pub fn hyprland(window: &Value) -> Option<u32> {
        pid(window)
    }

    /// niri: the answer to `"FocusedWindow"`.
    pub fn niri(reply: &Value) -> Option<u32> {
        pid(&reply["Ok"]["FocusedWindow"])
    }
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
    use super::{few, keep_free, wayland};
    use serde_json::json;

    #[test]
    fn the_window_in_front_is_read_from_each_compositors_answer() {
        let tree = json!({"type": "root", "focused": false, "nodes": [{"type": "output", "nodes": [
            {"type": "workspace", "focused": false,
             "nodes": [{"type": "con", "focused": false, "pid": 700},
                       {"type": "con", "nodes": [{"type": "con", "focused": true, "pid": 4321}]}],
             "floating_nodes": [{"type": "floating_con", "focused": false, "pid": 900}]}]}]});
        assert_eq!(wayland::sway(&tree), Some(4321));
        let floating = json!({"nodes": [{"nodes": [], "floating_nodes": [{"focused": true, "pid": 900}]}]});
        assert_eq!(wayland::sway(&floating), Some(900));
        let empty_workspace = json!({"nodes": [{"type": "workspace", "focused": true, "nodes": []}]});
        assert_eq!(wayland::sway(&empty_workspace), None, "an empty workspace has the focus: no app in front");

        assert_eq!(wayland::hyprland(&json!({"address": "0x1", "pid": 2222, "class": "foot"})), Some(2222));
        assert_eq!(wayland::hyprland(&json!({})), None);

        assert_eq!(wayland::niri(&json!({"Ok": {"FocusedWindow": {"id": 3, "pid": 3333}}})), Some(3333));
        assert_eq!(wayland::niri(&json!({"Ok": {"FocusedWindow": null}})), None);
        assert_eq!(wayland::niri(&json!({"Err": "unknown request"})), None);
    }

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

    #[test]
    fn saver_mode_keeps_held_apps_to_a_quarter_of_the_processor() {
        let pairs = |n: u32| (0..n).map(|c| vec![2 * c, 2 * c + 1]).collect::<Vec<_>>();
        let singles = |n: u32| (0..n).map(|c| vec![c]).collect::<Vec<_>>();
        assert!(few(&singles(1)).is_empty(), "one thread: there is no choice to make");
        assert_eq!(few(&singles(2)), [1], "two threads: the one that is not kept free");
        assert_eq!(few(&pairs(1)), [1]);
        assert_eq!(few(&singles(3)), [2], "at least one thread, also below four");
        assert_eq!(few(&pairs(2)), [3], "four threads: one, on the weakest core");
        assert_eq!(few(&singles(4)), [3]);
        assert_eq!(few(&pairs(4)), [6, 7], "eight threads: two, a whole core");
        assert_eq!(few(&pairs(8)), [12, 13, 14, 15]);
        for cores in [singles(2), singles(3), pairs(2), singles(6), pairs(4), pairs(8), pairs(32)] {
            let (free, some) = (keep_free(&cores), few(&cores));
            assert!(!some.is_empty() && some.iter().all(|thread| !free.contains(thread)), "never a kept-free thread");
        }
    }
}

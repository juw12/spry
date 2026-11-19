//! macOS and Linux.
//!
//! macOS: the "background" state (the same switch `taskpolicy -b` uses): lowest CPU priority
//! and throttled disk access. Linux: idle disk priority. A normal user can lower a nice value
//! but not raise it again, so nice is not used. Under pressure eased apps keep off part of the
//! processor (which a normal user can undo), and pause-and-resume pulses do the rest.

use super::Saved;
use std::io;

#[cfg(target_os = "macos")]
pub const EASE_TEXT: &str = "macOS background state: lowest CPU priority and throttled disk access";
#[cfg(not(target_os = "macos"))]
pub const EASE_TEXT: &str =
    "idle disk priority; under pressure part of the processor is kept free, plus short pause-and-resume pulses";
pub const CAN_TRIM: bool = false;

pub fn attach_console() {}

fn signal(pid: u32, sig: libc::c_int) -> io::Result<()> {
    if pid <= 1 {
        return Err(io::Error::other("refusing to signal a system process"));
    }
    if unsafe { libc::kill(pid as libc::pid_t, sig) } == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}

pub fn suspend(pid: u32) -> io::Result<()> {
    signal(pid, libc::SIGSTOP)
}

pub fn resume(pid: u32) -> io::Result<()> {
    signal(pid, libc::SIGCONT)
}

/// Only Windows starts child processes low when the parent runs low; nothing to do here.
pub struct NormalPriority;

impl NormalPriority {
    pub fn hold() -> Self {
        NormalPriority
    }
}

/// Only Windows has a cheaper way than the general one, so there is no answer here.
pub fn cpu_total() -> Option<f32> {
    None
}

/// The processor threads this process may use.
pub fn usable_threads() -> usize {
    std::thread::available_parallelism().map_or(2, |n| n.get())
}

pub fn trim(_pid: u32) -> io::Result<()> {
    Err(io::Error::other("not available on this system"))
}

// ---------------------------------------------------------------- macOS

#[cfg(target_os = "macos")]
mod os {
    use super::*;
    use crate::platform::command;

    const PRIO_DARWIN_PROCESS: libc::c_int = 4;
    const PRIO_DARWIN_BG: libc::c_int = 0x1000;

    /// The highest priority a throttled (background-state) task can have.
    const MAXPRI_THROTTLE: i32 = 4;

    /// 1 = in the background state, 0 = not. `getpriority` only answers this for the calling
    /// process, so the task's base priority is read instead: the background state pins it low.
    pub fn read_level(pid: u32) -> Option<i64> {
        let mut info = std::mem::MaybeUninit::<libc::proc_taskinfo>::zeroed();
        let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
        let got = unsafe {
            libc::proc_pidinfo(
                pid as libc::c_int,
                libc::PROC_PIDTASKINFO,
                0,
                info.as_mut_ptr() as *mut libc::c_void,
                size,
            )
        };
        if got != size {
            return None;
        }
        let info = unsafe { info.assume_init() };
        Some((info.pti_priority <= MAXPRI_THROTTLE) as i64)
    }

    pub fn save(pid: u32) -> io::Result<Saved> {
        read_level(pid).map(|a| Saved { a, b: 0 }).ok_or_else(io::Error::last_os_error)
    }

    fn set(pid: u32, value: libc::c_int) -> io::Result<()> {
        if unsafe { libc::setpriority(PRIO_DARWIN_PROCESS, pid as libc::id_t, value) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub fn apply(pid: u32, _saved: &Saved, _deep: bool) -> io::Result<()> {
        set(pid, PRIO_DARWIN_BG)
    }

    pub fn restore(pid: u32, saved: &Saved) -> io::Result<()> {
        if saved.a == 0 { set(pid, 0) } else { Ok(()) }
    }

    pub fn forget(_pid: u32) {}

    /// macOS gives no way to keep another app off a processor core.
    pub fn kept_free_count() -> usize {
        0
    }

    pub fn cpus_open(_pid: u32) -> Option<u32> {
        None
    }

    pub fn squeeze(_threads: usize) -> bool {
        false
    }

    /// The app in front. AppKit only keeps this fresh while a run loop turns, so when called
    /// on the main thread (no tray) the run loop is given one turn first.
    pub fn foreground_pid() -> Option<u32> {
        use objc2_app_kit::NSWorkspace;
        use objc2_foundation::{NSDate, NSRunLoop};
        let pid = objc2::rc::autoreleasepool(|_| {
            if unsafe { libc::pthread_main_np() } != 0 {
                NSRunLoop::currentRunLoop().runUntilDate(&NSDate::now());
            }
            let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
            Some(app.processIdentifier())
        });
        match pid {
            Some(pid) if pid > 0 => Some(pid as u32),
            _ => {
                // The slow way round starts two small programs, so it is asked at most every 3 s.
                use std::sync::Mutex;
                use std::time::{Duration, Instant};
                static LAST: Mutex<Option<(Instant, Option<u32>)>> = Mutex::new(None);
                let mut last = LAST.lock().ok()?;
                match *last {
                    Some((at, pid)) if at.elapsed() < Duration::from_secs(3) => pid,
                    _ => {
                        let pid = lsappinfo_front();
                        *last = Some((Instant::now(), pid));
                        pid
                    }
                }
            }
        }
    }

    fn lsappinfo_front() -> Option<u32> {
        let run = |args: &[&str]| -> Option<String> {
            let out = command("lsappinfo").args(args).output().ok()?;
            Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
        };
        let asn = run(&["front"])?;
        let info = run(&["info", "-only", "pid", &asn])?;
        info.rsplit('=').next()?.trim().parse().ok()
    }
}

// ---------------------------------------------------------------- Linux and other Unix

#[cfg(not(target_os = "macos"))]
mod os {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    const IOPRIO_WHO_PROCESS: libc::c_long = 1;
    const IOPRIO_CLASS_SHIFT: i64 = 13;
    const IOPRIO_IDLE: i64 = 3 << IOPRIO_CLASS_SHIFT;

    #[cfg(target_os = "linux")]
    fn ioprio_get(tid: u32) -> Option<i64> {
        let value = unsafe { libc::syscall(libc::SYS_ioprio_get, IOPRIO_WHO_PROCESS, tid as libc::c_long) };
        (value >= 0).then_some(value as i64)
    }

    #[cfg(target_os = "linux")]
    fn ioprio_set(tid: u32, value: i64) -> bool {
        unsafe {
            libc::syscall(libc::SYS_ioprio_set, IOPRIO_WHO_PROCESS, tid as libc::c_long, value as libc::c_long) == 0
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn ioprio_get(_tid: u32) -> Option<i64> {
        Some(0)
    }

    #[cfg(not(target_os = "linux"))]
    fn ioprio_set(_tid: u32, _value: i64) -> bool {
        true
    }

    /// Disk priority is per thread on Linux, so every thread of the process is set.
    fn threads(pid: u32) -> Vec<u32> {
        let mut ids: Vec<u32> = std::fs::read_dir(format!("/proc/{pid}/task"))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| e.file_name().to_string_lossy().parse().ok())
            .collect();
        if ids.is_empty() {
            ids.push(pid);
        }
        ids
    }

    /// The processor threads a thread may run on, as bits (thread 0 = bit 0).
    /// None on machines with more than 64 of them: those are left alone.
    #[cfg(target_os = "linux")]
    fn cpus_get(tid: u32) -> Option<u64> {
        let mut words = [0u64; 16];
        let bytes = std::mem::size_of_val(&words);
        let got = unsafe { libc::syscall(libc::SYS_sched_getaffinity, tid as libc::c_long, bytes, words.as_mut_ptr()) };
        (got > 0 && words[1..].iter().all(|w| *w == 0) && words[0] != 0).then_some(words[0])
    }

    #[cfg(target_os = "linux")]
    fn cpus_set(tid: u32, mask: u64) -> bool {
        let words = [mask];
        let bytes = std::mem::size_of_val(&words);
        unsafe { libc::syscall(libc::SYS_sched_setaffinity, tid as libc::c_long, bytes, words.as_ptr()) == 0 }
    }

    #[cfg(not(target_os = "linux"))]
    fn cpus_get(_tid: u32) -> Option<u64> {
        None
    }

    #[cfg(not(target_os = "linux"))]
    fn cpus_set(_tid: u32, _mask: u64) -> bool {
        false
    }

    /// The other threads of the same physical core, as bits.
    fn siblings(cpu: u32) -> u64 {
        let path = format!("/sys/devices/system/cpu/cpu{cpu}/topology/thread_siblings_list");
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let mut mask = 0u64;
        for part in text.trim().split(',') {
            let (first, last) = part.split_once('-').unwrap_or((part, part));
            if let (Ok(first), Ok(last)) = (first.parse::<u32>(), last.parse::<u32>()) {
                for c in first..=last.min(63) {
                    mask |= 1 << c;
                }
            }
        }
        mask
    }

    /// The processor threads eased apps keep off under pressure, as bits. 0 = not used here.
    /// Chosen once, among the threads Spry itself may use, strongest cores first.
    fn kept_free() -> u64 {
        static FREE: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
        *FREE.get_or_init(|| {
            let Some(usable) = cpus_get(0) else { return 0 };
            let mut cores: Vec<(u32, Vec<u32>)> = Vec::new();
            let mut seen = 0u64;
            for cpu in (0..64u32).filter(|c| usable & (1 << c) != 0) {
                if seen & (1 << cpu) != 0 {
                    continue;
                }
                let core = (siblings(cpu) & usable) | (1 << cpu);
                seen |= core;
                let strength = std::fs::read_to_string(format!("/sys/devices/system/cpu/cpu{cpu}/cpu_capacity"))
                    .ok()
                    .and_then(|t| t.trim().parse().ok())
                    .unwrap_or(0u32);
                cores.push((strength, (0..64).filter(|c| core & (1 << c) != 0).collect()));
            }
            cores.sort_by_key(|(strength, _)| std::cmp::Reverse(*strength));
            let cores: Vec<Vec<u32>> = cores.into_iter().map(|(_, threads)| threads).collect();
            crate::platform::keep_free(&cores).iter().fold(0, |mask, c| mask | (1 << c))
        })
    }

    pub fn kept_free_count() -> usize {
        kept_free().count_ones() as usize
    }

    /// Limits this program, and what it starts from now on, to its first `threads` processor threads.
    pub fn squeeze(threads: usize) -> bool {
        let Some(now) = cpus_get(0) else { return false };
        let mut keep = 0u64;
        for cpu in (0..64).filter(|c| now & (1 << c) != 0).take(threads) {
            keep |= 1 << cpu;
        }
        cpus_set(0, keep)
    }

    /// How many processor threads the process may use right now.
    pub fn cpus_open(pid: u32) -> Option<u32> {
        cpus_get(pid).map(u64::count_ones)
    }

    /// The disk priority class: 0 = default, 3 = idle.
    pub fn read_level(pid: u32) -> Option<i64> {
        ioprio_get(pid).map(|v| v >> IOPRIO_CLASS_SHIFT)
    }

    /// What one thread had before Spry changed it.
    struct Was {
        tid: u32,
        disk: i64,
        /// The processor threads it could use, and what Spry narrowed that to.
        cpus: Option<(u64, u64)>,
    }

    static BEFORE: Mutex<BTreeMap<u32, Vec<Was>>> = Mutex::new(BTreeMap::new());

    /// `a` is the disk priority, `b` the processor threads the main thread may use (0 = unknown).
    pub fn save(pid: u32) -> io::Result<Saved> {
        let a = ioprio_get(pid).ok_or_else(io::Error::last_os_error)?;
        Ok(Saved { a, b: cpus_get(pid).unwrap_or(0) as i64 })
    }

    /// Disk priority goes to idle. While `deep` (the machine is under pressure) the process
    /// also keeps off the processor threads that are kept free for the app in front.
    pub fn apply(pid: u32, _saved: &Saved, deep: bool) -> io::Result<()> {
        let free = kept_free();
        let mut before = BEFORE.lock().unwrap();
        let known = before.entry(pid).or_default();
        let mut done = 0;
        for tid in threads(pid) {
            let at = match known.iter().position(|w| w.tid == tid) {
                Some(at) => at,
                None => {
                    let Some(disk) = ioprio_get(tid) else { continue };
                    known.push(Was { tid, disk, cpus: None });
                    known.len() - 1
                }
            };
            let was = &mut known[at];
            if ioprio_set(tid, IOPRIO_IDLE) {
                done += 1;
            }
            if deep && free != 0 {
                if was.cpus.is_none()
                    && let Some(now) = cpus_get(tid)
                {
                    let narrow = now & !free;
                    if narrow != 0 && narrow != now && cpus_set(tid, narrow) {
                        was.cpus = Some((now, narrow));
                    }
                }
            } else if let Some((wide, narrow)) = was.cpus.take() {
                // Pressure is over. Only undo what is still as Spry left it.
                if cpus_get(tid) == Some(narrow) {
                    cpus_set(tid, wide);
                }
            }
        }
        if done > 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
    }

    /// Puts back only what is still as Spry set it: a value the app changed itself in the
    /// meantime is the app's own choice and stays.
    pub fn restore(pid: u32, saved: &Saved) -> io::Result<()> {
        let known = BEFORE.lock().unwrap().remove(&pid).unwrap_or_default();
        let (free, main_wide) = (kept_free(), saved.b as u64);
        let mut done = 0;
        for tid in threads(pid) {
            let was = known.iter().find(|w| w.tid == tid);
            // Threads started while eased took over the eased values: they get the main thread's.
            let idle = ioprio_get(tid).map(|v| v >> IOPRIO_CLASS_SHIFT) == Some(IOPRIO_IDLE >> IOPRIO_CLASS_SHIFT);
            if !idle || ioprio_set(tid, was.map_or(saved.a, |w| w.disk)) {
                done += 1;
            }
            let now = cpus_get(tid);
            match was.and_then(|w| w.cpus) {
                Some((wide, narrow)) => {
                    if now == Some(narrow) {
                        cpus_set(tid, wide);
                    }
                }
                // No note for this thread (started while eased, or Spry was killed and restarted).
                None => {
                    let narrow = main_wide & !free;
                    if narrow != main_wide && narrow != 0 && now == Some(narrow) {
                        cpus_set(tid, main_wide);
                    }
                }
            }
        }
        if done > 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
    }

    pub fn forget(pid: u32) {
        BEFORE.lock().unwrap().remove(&pid);
    }

    /// The process behind the focused window, on X11. Wayland does not tell other programs
    /// which window is in front, so this is None there (Spry then works from your rules only).
    #[cfg(target_os = "linux")]
    pub fn foreground_pid() -> Option<u32> {
        use std::sync::Mutex;
        use std::time::{Duration, Instant};
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};
        use x11rb::rust_connection::RustConnection;

        struct X {
            conn: RustConnection,
            root: u32,
            active: u32,
            wm_pid: u32,
        }
        static LINK: Mutex<(Option<X>, Option<Instant>)> = Mutex::new((None, None));

        fn connect() -> Option<X> {
            let (conn, screen) = x11rb::connect(None).ok()?;
            let root = conn.setup().roots.get(screen)?.root;
            let active = conn.intern_atom(false, b"_NET_ACTIVE_WINDOW").ok()?.reply().ok()?.atom;
            let wm_pid = conn.intern_atom(false, b"_NET_WM_PID").ok()?.reply().ok()?.atom;
            Some(X { conn, root, active, wm_pid })
        }

        fn ask(x: &X) -> Result<Option<u32>, ()> {
            let prop = |window, atom, kind: AtomEnum| -> Result<Option<u32>, ()> {
                let reply = x.conn.get_property(false, window, atom, kind, 0, 1).map_err(|_| ())?.reply();
                match reply {
                    Ok(r) => Ok(r.value32().and_then(|mut v| v.next())),
                    // The window can close between the two questions; that is not a lost connection.
                    Err(x11rb::errors::ReplyError::X11Error(_)) => Ok(None),
                    Err(_) => Err(()),
                }
            };
            let Some(window) = prop(x.root, x.active, AtomEnum::WINDOW)? else { return Ok(None) };
            if window == 0 {
                return Ok(None);
            }
            Ok(prop(window, x.wm_pid, AtomEnum::CARDINAL)?.filter(|pid| *pid > 1))
        }

        let mut link = LINK.lock().ok()?;
        if link.0.is_none() {
            // No display (or Wayland only): try again once in a while, not on every tick.
            if link.1.is_some_and(|t| t.elapsed() < Duration::from_secs(30)) {
                return None;
            }
            link.1 = Some(Instant::now());
            link.0 = connect();
        }
        match ask(link.0.as_ref()?) {
            Ok(pid) => pid,
            Err(()) => {
                link.0 = None;
                None
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    pub fn foreground_pid() -> Option<u32> {
        None
    }
}

pub use os::*;

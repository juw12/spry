//! The operating-system levers, tried on a real busy process: every change must be
//! visible, and every change must be put back exactly.

use spry::engine::Pulse;
use spry::platform;
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

struct Burner(Child);

impl Burner {
    fn start() -> Self {
        // At normal priority, also when the test itself was started lower (CI runners are).
        let mut busy = Command::new(env!("CARGO_BIN_EXE_spry"));
        let child = platform::at_normal_priority(&mut busy)
            .args(["burn", "120"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .expect("start a busy process");
        sleep(Duration::from_millis(300));
        Burner(child)
    }

    fn pid(&self) -> u32 {
        self.0.id()
    }

    /// CPU time the process has used so far, in milliseconds.
    fn cpu_ms(&self) -> u64 {
        let mut sys = System::new().expect("read processes");
        let pid = Pid::from_u32(self.pid());
        sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            false,
            ProcessRefreshKind::nothing().with_cpu(),
        );
        sys.process(pid).expect("the busy process is alive").accumulated_cpu_time()
    }

    /// Share of one core used over the next `window`, from 0.0 to about 1.0.
    fn load(&self, window: Duration) -> f64 {
        let (before, start) = (self.cpu_ms(), Instant::now());
        sleep(window);
        (self.cpu_ms() - before) as f64 / start.elapsed().as_millis() as f64
    }
}

impl Drop for Burner {
    fn drop(&mut self) {
        let _ = platform::resume(self.pid());
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn easing_shows_and_is_put_back_exactly() {
    let burner = Burner::start();
    let before = platform::read_level(burner.pid()).expect("read the starting state");
    let saved = platform::save(burner.pid()).expect("save the starting state");

    for deep in [false, true] {
        platform::apply(burner.pid(), &saved, deep).expect("ease the process");
        let eased = platform::read_level(burner.pid()).expect("read the eased state");
        assert_ne!(eased, before, "easing (deep = {deep}) must change the state the system reports");
    }

    platform::restore(burner.pid(), &saved).expect("put the process back");
    assert_eq!(platform::read_level(burner.pid()), Some(before), "the starting state must come back exactly");
    assert!(burner.load(Duration::from_millis(600)) > 0.2, "the process must still be running");
}

#[test]
fn freezing_stops_the_process_and_thawing_starts_it_again() {
    let burner = Burner::start();
    assert!(burner.load(Duration::from_millis(600)) > 0.2, "busy before the freeze");

    platform::suspend(burner.pid()).expect("freeze");
    sleep(Duration::from_millis(150));
    let frozen = burner.load(Duration::from_millis(600));
    assert!(frozen < 0.1, "a frozen process must use no processor time, used {frozen:.2}");

    platform::resume(burner.pid()).expect("thaw");
    assert!(burner.load(Duration::from_millis(600)) > 0.2, "busy again after the thaw");
}

#[test]
fn pulses_hold_a_process_back_and_leave_it_running() {
    let burner = Burner::start();
    let free = burner.load(Duration::from_millis(800));

    let pulse = Pulse::new();
    pulse.set(vec![burner.pid()]);
    sleep(Duration::from_millis(500));
    let pulsed = burner.load(Duration::from_millis(2000));
    assert!(pulsed < free * 0.8, "pulses must cut processor use: {free:.2} before, {pulsed:.2} during");

    drop(pulse);
    assert!(burner.load(Duration::from_millis(600)) > 0.2, "running again once the pulses stop");
}

#[test]
fn asking_for_the_front_app_never_fails() {
    // On a server without a screen this is None; with a desktop it is a real process.
    if let Some(pid) = platform::foreground_pid() {
        assert!(pid > 0);
    }
}

/// Browsers move their own tabs between priorities. If an app changed its priority after
/// Spry eased it, Spry must leave that choice alone, both later and when letting go.
#[cfg(windows)]
#[test]
fn a_priority_the_app_set_itself_is_never_undone() {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        IDLE_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS, OpenProcess, PROCESS_SET_INFORMATION, SetPriorityClass,
    };
    let set_class = |pid: u32, class: u32| unsafe {
        let handle = OpenProcess(PROCESS_SET_INFORMATION, 0, pid);
        assert!(!handle.is_null());
        assert_ne!(SetPriorityClass(handle, class), 0);
        CloseHandle(handle);
    };

    // The app had lowered itself, then raises itself while Spry holds it: it stays raised.
    let burner = Burner::start();
    set_class(burner.pid(), IDLE_PRIORITY_CLASS);
    let saved = platform::save(burner.pid()).unwrap();
    platform::apply(burner.pid(), &saved, false).unwrap();
    set_class(burner.pid(), NORMAL_PRIORITY_CLASS);
    platform::apply(burner.pid(), &saved, true).unwrap();
    assert_eq!(platform::read_level(burner.pid()), Some(NORMAL_PRIORITY_CLASS as i64), "not lowered again");
    platform::restore(burner.pid(), &saved).unwrap();
    assert_eq!(
        platform::read_level(burner.pid()),
        Some(NORMAL_PRIORITY_CLASS as i64),
        "not put back to the old, lower value"
    );

    // The app lowers itself further while Spry holds it: letting go does not raise it.
    let burner = Burner::start();
    let saved = platform::save(burner.pid()).unwrap();
    platform::apply(burner.pid(), &saved, false).unwrap();
    set_class(burner.pid(), IDLE_PRIORITY_CLASS);
    platform::restore(burner.pid(), &saved).unwrap();
    assert_eq!(platform::read_level(burner.pid()), Some(IDLE_PRIORITY_CLASS as i64), "the app's own choice stays");
}

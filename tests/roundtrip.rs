//! The operating-system levers, tried on a real busy process: every change must be
//! visible, and every change must be put back exactly.

use spry::engine::{Pulse, Sampler};
use spry::platform;
use std::collections::HashSet;
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, MutexGuard};
use std::thread::sleep;
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

/// One busy process at a time: side by side they take processor time from each other
/// on a small machine, and the measurements below stop meaning anything.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

struct Burner(Child, #[allow(dead_code)] MutexGuard<'static, ()>);

impl Burner {
    fn start() -> Self {
        let alone = ONE_AT_A_TIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        // At normal priority, also when the test itself was started lower (CI runners are).
        let mut busy = Command::new(env!("CARGO_BIN_EXE_spry"));
        let child = platform::at_normal_priority(&mut busy)
            .args(["burn", "120"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .expect("start a busy process");
        sleep(Duration::from_millis(300));
        Burner(child, alone)
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

/// While the app in front rests, a held process gets back everything that costs speed on a
/// free processor, and the hold can come back and be undone exactly as before.
#[test]
fn lending_gives_the_speed_back_and_the_hold_can_return() {
    let burner = Burner::start();
    let pid = burner.pid();
    let before = platform::read_level(pid).expect("read the starting state");
    let all = platform::cpus_open(pid);
    let kept = platform::kept_free_count() as u32;
    let saved = platform::save(pid).expect("save the starting state");

    for round in 0..2 {
        platform::hold(pid, &saved, platform::Hold::Deep).expect("hold under pressure");
        assert_ne!(platform::read_level(pid), Some(before), "round {round}: the hold must show");
        if let Some(all) = all.filter(|_| kept > 0) {
            assert_eq!(platform::cpus_open(pid), Some(all - kept), "round {round}: kept-free threads are closed");
        }

        platform::hold(pid, &saved, platform::Hold::Lent).expect("lend the idle time");
        assert_eq!(platform::cpus_open(pid), all, "round {round}: lent, every processor thread is open again");
        if cfg!(target_os = "macos") {
            assert_eq!(platform::read_level(pid), Some(before), "round {round}: lent, the background state is lifted");
        }
        assert!(burner.load(Duration::from_millis(600)) > 0.2, "round {round}: lent, the process runs");
    }

    platform::hold(pid, &saved, platform::Hold::Deep).expect("the app in front works again");
    platform::restore(pid, &saved).expect("put the process back");
    assert_eq!(platform::read_level(pid), Some(before), "the starting state must come back exactly");
    assert_eq!(platform::cpus_open(pid), all, "and every processor thread is open");
}

/// The half-second reading that brings the hold back: it must see a process start to work.
#[test]
fn the_fresh_reading_sees_the_front_app_work_and_rest() {
    let burner = Burner::start();
    let front = HashSet::from([burner.pid()]);
    let mut sampler = Sampler::new();
    assert_eq!(sampler.front_now(&front), None, "the first reading has nothing to compare with");
    sleep(Duration::from_millis(500));
    let working = sampler.front_now(&front).expect("a second reading half a second later");
    assert!(working > 20.0, "a busy process must read as working, read {working:.0}% of one core");

    platform::suspend(burner.pid()).expect("freeze");
    sleep(Duration::from_millis(200));
    sampler.front_now(&front);
    sleep(Duration::from_millis(500));
    let resting = sampler.front_now(&front).expect("a reading while it is frozen");
    assert!(resting < 5.0, "a frozen process must read as resting, read {resting:.0}% of one core");

    platform::resume(burner.pid()).expect("thaw");
    sleep(Duration::from_millis(500));
    let again = sampler.front_now(&front).expect("a reading after the thaw");
    assert!(again > 15.0, "working again must show within one reading, read {again:.0}% of one core");
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
    drop(burner);
    let burner = Burner::start();
    let saved = platform::save(burner.pid()).unwrap();
    platform::apply(burner.pid(), &saved, false).unwrap();
    set_class(burner.pid(), IDLE_PRIORITY_CLASS);
    platform::restore(burner.pid(), &saved).unwrap();
    assert_eq!(platform::read_level(burner.pid()), Some(IDLE_PRIORITY_CLASS as i64), "the app's own choice stays");
}

#[test]
fn under_pressure_part_of_the_processor_is_kept_free_and_given_back() {
    let burner = Burner::start();
    let pid = burner.pid();
    let kept = platform::kept_free_count() as u32;
    // macOS has no such lever, and a machine with one processor thread has nothing to keep free.
    let Some(before) = platform::cpus_open(pid) else { return };
    if kept == 0 {
        return;
    }
    let saved = platform::save(pid).expect("save the starting state");

    platform::apply(pid, &saved, false).expect("ease without pressure");
    assert_eq!(platform::cpus_open(pid), Some(before), "without pressure every processor thread stays open");

    platform::apply(pid, &saved, true).expect("ease under pressure");
    assert_eq!(platform::cpus_open(pid), Some(before - kept), "under pressure the kept-free threads are closed");

    platform::apply(pid, &saved, false).expect("pressure is over");
    assert_eq!(platform::cpus_open(pid), Some(before), "when pressure ends they are open again");

    platform::apply(pid, &saved, true).expect("ease under pressure");
    platform::restore(pid, &saved).expect("put the process back");
    assert_eq!(platform::cpus_open(pid), Some(before), "letting go opens every thread again");

    // Spry was killed and started again: nothing is remembered but the saved state on disk.
    platform::apply(pid, &saved, true).expect("ease under pressure");
    platform::forget(pid);
    platform::restore(pid, &saved).expect("put the process back from the saved state alone");
    assert_eq!(platform::cpus_open(pid), Some(before), "the rescue opens every thread again");
    assert!(burner.load(Duration::from_millis(600)) > 0.2, "the process must still be running");
}

/// The test stands in for an app that changes its own settings while Spry holds it back.
#[cfg(target_os = "linux")]
#[test]
fn a_choice_the_app_made_itself_while_eased_is_left_alone() {
    let burner = Burner::start();
    let pid = burner.pid();
    let saved = platform::save(pid).expect("save the starting state");
    platform::apply(pid, &saved, true).expect("ease under pressure");

    let own_disk: libc::c_long = (2 << 13) | 6; // best effort, level 6
    let mut allowed = [0u64; 16];
    let one_thread = unsafe {
        assert_eq!(libc::syscall(libc::SYS_ioprio_set, 1, pid as libc::c_long, own_disk), 0, "set a disk priority");
        let bytes = std::mem::size_of_val(&allowed);
        assert!(libc::syscall(libc::SYS_sched_getaffinity, pid as libc::c_long, bytes, allowed.as_mut_ptr()) > 0);
        let one = [allowed[0] & allowed[0].wrapping_neg()]; // the lowest thread it may still use
        assert_eq!(libc::syscall(libc::SYS_sched_setaffinity, pid as libc::c_long, 8usize, one.as_ptr()), 0);
        platform::kept_free_count() > 0
    };

    platform::restore(pid, &saved).expect("let go of the process");
    let disk = unsafe { libc::syscall(libc::SYS_ioprio_get, 1, pid as libc::c_long) };
    assert_eq!(disk, own_disk, "the disk priority the app chose itself must stay");
    if one_thread {
        assert_eq!(platform::cpus_open(pid), Some(1), "the processor thread the app chose itself must stay");
    }
}

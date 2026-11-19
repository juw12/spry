//! Reads the machine, asks the policy what to do, applies it, and always puts things back.

use crate::config::{self, Config, is_protected, norm};
use crate::platform::{self, Hold, Saved};
use crate::policy::{self, Level, Plan, PolicyState, Proc, Snapshot};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use sysinfo::{Disks, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// How often the front app is checked, and how often the whole machine is measured.
const FAST: Duration = Duration::from_millis(500);
const SLOW: Duration = Duration::from_secs(2);
/// While the machine is calm and nobody is looking at the dashboard, measuring can wait longer.
const CALM: Duration = Duration::from_secs(5);
const CALM_BELOW_CPU: f32 = 35.0;
// When the app in front wakes up, the half-second reading brings the hold back; the full
// reading must confirm it before time can be lent again, also at the calm pace.
const _: () = assert!(policy::LEND_AFTER.as_secs() > CALM.as_secs());
const WATCHED_FOR: Duration = Duration::from_secs(10);
/// One pulse: paused for this long, then running for this long.
pub const PULSE_STOP: Duration = Duration::from_millis(120);
pub const PULSE_RUN: Duration = Duration::from_millis(80);

/// What the engine, the dashboard and the tray share.
pub struct Shared {
    pub cfg: Mutex<Config>,
    pub reference: Mutex<crate::reference::Capture>,
    pub status: Mutex<String>,
    pub quit: AtomicBool,
    pub pause_until: Mutex<Option<Instant>>,
    pub token: String,
    pub held: AtomicUsize,
    pub autostart: AtomicBool,
    pub update: Mutex<Option<String>>,
    /// When the dashboard last asked for the numbers.
    pub watched: Mutex<Option<Instant>>,
    /// True while this Spry can show the page in a window of its own (tray.rs).
    pub window: AtomicBool,
    /// Set by whoever wants that window in front: the tray, a second start, `spry open`.
    pub show: AtomicBool,
}

impl Shared {
    pub fn new(cfg: Config) -> Arc<Self> {
        Arc::new(Shared {
            cfg: Mutex::new(cfg),
            reference: Mutex::new(crate::reference::Capture::load(config::data_dir())),
            status: Mutex::new("{}".into()),
            quit: AtomicBool::new(false),
            pause_until: Mutex::new(None),
            token: config::token(),
            held: AtomicUsize::new(0),
            autostart: AtomicBool::new(crate::install::is_installed()),
            update: Mutex::new(None),
            watched: Mutex::new(None),
            window: AtomicBool::new(false),
            show: AtomicBool::new(false),
        })
    }

    pub fn paused_secs(&self) -> u64 {
        let mut until = self.pause_until.lock().unwrap();
        match *until {
            Some(t) if t > Instant::now() => (t - Instant::now()).as_secs() + 1,
            _ => {
                *until = None;
                0
            }
        }
    }
}

// ---------------------------------------------------------------- measuring

pub struct Sampler {
    sys: System,
    uid: Option<sysinfo::Uid>,
    smooth: HashMap<u32, f32>,
    /// Processor time each process had used at the last reading, in ms.
    spent: HashMap<u32, u64>,
    /// Per process (id, start time): does its program file belong to the system? Looked up once.
    system: HashMap<u32, (u64, bool)>,
    disk: (u64, u64),
    disk_at: Option<Instant>,
    /// A second, small reader for the app in front alone: (processor time used so far per
    /// process in ms, when that was read). Kept apart so it cannot disturb the main readings.
    fast: System,
    fast_seen: Option<(HashMap<u32, u64>, Instant)>,
    pub self_pid: u32,
}

impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}

impl Sampler {
    pub fn new() -> Self {
        let mut sampler = Sampler {
            sys: System::new().expect("the operating system refused to report its processes"),
            uid: None,
            smooth: HashMap::new(),
            spent: HashMap::new(),
            system: HashMap::new(),
            disk: (0, 0),
            disk_at: None,
            fast: System::new().expect("the operating system refused to report its processes"),
            fast_seen: None,
            self_pid: std::process::id(),
        };
        sampler.refresh();
        sampler.uid = sampler.sys.process(sysinfo::Pid::from_u32(sampler.self_pid)).and_then(|p| p.user_id().cloned());
        sampler
    }

    fn refresh(&mut self) {
        let kind =
            ProcessRefreshKind::nothing().with_cpu().with_memory().with_user(UpdateKind::OnlyIfNotSet).without_tasks();
        if !cfg!(windows) {
            self.sys.refresh_cpu_usage();
        }
        self.sys.refresh_memory();
        self.sys.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
    }

    /// (total, free) bytes of the disk that holds the home folder. Read at most twice a minute.
    pub fn disk(&mut self) -> (u64, u64) {
        if self.disk_at.is_none_or(|t| t.elapsed() > Duration::from_secs(30)) {
            let home = config::home();
            if let Ok(disks) = Disks::new_with_refreshed_list() {
                let best = disks
                    .iter()
                    .filter(|d| home.starts_with(d.mount_point()))
                    .max_by_key(|d| d.mount_point().as_os_str().len());
                if let Some(d) = best {
                    self.disk = (d.total_space(), d.available_space());
                }
            }
            self.disk_at = Some(Instant::now());
        }
        self.disk
    }

    /// Finds out where the program file of each new process of this user lives. The path is
    /// read once per process, so a calm machine costs nothing here.
    fn note_system_programs(&mut self) {
        let fresh: Vec<sysinfo::Pid> = self
            .sys
            .processes()
            .iter()
            .filter(|(pid, p)| {
                p.user_id() == self.uid.as_ref()
                    && self.system.get(&pid.as_u32()).is_none_or(|(start, _)| *start != p.start_time())
            })
            .map(|(pid, _)| *pid)
            .collect();
        if !fresh.is_empty() {
            let kind = ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet).without_tasks();
            self.sys.refresh_processes_specifics(ProcessesToUpdate::Some(&fresh), false, kind);
            for pid in fresh {
                if let Some(p) = self.sys.process(pid) {
                    let system = p.exe().is_some_and(config::is_system_path);
                    self.system.insert(pid.as_u32(), (p.start_time(), system));
                }
            }
        }
        let sys = &self.sys;
        self.system.retain(|pid, _| sys.process(sysinfo::Pid::from_u32(*pid)).is_some());
    }

    /// How much processor these processes used since the last call, as a share of one core
    /// in percent. None on the first call and after a gap, when there is nothing to compare with.
    pub fn front_now(&mut self, pids: &HashSet<u32>) -> Option<f32> {
        let list: Vec<sysinfo::Pid> = pids.iter().map(|pid| sysinfo::Pid::from_u32(*pid)).collect();
        let kind = ProcessRefreshKind::nothing().with_cpu().without_tasks();
        self.fast.refresh_processes_specifics(ProcessesToUpdate::Some(&list), true, kind);
        let now = Instant::now();
        let used: HashMap<u32, u64> = list
            .iter()
            .filter_map(|pid| self.fast.process(*pid).map(|p| (pid.as_u32(), p.accumulated_cpu_time())))
            .collect();
        let (before, at) = self.fast_seen.replace((used.clone(), now))?;
        let passed = now.duration_since(at);
        if passed < Duration::from_millis(100) || passed > Duration::from_millis(1500) {
            return None;
        }
        let ms: u64 = used.iter().filter_map(|(pid, t)| before.get(pid).map(|old| t.saturating_sub(*old))).sum();
        Some(ms as f32 * 100.0 / passed.as_millis() as f32)
    }

    pub fn sample(&mut self) -> Snapshot {
        self.refresh();
        if self.uid.is_some() {
            self.note_system_programs();
        }
        let mut procs = Vec::with_capacity(self.sys.processes().len());
        let mut smooth = HashMap::with_capacity(self.smooth.len());
        let mut spent = HashMap::with_capacity(self.spent.len());
        for (pid, p) in self.sys.processes() {
            let pid = pid.as_u32();
            // A process that used no processor time at all since the last reading used 0%.
            // (On macOS the library keeps the last share it worked out for such a process, so
            // an app that stopped completely would look busy for as long as it stays stopped.)
            let used = p.accumulated_cpu_time();
            let now = if self.spent.get(&pid) == Some(&used) { 0.0 } else { p.cpu_usage() };
            spent.insert(pid, used);
            let cpu = self.smooth.get(&pid).map_or(now, |old| 0.5 * old + 0.5 * now);
            smooth.insert(pid, cpu);
            procs.push(Proc {
                pid,
                ppid: p.parent().map(|x| x.as_u32()),
                name: norm(&p.name().to_string_lossy()),
                cpu,
                cpu_now: now,
                mem: p.memory(),
                mine: self.uid.is_some() && p.user_id() == self.uid.as_ref(),
                start: p.start_time(),
                system: self.system.get(&pid).is_some_and(|(_, system)| *system),
            });
        }
        self.smooth = smooth;
        self.spent = spent;
        let total = self.sys.total_memory().max(1);
        Snapshot {
            procs,
            fg: None,
            front_now: None,
            cpu_total: platform::cpu_total().unwrap_or_else(|| self.sys.global_cpu_usage()),
            mem_free_pct: self.sys.available_memory() as f32 * 100.0 / total as f32,
            mem_total: total,
            self_pid: self.self_pid,
        }
    }
}

// ---------------------------------------------------------------- pulses

/// Pauses and resumes a set of processes in a steady rhythm, the way `cpulimit` does.
pub struct Pulse {
    list: Arc<Mutex<Vec<u32>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Default for Pulse {
    fn default() -> Self {
        Self::new()
    }
}

impl Pulse {
    pub fn new() -> Self {
        let list = Arc::new(Mutex::new(Vec::<u32>::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (list2, stop2) = (list.clone(), stop.clone());
        let thread = thread::spawn(move || {
            while !stop2.load(Ordering::Relaxed) {
                let idle = {
                    // The list stays locked for the whole pause, so every process this thread
                    // paused is running again before anyone else may change the list.
                    let pids = list2.lock().unwrap();
                    for pid in pids.iter() {
                        let _ = platform::suspend(*pid);
                    }
                    if !pids.is_empty() {
                        thread::sleep(PULSE_STOP);
                    }
                    for pid in pids.iter() {
                        let _ = platform::resume(*pid);
                    }
                    pids.is_empty()
                };
                if idle {
                    // Sleep until the list changes or Spry quits; an empty list needs no periodic wakeups.
                    thread::park();
                } else {
                    thread::sleep(PULSE_RUN);
                }
            }
        });
        Pulse { list, stop, thread: Some(thread) }
    }

    pub fn set(&self, pids: Vec<u32>) {
        let mut list = self.list.lock().unwrap();
        if *list != pids {
            *list = pids;
            if let Some(t) = &self.thread {
                t.thread().unpark();
            }
        }
    }
}

impl Drop for Pulse {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            t.thread().unpark();
            let _ = t.join();
        }
    }
}

// ---------------------------------------------------------------- applying

pub struct Touched {
    pub name: String,
    pub start: u64,
    pub level: Level,
    pub saved: Saved,
    pub frozen: bool,
    pub hold: Hold,
    pub since: Instant,
}

/// One line of the crash-recovery list on disk.
#[derive(Serialize, Deserialize)]
struct Held {
    pid: u32,
    start: u64,
    saved: Saved,
    stopped: bool,
}

#[derive(Default)]
pub struct Engine {
    pub touched: HashMap<u32, Touched>,
    /// Processes that refused (no permission, or priority raised on purpose): pid -> start time.
    refused: HashMap<u32, u64>,
    pulse: Pulse,
    pub events: u64,
    pub trims: u64,
}

/// Even when the engine stops because of a panic, every process is put back.
impl Drop for Engine {
    fn drop(&mut self) {
        self.restore_all();
    }
}

fn state_file() -> std::path::PathBuf {
    config::data_dir().join("held.json")
}

impl Engine {
    pub fn apply(&mut self, plan: &Plan, snap: &Snapshot) {
        let by_pid: HashMap<u32, &Proc> = snap.procs.iter().map(|p| (p.pid, p)).collect();
        let mut changed = false;

        // Let go of everything that is gone or no longer wanted.
        let held: Vec<u32> = self.touched.keys().copied().collect();
        for pid in held {
            let same = by_pid.get(&pid).is_some_and(|p| p.start == self.touched[&pid].start);
            if !same {
                self.touched.remove(&pid); // ended, nothing to put back
                platform::forget(pid);
                changed = true;
            } else if !plan.levels.contains_key(&pid) {
                self.release(pid);
                changed = true;
            }
        }
        self.refused.retain(|pid, start| by_pid.get(pid).is_some_and(|p| p.start == *start));

        for (&pid, &level) in &plan.levels {
            let Some(p) = by_pid.get(&pid) else { continue };
            if self.refused.contains_key(&pid) {
                continue;
            }
            let hold = match level {
                Level::Lent => Hold::Lent,
                _ if plan.deep => Hold::Deep,
                _ => Hold::Eased,
            };
            match self.touched.get_mut(&pid) {
                Some(t) => {
                    if t.hold != hold && platform::hold(pid, &t.saved, hold).is_ok() {
                        t.hold = hold;
                    }
                    changed |= t.level != level;
                    t.level = level;
                }
                None => {
                    let done = platform::save(pid).and_then(|s| platform::hold(pid, &s, hold).map(|_| s));
                    match done {
                        Ok(saved) => {
                            let (name, start) = (p.name.clone(), p.start);
                            let t = Touched { name, start, level, saved, frozen: false, hold, since: Instant::now() };
                            self.touched.insert(pid, t);
                            self.events += 1;
                            changed = true;
                        }
                        Err(_) => {
                            self.refused.insert(pid, p.start);
                        }
                    }
                }
            }
        }

        // Pulses first: this waits until the pulse thread has resumed what it paused,
        // so a pulse can never wake a process that is being frozen below.
        self.pulse.set(self.pids_at(Level::Pulse));
        for (pid, t) in self.touched.iter_mut() {
            if t.level == Level::Freeze && !t.frozen {
                t.frozen = platform::suspend(*pid).is_ok();
            } else if t.level != Level::Freeze && t.frozen {
                let _ = platform::resume(*pid);
                t.frozen = false;
            }
        }

        for pid in &plan.trim {
            if platform::trim(*pid).is_ok() {
                self.trims += 1;
            }
        }
        if changed {
            self.write_state();
        }
    }

    fn pids_at(&self, level: Level) -> Vec<u32> {
        let mut pids: Vec<u32> = self.touched.iter().filter(|(_, t)| t.level == level).map(|(p, _)| *p).collect();
        pids.sort_unstable();
        pids
    }

    fn release(&mut self, pid: u32) {
        if let Some(t) = self.touched.remove(&pid) {
            if t.level == Level::Pulse {
                self.pulse.set(self.pids_at(Level::Pulse));
            }
            if t.frozen {
                let _ = platform::resume(pid);
            }
            let _ = platform::restore(pid, &t.saved);
        }
    }

    /// Put every process back the way it was.
    pub fn restore_all(&mut self) {
        let held: Vec<u32> = self.touched.keys().copied().collect();
        for pid in held {
            self.release(pid);
        }
        self.pulse.set(vec![]);
        self.write_state();
    }

    /// Keep a list on disk of what is held back, so a crash can be cleaned up on the next start.
    fn write_state(&self) {
        if self.touched.is_empty() {
            let _ = std::fs::remove_file(state_file());
            return;
        }
        let held: Vec<Held> = self
            .touched
            .iter()
            .map(|(pid, t)| Held {
                pid: *pid,
                start: t.start,
                saved: t.saved,
                stopped: matches!(t.level, Level::Pulse | Level::Freeze),
            })
            .collect();
        let _ = std::fs::create_dir_all(config::data_dir());
        if let Ok(text) = serde_json::to_string(&held) {
            let _ = std::fs::write(state_file(), text);
        }
    }
}

/// Undo what an earlier Spry left behind if it was killed. Returns how many processes were put back.
pub fn recover() -> usize {
    let Ok(text) = std::fs::read_to_string(state_file()) else { return 0 };
    let _ = std::fs::remove_file(state_file());
    let held: Vec<Held> = serde_json::from_str(&text).unwrap_or_default();
    if held.is_empty() {
        return 0;
    }
    let Ok(mut sys) = System::new() else { return 0 };
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    let mut done = 0;
    for h in held {
        let same = sys.process(sysinfo::Pid::from_u32(h.pid)).is_some_and(|p| p.start_time() == h.start);
        if same {
            if h.stopped {
                let _ = platform::resume(h.pid);
            }
            let _ = platform::restore(h.pid, &h.saved);
            done += 1;
        }
    }
    done
}

// ---------------------------------------------------------------- the loop

fn status_json(
    shared: &Shared,
    cfg: &Config,
    snap: &Snapshot,
    plan: &Plan,
    engine: &Engine,
    st: &PolicyState,
    disk: (u64, u64),
) -> String {
    let by_pid: HashMap<u32, &Proc> = snap.procs.iter().map(|p| (p.pid, p)).collect();
    let mut held: Vec<_> = engine
        .touched
        .iter()
        .map(|(pid, t)| {
            let p = by_pid.get(pid);
            json!({ "pid": pid, "name": t.name, "level": t.level, "cpu": p.map_or(0.0, |p| p.cpu),
                    "mem": p.map_or(0, |p| p.mem), "secs": t.since.elapsed().as_secs() })
        })
        .collect();
    held.sort_by(|a, b| b["cpu"].as_f64().partial_cmp(&a["cpu"].as_f64()).unwrap_or(std::cmp::Ordering::Equal));

    // One row per app name.
    struct App {
        cpu: f32,
        mem: u64,
        count: u32,
        state: u8,
        system: bool,
    }
    let mut apps: HashMap<&str, App> = HashMap::new();
    for p in snap.procs.iter().filter(|p| p.mine) {
        let state = if plan.family.contains(&p.pid) {
            5
        } else {
            match engine.touched.get(&p.pid).map(|t| t.level) {
                Some(Level::Freeze) => 4,
                Some(Level::Pulse) => 3,
                Some(Level::Ease) => 2,
                Some(Level::Lent) => 1,
                None => 0,
            }
        };
        let app = apps.entry(p.name.as_str()).or_insert(App { cpu: 0.0, mem: 0, count: 0, state: 0, system: true });
        app.cpu += p.cpu;
        app.mem += p.mem;
        app.count += 1;
        app.system &= p.system;
        app.state = app.state.max(state);
    }
    let mut rows: Vec<_> = apps.into_iter().collect();
    rows.sort_by(|a, b| {
        let weight = |x: &App| x.cpu as f64 + x.mem as f64 / (64 << 20) as f64;
        weight(&b.1).partial_cmp(&weight(&a.1)).unwrap_or(std::cmp::Ordering::Equal)
    });
    let apps: Vec<_> = rows
        .iter()
        .enumerate()
        .filter(|(i, (name, _))| *i < 14 || policy::rule_for_name(cfg, name) != policy::Rule::Auto)
        .map(|(_, (name, a))| {
            let state = ["", "lent", "ease", "pulse", "freeze", "front"][a.state as usize];
            json!({ "name": name, "cpu": a.cpu, "mem": a.mem, "count": a.count, "state": state,
                    "protected": is_protected(name) || a.system, "rule": policy::rule_for_name(cfg, name) })
        })
        .collect();

    let me = by_pid.get(&snap.self_pid);
    json!({
        "version": config::VERSION,
        "os": std::env::consts::OS,
        "enabled": cfg.enabled,
        "paused_secs": shared.paused_secs(),
        "off_reference": shared.reference.lock().unwrap().status(Instant::now()),
        "fg_known": snap.fg.is_some(),
        "fg": snap.fg.and_then(|pid| by_pid.get(&pid)).map(|p| p.name.clone()),
        "cpu": snap.cpu_total,
        "mem_free_pct": snap.mem_free_pct,
        "mem_total": snap.mem_total,
        "pressure": st.pressure,
        "front_cpu": st.front_cpu,
        "front_rested_secs": st.rested(Instant::now()).map(|d| d.as_secs_f32()),
        "front_now": snap.front_now,
        "lending": engine.touched.values().any(|t| t.level == Level::Lent),
        "mem_tight": snap.mem_free_pct < cfg.pressure_mem_free,
        "disk_total": disk.0,
        "disk_free": disk.1,
        "held": held,
        "refused": engine.refused.keys().filter_map(|pid| by_pid.get(pid)).map(|p| p.name.clone()).collect::<Vec<_>>(),
        "apps": apps,
        "events": engine.events,
        "trims": engine.trims,
        "cores": crate::bench::cores(),
        "self_cpu": me.map_or(0.0, |p| p.cpu),
        "self_mem": me.map_or(0, |p| p.mem),
        "ease_text": if cfg.mode == config::Mode::Saver { platform::SAVER_TEXT } else { platform::EASE_TEXT },
        "can_trim": platform::CAN_TRIM,
        "autostart": shared.autostart.load(Ordering::Relaxed),
        "update": *shared.update.lock().unwrap(),
        "config": cfg,
    })
    .to_string()
}

/// Leave the OFF reference measurement at the original cadence. Foreground checks stay independent
/// of these slower, full-machine scans, so a switch of app can still release its existing holds.
fn scan_interval(saver: bool, watched: bool, recording: bool, cpu: f32) -> Duration {
    if recording {
        SLOW
    } else if saver {
        Duration::from_secs(if watched || cpu >= CALM_BELOW_CPU { 4 } else { 8 })
    } else if watched || cpu >= CALM_BELOW_CPU {
        SLOW
    } else {
        CALM
    }
}

/// Runs until `shared.quit` is set, then restores every process it touched.
pub fn run_loop(shared: &Shared) {
    let mut sampler = Sampler::new();
    let mut engine = Engine::default();
    let mut st = PolicyState::default();
    let mut snap = Snapshot::default();
    let mut measured: Option<Instant> = None;
    // For tests on machines without a screen: this process stands in for the app in front.
    let front: Option<u32> = std::env::var("SPRY_FRONT_PID").ok().and_then(|pid| pid.parse().ok());
    // The app in front, while held apps are lent its idle time.
    let mut resting: Option<HashSet<u32>> = None;

    while !shared.quit.load(Ordering::Relaxed) {
        let cfg = shared.cfg.lock().unwrap().clone();
        let watched = shared.watched.lock().unwrap().is_some_and(|t| t.elapsed() < WATCHED_FOR);
        let recording = shared.reference.lock().unwrap().active();
        let saver = cfg.enabled && cfg.mode == config::Mode::Saver;
        let every = scan_interval(saver, watched, recording, snap.cpu_total);
        let slow = measured.is_none_or(|t| t.elapsed() >= every);
        if slow {
            snap = sampler.sample();
            measured = Some(Instant::now());
        }
        snap.fg = front.or_else(platform::foreground_pid);
        // Read it twice a second, so the hold is back quickly when it starts to work.
        snap.front_now = resting.as_ref().and_then(|family| sampler.front_now(family));

        let plan = if !recording && cfg.enabled && shared.paused_secs() == 0 {
            policy::decide(&cfg, &snap, &mut st, Instant::now())
        } else {
            st = PolicyState::default();
            Plan::default()
        };
        engine.apply(&plan, &snap);
        resting = plan.levels.values().any(|level| *level == Level::Lent).then(|| plan.family.clone());

        if recording {
            // Only fresh readings taken after the engine released its holds qualify for the OFF trace.
            let cpu = (slow && engine.touched.is_empty()).then_some(snap.cpu_total);
            shared.reference.lock().unwrap().advance(Instant::now(), cpu);
        }

        if slow {
            let disk = sampler.disk();
            *shared.status.lock().unwrap() = status_json(shared, &cfg, &snap, &plan, &engine, &st, disk);
            shared.held.store(engine.touched.len(), Ordering::Relaxed);
        }
        thread::sleep(FAST);
    }
    engine.restore_all();
}

#[cfg(test)]
mod cadence_tests {
    use super::*;

    #[test]
    fn saver_scans_less_often_but_does_not_change_the_reference() {
        assert_eq!(scan_interval(true, true, false, 10.0), Duration::from_secs(4));
        assert_eq!(scan_interval(true, false, false, 10.0), Duration::from_secs(8));
        assert_eq!(scan_interval(true, false, false, 90.0), Duration::from_secs(4));
        assert_eq!(scan_interval(true, false, true, 10.0), SLOW);
        assert_eq!(scan_interval(false, true, false, 10.0), SLOW);
        assert_eq!(scan_interval(false, false, false, 10.0), CALM);
    }
}

//! Reads the machine, asks the policy what to do, applies it, and always puts things back.

use crate::config::{self, Config, is_protected, norm};
use crate::platform::{self, Saved};
use crate::policy::{self, Level, Plan, PolicyState, Proc, Snapshot};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
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
const WATCHED_FOR: Duration = Duration::from_secs(10);
/// One pulse: paused for this long, then running for this long.
pub const PULSE_STOP: Duration = Duration::from_millis(120);
pub const PULSE_RUN: Duration = Duration::from_millis(80);

/// What the engine, the dashboard and the tray share.
pub struct Shared {
    pub cfg: Mutex<Config>,
    pub status: Mutex<String>,
    pub quit: AtomicBool,
    pub pause_until: Mutex<Option<Instant>>,
    pub token: String,
    pub held: AtomicUsize,
    pub autostart: AtomicBool,
    pub update: Mutex<Option<String>>,
    /// When the dashboard last asked for the numbers.
    pub watched: Mutex<Option<Instant>>,
}

impl Shared {
    pub fn new(cfg: Config) -> Arc<Self> {
        Arc::new(Shared {
            cfg: Mutex::new(cfg),
            status: Mutex::new("{}".into()),
            quit: AtomicBool::new(false),
            pause_until: Mutex::new(None),
            token: config::token(),
            held: AtomicUsize::new(0),
            autostart: AtomicBool::new(crate::install::is_installed()),
            update: Mutex::new(None),
            watched: Mutex::new(None),
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
    disk: (u64, u64),
    disk_at: Option<Instant>,
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
            disk: (0, 0),
            disk_at: None,
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

    pub fn sample(&mut self) -> Snapshot {
        self.refresh();
        let mut procs = Vec::with_capacity(self.sys.processes().len());
        let mut smooth = HashMap::with_capacity(self.smooth.len());
        for (pid, p) in self.sys.processes() {
            let pid = pid.as_u32();
            let now = p.cpu_usage();
            let cpu = self.smooth.get(&pid).map_or(now, |old| 0.5 * old + 0.5 * now);
            smooth.insert(pid, cpu);
            procs.push(Proc {
                pid,
                ppid: p.parent().map(|x| x.as_u32()),
                name: norm(&p.name().to_string_lossy()),
                cpu,
                mem: p.memory(),
                mine: self.uid.is_some() && p.user_id() == self.uid.as_ref(),
                start: p.start_time(),
            });
        }
        self.smooth = smooth;
        let total = self.sys.total_memory().max(1);
        Snapshot {
            procs,
            fg: None,
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
                thread::sleep(if idle { Duration::from_millis(400) } else { PULSE_RUN });
            }
        });
        Pulse { list, stop, thread: Some(thread) }
    }

    pub fn set(&self, pids: Vec<u32>) {
        let mut list = self.list.lock().unwrap();
        if *list != pids {
            *list = pids;
        }
    }
}

impl Drop for Pulse {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
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
    pub deep: bool,
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
            match self.touched.get_mut(&pid) {
                Some(t) => {
                    if t.deep != plan.deep && platform::apply(pid, &t.saved, plan.deep).is_ok() {
                        t.deep = plan.deep;
                    }
                    changed |= t.level != level;
                    t.level = level;
                }
                None => {
                    let done = platform::save(pid).and_then(|s| platform::apply(pid, &s, plan.deep).map(|_| s));
                    match done {
                        Ok(saved) => {
                            let (name, start) = (p.name.clone(), p.start);
                            let t = Touched {
                                name,
                                start,
                                level,
                                saved,
                                frozen: false,
                                deep: plan.deep,
                                since: Instant::now(),
                            };
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
            .map(|(pid, t)| Held { pid: *pid, start: t.start, saved: t.saved, stopped: t.level != Level::Ease })
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
    pressure: bool,
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
    }
    let mut apps: HashMap<&str, App> = HashMap::new();
    for p in snap.procs.iter().filter(|p| p.mine) {
        let state = if plan.family.contains(&p.pid) {
            4
        } else {
            match engine.touched.get(&p.pid).map(|t| t.level) {
                Some(Level::Freeze) => 3,
                Some(Level::Pulse) => 2,
                Some(Level::Ease) => 1,
                None => 0,
            }
        };
        let app = apps.entry(p.name.as_str()).or_insert(App { cpu: 0.0, mem: 0, count: 0, state: 0 });
        app.cpu += p.cpu;
        app.mem += p.mem;
        app.count += 1;
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
            let state = ["", "ease", "pulse", "freeze", "front"][a.state as usize];
            json!({ "name": name, "cpu": a.cpu, "mem": a.mem, "count": a.count, "state": state,
                    "protected": is_protected(name), "rule": policy::rule_for_name(cfg, name) })
        })
        .collect();

    let me = by_pid.get(&snap.self_pid);
    json!({
        "version": config::VERSION,
        "os": std::env::consts::OS,
        "enabled": cfg.enabled,
        "paused_secs": shared.paused_secs(),
        "fg_known": snap.fg.is_some(),
        "fg": snap.fg.and_then(|pid| by_pid.get(&pid)).map(|p| p.name.clone()),
        "cpu": snap.cpu_total,
        "mem_free_pct": snap.mem_free_pct,
        "mem_total": snap.mem_total,
        "pressure": pressure,
        "mem_tight": snap.mem_free_pct < cfg.pressure_mem_free,
        "disk_total": disk.0,
        "disk_free": disk.1,
        "held": held,
        "refused": engine.refused.keys().filter_map(|pid| by_pid.get(pid)).map(|p| p.name.clone()).collect::<Vec<_>>(),
        "apps": apps,
        "events": engine.events,
        "trims": engine.trims,
        "self_cpu": me.map_or(0.0, |p| p.cpu),
        "self_mem": me.map_or(0, |p| p.mem),
        "ease_text": platform::EASE_TEXT,
        "can_trim": platform::CAN_TRIM,
        "autostart": shared.autostart.load(Ordering::Relaxed),
        "update": *shared.update.lock().unwrap(),
        "config": cfg,
    })
    .to_string()
}

/// Runs until `shared.quit` is set, then restores every process it touched.
pub fn run_loop(shared: &Shared) {
    let mut sampler = Sampler::new();
    let mut engine = Engine::default();
    let mut st = PolicyState::default();
    let mut snap = Snapshot::default();
    let mut measured: Option<Instant> = None;

    while !shared.quit.load(Ordering::Relaxed) {
        let cfg = shared.cfg.lock().unwrap().clone();
        let watched = shared.watched.lock().unwrap().is_some_and(|t| t.elapsed() < WATCHED_FOR);
        let every = if watched || snap.cpu_total >= CALM_BELOW_CPU { SLOW } else { CALM };
        let slow = measured.is_none_or(|t| t.elapsed() >= every);
        if slow {
            snap = sampler.sample();
            measured = Some(Instant::now());
        }
        snap.fg = platform::foreground_pid();

        let plan = if cfg.enabled && shared.paused_secs() == 0 {
            policy::decide(&cfg, &snap, &mut st, Instant::now())
        } else {
            st = PolicyState::default();
            Plan::default()
        };
        engine.apply(&plan, &snap);

        if slow {
            let disk = sampler.disk();
            *shared.status.lock().unwrap() = status_json(shared, &cfg, &snap, &plan, &engine, st.pressure, disk);
            shared.held.store(engine.touched.len(), Ordering::Relaxed);
        }
        thread::sleep(FAST);
    }
    engine.restore_all();
}

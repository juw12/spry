//! A measurement on this very computer: how much of the lost speed comes back when busy
//! background processes are held back. It starts its own busy processes and removes them again.

use crate::engine::Pulse;
use crate::platform::{self, Hold, Saved};
use memmap2::MmapMut;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::sleep;
use std::time::{Duration, Instant};

#[derive(Serialize, Clone, Copy, Debug)]
pub struct Phase {
    /// Fixed pieces of work finished per second.
    pub work: f64,
    /// Time to wake up and finish one small piece of work, in milliseconds.
    pub p50_ms: f64,
    pub p95_ms: f64,
}

/// What the hold costs the held apps: their own steps per second, all of them together.
#[derive(Serialize, Clone, Copy, Debug)]
pub struct Background {
    /// While the app in front works: with Spry off, and held.
    pub front_works_off: f64,
    pub front_works_held: f64,
    /// While the app in front rests: left alone, held the way Spry 0.2 held them whatever the
    /// app in front did (the same again with pulses), and lent the idle time as Spry does now.
    pub front_rests_off: f64,
    pub front_rests_held: f64,
    pub front_rests_pulsed: Option<f64>,
    pub front_rests_lent: f64,
    /// The same three as a share of `front_rests_off`. The ways take turns in rounds; this is
    /// the middle round of (speed this way / speed left alone in the same round).
    pub rest_share_held: f64,
    pub rest_share_pulsed: Option<f64>,
    pub rest_share_lent: f64,
    /// How busy the whole computer was while the app in front rested, in percent: with Spry
    /// off, and with the background held as Saver mode holds it (the middle round of five).
    /// `None` where the system did not say.
    pub rest_busy_off: Option<f64>,
    pub rest_busy_held: Option<f64>,
}

/// How busy the whole computer was between two looks. Its own reading, apart from the one the
/// running Spry keeps: the Measure page runs this bench inside Spry.
struct Meter(sysinfo::System);

impl Meter {
    fn start() -> Option<Self> {
        let mut meter = Meter(sysinfo::System::new().ok()?);
        meter.0.refresh_cpu_usage();
        Some(meter)
    }

    /// Percent of the whole processor in use since the last look.
    fn look(&mut self) -> f64 {
        self.0.refresh_cpu_usage();
        f64::from(self.0.global_cpu_usage())
    }
}

#[derive(Serialize, Debug)]
pub struct Report {
    pub os: &'static str,
    pub cores: usize,
    pub burners: usize,
    /// Processor threads eased apps keep off under pressure (0 = this system has no such lever).
    pub kept_free: usize,
    /// Processor threads held apps keep to in Saver mode (0 = this system has no such lever).
    pub few: usize,
    pub secs: f64,
    pub quiet: Phase,
    pub busy: Phase,
    pub eased: Phase,
    pub pulsed: Option<Phase>,
    pub background: Background,
    pub ease_text: &'static str,
}

pub fn cores() -> usize {
    platform::usable_threads()
}

pub fn default_burners() -> usize {
    cores() * 2
}

fn chunk(n: u64) -> u64 {
    let mut x = 0x9E37_79B9_7F4A_7C15u64;
    for i in 0..n {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x = x.wrapping_add(i);
    }
    std::hint::black_box(x)
}

/// Finds a piece of work that takes about 4 ms on this machine.
fn calibrate() -> u64 {
    let mut n = 50_000u64;
    loop {
        let t = Instant::now();
        chunk(n);
        let took = t.elapsed().as_secs_f64();
        if took >= 0.002 {
            return ((n as f64) * 0.004 / took).max(1000.0) as u64;
        }
        n *= 2;
    }
}

fn measure(n: u64, secs: f64) -> Phase {
    let half = Duration::from_secs_f64(secs / 2.0);

    // Steady work, like an export or a build in the app you are using.
    let t = Instant::now();
    let mut done = 0u64;
    while t.elapsed() < half {
        chunk(n);
        done += 1;
    }
    let work = done as f64 / t.elapsed().as_secs_f64();

    // Short bursts after a pause, like reacting to a click. A second thread plays the hand on
    // the mouse: it waits, notes the time and wakes this one. The clock starts at that note,
    // so a timer that fires late (virtual machines do that) is not counted as a slow reaction.
    let nap = Duration::from_millis(16);
    let (click, clicks) = std::sync::mpsc::channel::<Instant>();
    let (done, dones) = std::sync::mpsc::channel::<()>();
    let hand = std::thread::spawn(move || {
        let t = Instant::now();
        while t.elapsed() < half {
            sleep(nap);
            if click.send(Instant::now()).is_err() || dones.recv().is_err() {
                break;
            }
        }
    });
    let mut times = Vec::new();
    for clicked in clicks {
        chunk(n);
        times.push(clicked.elapsed().as_secs_f64() * 1000.0);
        let _ = done.send(());
    }
    let _ = hand.join();
    if times.is_empty() {
        times.push(f64::NAN);
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let at = |q: f64| times[((times.len() - 1) as f64 * q) as usize];
    Phase { work, p50_ms: at(0.5), p95_ms: at(0.95) }
}

/// A count of steps that a busy process and the measuring process share: a small file that
/// both map into memory. Noting a step is one store into memory, with no call into the system.
/// Even so, the app in front is not measured against processes that count. With counting
/// processes the result with Spry off differed by up to 8% on one model of Windows test
/// machine, and it could not be settled whether the counting caused that, so the question is
/// removed: the app in front is measured against processes that only compute, exactly as in
/// 0.2, and the counting ones are a second set, used only for what the hold costs the held
/// apps. (Writing the count to a file did change the load: the app in front got about half of
/// a quiet machine's speed, not the 72% it gets against processes that only compute.)
struct Count {
    cell: *const AtomicU64,
    _map: MmapMut,
}

impl Count {
    fn open(path: &Path, create: bool) -> std::io::Result<Self> {
        let file = std::fs::OpenOptions::new().read(true).write(true).create(create).truncate(create).open(path)?;
        if create {
            file.set_len(8)?;
        }
        // SAFETY: the file belongs to this measurement alone and is only used through `cell`.
        let mut map = unsafe { MmapMut::map_mut(&file) }?;
        if map.len() < 8 {
            return Err(std::io::Error::other("the step count file is too short"));
        }
        Ok(Count { cell: map.as_mut_ptr().cast::<AtomicU64>(), _map: map })
    }

    fn cell(&self) -> &AtomicU64 {
        // SAFETY: a mapping starts on a page boundary (aligned for a u64), is at least 8 bytes
        // long, lives as long as `self`, and both processes touch it only as this one atomic.
        unsafe { &*self.cell }
    }
}

/// A process that only burns CPU. Stops by itself, so none is ever left behind for long.
/// This is the load the app in front is measured against, unchanged since 0.1.
pub fn burn(secs: u64) {
    let t = Instant::now();
    while t.elapsed().as_secs() < secs {
        chunk(2_000_000);
    }
}

/// The same, and it notes in the shared count how many steps it has done.
pub fn burn_counting(secs: u64, note: &Path) {
    let t = Instant::now();
    let count = Count::open(note, false).ok();
    let mut steps = 0u64;
    while t.elapsed().as_secs() < secs {
        chunk(2_000_000);
        steps += 1;
        if let Some(count) = &count {
            count.cell().store(steps, Ordering::Relaxed);
        }
    }
}

/// The busy background processes and, for a set that counts, the folder with the step counts
/// and the counts themselves.
struct Burners {
    kids: Vec<Child>,
    notes: Option<PathBuf>,
    counts: Vec<Count>,
}

impl Burners {
    /// Starts the busy processes and gives them a moment to get going. A `counting` set also
    /// notes its steps where this process can read them.
    fn start(exe: &Path, burners: usize, counting: bool) -> std::io::Result<Self> {
        let notes = counting.then(|| std::env::temp_dir().join(format!("spry-bench-{}", std::process::id())));
        if let Some(notes) = &notes {
            std::fs::create_dir_all(notes)?;
        }
        let mut set = Burners { kids: Vec::new(), notes, counts: Vec::new() };
        for i in 0..burners {
            let mut burner = std::process::Command::new(exe);
            let burner = platform::at_normal_priority(&mut burner).args(["burn", "180"]);
            if let Some(notes) = &set.notes {
                let note = notes.join(i.to_string());
                set.counts.push(Count::open(&note, true)?);
                burner.arg(note);
            }
            let child = burner.stdin(Stdio::null()).stdout(Stdio::null()).spawn()?;
            set.kids.push(child);
        }
        sleep(Duration::from_millis(700));
        Ok(set)
    }

    fn pids(&self) -> Vec<u32> {
        self.kids.iter().map(|child| child.id()).collect()
    }

    /// Each process with what it looked like before anything was held.
    fn saved(&self) -> Vec<(u32, Saved)> {
        self.pids().iter().filter_map(|pid| platform::save(*pid).ok().map(|saved| (*pid, saved))).collect()
    }

    /// Steps done so far by all of them.
    fn steps(&self) -> u64 {
        self.counts.iter().map(|count| count.cell().load(Ordering::Relaxed)).sum()
    }

    /// Their speed while `front` runs.
    fn speed_during<T>(&self, front: impl FnOnce() -> T) -> (T, f64) {
        let (before, t) = (self.steps(), Instant::now());
        let out = front();
        let secs = t.elapsed().as_secs_f64();
        (out, (self.steps() - before) as f64 / secs)
    }

    /// Their speed while `front` runs, and how busy the whole computer was meanwhile.
    fn speed_and_busy(&self, meter: &mut Option<Meter>, front: impl FnOnce()) -> (f64, Option<f64>) {
        if let Some(meter) = meter {
            meter.look();
        }
        let speed = self.speed_during(front).1;
        (speed, meter.as_mut().map(Meter::look))
    }
}

impl Drop for Burners {
    fn drop(&mut self) {
        for child in &mut self.kids {
            let _ = platform::resume(child.id());
            let _ = child.kill();
            let _ = child.wait();
        }
        // Windows removes a file only once nothing maps it any more.
        self.counts.clear();
        if let Some(notes) = &self.notes {
            let _ = std::fs::remove_dir_all(notes);
        }
    }
}

/// Holds the processes this way and gives the system a moment to act on it.
fn hold(saved: &[(u32, Saved)], how: Hold) {
    for (pid, s) in saved {
        let _ = platform::hold(*pid, s, how);
    }
    sleep(Duration::from_millis(300));
}

fn let_go(saved: &[(u32, Saved)]) {
    for (pid, s) in saved {
        let _ = platform::restore(*pid, s);
    }
}

pub fn run(secs: f64, burners: usize) -> std::io::Result<Report> {
    let exe = std::env::current_exe()?;
    // The measuring process stands in for the app in front, the burners for normal background apps.
    let _normal = platform::NormalPriority::hold();
    let n = calibrate();
    let quiet = measure(n, secs);

    // First the app in front, against busy processes that only compute: the load every
    // version has measured, so the gain can be compared from version to version.
    let (busy, eased, pulsed) = {
        let kids = Burners::start(&exe, burners, false)?;
        let busy = measure(n, secs);
        let saved = kids.saved();
        hold(&saved, Hold::Deep);
        let eased = measure(n, secs);
        let pulsed = if cfg!(unix) {
            let pulse = Pulse::new();
            pulse.set(kids.pids());
            sleep(Duration::from_millis(300));
            let phase = measure(n, secs);
            drop(pulse);
            Some(phase)
        } else {
            None
        };
        let_go(&saved);
        (busy, eased, pulsed)
    };

    // Then the other side of the trade, what the hold costs the held apps, with a second set
    // of busy processes that count their steps. First while the app in front works.
    let kids = Burners::start(&exe, burners, true)?;
    let pids = kids.pids();
    let saved = kids.saved();
    let front_works_off = kids.speed_during(|| measure(n, secs / 2.0)).1;
    hold(&saved, Hold::Deep);
    let front_works_held = kids.speed_during(|| measure(n, secs / 2.0)).1;

    // Now the app in front rests. The ways of holding take turns, five rounds, and each is
    // compared with "left alone" in the same round. The middle round counts, so a machine
    // whose speed drifts or stumbles once (shared test machines do both) cannot favour one.
    // "Held" is the hold of Saver mode: deep, and on a few processor threads where that can be set.
    const ROUNDS: usize = 5;
    let rest = || sleep(Duration::from_secs_f64(secs / 5.0));
    let (mut held, mut pulsed_rest, mut lent, mut off) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let (mut busy_held, mut busy_off) = (Vec::new(), Vec::new());
    let mut meter = Meter::start();
    for _ in 0..ROUNDS {
        hold(&saved, Hold::Saver);
        let (speed, busy) = kids.speed_and_busy(&mut meter, rest);
        held.push(speed);
        busy_held.extend(busy);
        if cfg!(unix) {
            let pulse = Pulse::new();
            pulse.set(pids.clone());
            sleep(Duration::from_millis(300));
            pulsed_rest.push(kids.speed_during(rest).1);
        }
        hold(&saved, Hold::Lent);
        lent.push(kids.speed_during(rest).1);
        let_go(&saved);
        sleep(Duration::from_millis(300));
        let (speed, busy) = kids.speed_and_busy(&mut meter, rest);
        off.push(speed);
        busy_off.extend(busy);
    }
    let middle = |mut values: Vec<f64>| {
        values.sort_by(f64::total_cmp);
        values.get(values.len() / 2).copied()
    };
    let mean = |speeds: &[f64]| speeds.iter().sum::<f64>() / speeds.len().max(1) as f64;
    let share = |speeds: &[f64]| {
        let mut shares: Vec<f64> = speeds.iter().zip(&off).map(|(speed, alone)| speed / alone.max(0.001)).collect();
        shares.sort_by(f64::total_cmp);
        shares[shares.len() / 2]
    };

    let background = Background {
        front_works_off,
        front_works_held,
        front_rests_off: mean(&off),
        front_rests_held: mean(&held),
        front_rests_pulsed: cfg!(unix).then(|| mean(&pulsed_rest)),
        front_rests_lent: mean(&lent),
        rest_share_held: share(&held),
        rest_share_pulsed: cfg!(unix).then(|| share(&pulsed_rest)),
        rest_share_lent: share(&lent),
        rest_busy_off: middle(busy_off),
        rest_busy_held: middle(busy_held),
    };
    Ok(Report {
        os: std::env::consts::OS,
        cores: cores(),
        burners,
        kept_free: platform::kept_free_count(),
        few: platform::few_count(),
        secs,
        quiet,
        busy,
        eased,
        pulsed,
        background,
        ease_text: platform::EASE_TEXT,
    })
}

pub fn print(r: &Report) {
    println!("Spry benchmark: {}, {} CPU threads, {} busy background processes\n", r.os, r.cores, r.burners);
    println!("  {:<26}{:>10}{:>16}{:>16}", "", "work/s", "response p50", "response p95");
    let row = |label: &str, p: &Phase| {
        println!("  {:<26}{:>10.1}{:>13.1} ms{:>13.1} ms", label, p.work, p.p50_ms, p.p95_ms);
    };
    row("quiet machine", &r.quiet);
    row("busy, Spry off", &r.busy);
    row("busy, Spry on", &r.eased);
    if let Some(p) = &r.pulsed {
        row("busy, Strong mode", p);
    }
    let best = &r.eased;
    println!(
        "\nWith Spry the app in front did {:.2}x the work and answered {:.1}x as fast (95th percentile)",
        best.work / r.busy.work,
        r.busy.p95_ms / best.p95_ms.max(0.001)
    );
    println!(
        "as with Spry off. A quiet machine is the upper limit: {:.0}% of it was reached.",
        100.0 * best.work / r.quiet.work
    );
    if let Some(p) = &r.pulsed {
        println!("Strong mode (pulses, off unless you tick it) reached {:.2}x.", p.work / r.busy.work);
    }
    println!("Easing on this system: {}.", r.ease_text);
    if r.kept_free > 0 {
        println!(
            "Under pressure {} of the {} processor threads were kept free for the app in front.",
            r.kept_free, r.cores
        );
    }

    let b = &r.background;
    println!("\nWhat that costs the apps that are held (their own speed, with Spry off = 100%):");
    println!(
        "  {:<46}{:>5.0}%",
        "while the app in front works",
        100.0 * b.front_works_held / b.front_works_off.max(0.001)
    );
    println!("  {:<46}{:>5.0}%", "while it rests, Saver mode: still held", 100.0 * b.rest_share_held);
    if let Some(pulsed) = b.rest_share_pulsed {
        println!("  {:<46}{:>5.0}%", "the same with pulses (Strong mode)", 100.0 * pulsed);
    }
    println!("  {:<46}{:>5.0}%", "while it rests, Performance mode: lent", 100.0 * b.rest_share_lent);
    if let (Some(off), Some(held)) = (b.rest_busy_off, b.rest_busy_held) {
        println!(
            "\nThe whole computer while the app in front rests: {off:.0}% busy with Spry off, {held:.0}% in Saver mode."
        );
    }
    if r.few > 0 {
        println!("In Saver mode held apps keep to {} of the {} processor threads.", r.few, r.cores);
    }
}

//! A measurement on this very computer: how much of the lost speed comes back when busy
//! background processes are held back. It starts its own busy processes and removes them again.

use crate::engine::Pulse;
use crate::platform::{self, Hold, Saved};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
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
}

#[derive(Serialize, Debug)]
pub struct Report {
    pub os: &'static str,
    pub cores: usize,
    pub burners: usize,
    /// Processor threads eased apps keep off under pressure (0 = this system has no such lever).
    pub kept_free: usize,
    pub secs: f64,
    pub quiet: Phase,
    pub busy: Phase,
    pub eased: Phase,
    pub pulsed: Option<Phase>,
    pub background: Background,
    pub ease_text: &'static str,
}

impl Report {
    /// The strongest result this system offers.
    pub fn best(&self) -> Phase {
        match self.pulsed {
            Some(p) if p.work > self.eased.work => p,
            _ => self.eased,
        }
    }
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

/// A process that only burns CPU. Stops by itself, so none is ever left behind for long.
/// With a file to write to, it notes there how many steps it has done, about 40 times a second.
pub fn burn(secs: u64, note: Option<&Path>) {
    let t = Instant::now();
    let (mut steps, mut noted) = (0u64, Instant::now());
    while t.elapsed().as_secs() < secs {
        chunk(2_000_000);
        steps += 1;
        if let Some(file) = note
            && noted.elapsed() >= Duration::from_millis(25)
        {
            let _ = std::fs::write(file, steps.to_string());
            noted = Instant::now();
        }
    }
}

/// The busy background processes, the folder they note their steps in, and the last count read.
struct Burners {
    kids: Vec<Child>,
    notes: PathBuf,
    steps: Vec<u64>,
}

impl Burners {
    /// Steps done so far by all of them. A note caught half-written keeps its last value.
    fn steps(&mut self) -> u64 {
        for (i, last) in self.steps.iter_mut().enumerate() {
            let read = std::fs::read_to_string(self.notes.join(i.to_string())).ok().and_then(|t| t.parse().ok());
            *last = read.unwrap_or(*last).max(*last);
        }
        self.steps.iter().sum()
    }

    /// Their speed while `front` runs.
    fn speed_during<T>(&mut self, front: impl FnOnce() -> T) -> (T, f64) {
        let (before, t) = (self.steps(), Instant::now());
        let out = front();
        let secs = t.elapsed().as_secs_f64();
        (out, (self.steps() - before) as f64 / secs)
    }
}

impl Drop for Burners {
    fn drop(&mut self) {
        for child in &mut self.kids {
            let _ = platform::resume(child.id());
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = std::fs::remove_dir_all(&self.notes);
    }
}

pub fn run(secs: f64, burners: usize) -> std::io::Result<Report> {
    let exe = std::env::current_exe()?;
    // The measuring process stands in for the app in front, the burners for normal background apps.
    let _normal = platform::NormalPriority::hold();
    let n = calibrate();
    let quiet = measure(n, secs);

    let notes = std::env::temp_dir().join(format!("spry-bench-{}", std::process::id()));
    std::fs::create_dir_all(&notes)?;
    let mut kids = Burners { kids: Vec::new(), notes, steps: vec![0; burners] };
    for i in 0..burners {
        let mut burner = std::process::Command::new(&exe);
        let burner =
            platform::at_normal_priority(&mut burner).args(["burn", "180"]).arg(kids.notes.join(i.to_string()));
        let child = burner.stdin(Stdio::null()).stdout(Stdio::null()).spawn()?;
        kids.kids.push(child);
    }
    sleep(Duration::from_millis(700));
    let (busy, front_works_off) = kids.speed_during(|| measure(n, secs));

    let pids: Vec<u32> = kids.kids.iter().map(|c| c.id()).collect();
    let saved: Vec<(u32, Saved)> = pids.iter().filter_map(|p| platform::save(*p).ok().map(|s| (*p, s))).collect();
    let hold = |how: Hold| {
        for (pid, s) in &saved {
            let _ = platform::hold(*pid, s, how);
        }
        sleep(Duration::from_millis(300));
    };
    hold(Hold::Deep);
    let (eased, front_works_held) = kids.speed_during(|| measure(n, secs));

    let pulsed = if cfg!(unix) {
        let pulse = Pulse::new();
        pulse.set(pids.clone());
        sleep(Duration::from_millis(300));
        let phase = measure(n, secs);
        drop(pulse);
        Some(phase)
    } else {
        None
    };

    // The other side of the trade: the app in front rests now, and the held apps are timed.
    // The ways of holding take turns, three rounds, so a machine whose speed drifts (shared
    // test machines do) cannot favour one of them.
    const ROUNDS: usize = 3;
    let rest = || sleep(Duration::from_secs_f64(secs / 4.0));
    let (mut held, mut pulsed_rest, mut lent, mut off) = (0.0, 0.0, 0.0, 0.0);
    for _ in 0..ROUNDS {
        hold(Hold::Deep);
        held += kids.speed_during(rest).1;
        if cfg!(unix) {
            let pulse = Pulse::new();
            pulse.set(pids.clone());
            sleep(Duration::from_millis(300));
            pulsed_rest += kids.speed_during(rest).1;
        }
        hold(Hold::Lent);
        lent += kids.speed_during(rest).1;
        for (pid, s) in &saved {
            let _ = platform::restore(*pid, s);
        }
        sleep(Duration::from_millis(300));
        off += kids.speed_during(rest).1;
    }
    let rounds = ROUNDS as f64;

    let background = Background {
        front_works_off,
        front_works_held,
        front_rests_off: off / rounds,
        front_rests_held: held / rounds,
        front_rests_pulsed: cfg!(unix).then_some(pulsed_rest / rounds),
        front_rests_lent: lent / rounds,
    };
    Ok(Report {
        os: std::env::consts::OS,
        cores: cores(),
        burners,
        kept_free: platform::kept_free_count(),
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
    row("busy, Spry easing", &r.eased);
    if let Some(p) = &r.pulsed {
        row("busy, Spry easing + pulses", p);
    }
    let best = r.best();
    println!(
        "\nWith Spry the app in front did {:.2}x the work and answered {:.1}x as fast (95th percentile)",
        best.work / r.busy.work,
        r.busy.p95_ms / best.p95_ms.max(0.001)
    );
    println!(
        "as with Spry off. A quiet machine is the upper limit: {:.0}% of it was reached.",
        100.0 * best.work / r.quiet.work
    );
    println!("Easing on this system: {}.", r.ease_text);
    if r.kept_free > 0 {
        println!(
            "Under pressure {} of the {} processor threads were kept free for the app in front.",
            r.kept_free, r.cores
        );
    }

    let b = &r.background;
    let share = |speed: f64, of: f64| 100.0 * speed / of.max(0.001);
    println!("\nWhat that costs the apps that are held (their own speed, with Spry off = 100%):");
    println!("  {:<46}{:>5.0}%", "while the app in front works", share(b.front_works_held, b.front_works_off));
    println!(
        "  {:<46}{:>5.0}%",
        "while it rests, held anyway (Spry before 0.3)",
        share(b.front_rests_held, b.front_rests_off)
    );
    if let Some(pulsed) = b.front_rests_pulsed {
        println!("  {:<46}{:>5.0}%", "the same with pulses", share(pulsed, b.front_rests_off));
    }
    println!("  {:<46}{:>5.0}%", "while it rests, now: the time is lent", share(b.front_rests_lent, b.front_rests_off));
}

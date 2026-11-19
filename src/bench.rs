//! A measurement on this very computer: how much of the lost speed comes back when busy
//! background processes are held back. It starts its own busy processes and removes them again.

use crate::engine::Pulse;
use crate::platform::{self, Saved};
use serde::Serialize;
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

#[derive(Serialize, Debug)]
pub struct Report {
    pub os: &'static str,
    pub cores: usize,
    pub burners: usize,
    pub secs: f64,
    pub quiet: Phase,
    pub busy: Phase,
    pub eased: Phase,
    pub pulsed: Option<Phase>,
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
    std::thread::available_parallelism().map_or(2, |n| n.get())
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

    // Short bursts after a pause, like reacting to a click.
    let nap = Duration::from_millis(16);
    let mut times = Vec::new();
    let t = Instant::now();
    while t.elapsed() < half {
        let start = Instant::now();
        sleep(nap);
        chunk(n);
        times.push(start.elapsed().saturating_sub(nap).as_secs_f64() * 1000.0);
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let at = |q: f64| times[((times.len() - 1) as f64 * q) as usize];
    Phase { work, p50_ms: at(0.5), p95_ms: at(0.95) }
}

/// A process that only burns CPU. Stops by itself, so none is ever left behind for long.
pub fn burn(secs: u64) {
    let t = Instant::now();
    while t.elapsed().as_secs() < secs {
        chunk(2_000_000);
    }
}

struct Burners(Vec<Child>);

impl Drop for Burners {
    fn drop(&mut self) {
        for child in &mut self.0 {
            let _ = platform::resume(child.id());
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

pub fn run(secs: f64, burners: usize) -> std::io::Result<Report> {
    let exe = std::env::current_exe()?;
    // The measuring process stands in for the app in front, the burners for normal background apps.
    let _normal = platform::NormalPriority::hold();
    let n = calibrate();
    let quiet = measure(n, secs);

    let mut kids = Burners(Vec::new());
    for _ in 0..burners {
        let mut burner = std::process::Command::new(&exe);
        let burner = platform::at_normal_priority(&mut burner).args(["burn", "180"]);
        let child = burner.stdin(Stdio::null()).stdout(Stdio::null()).spawn()?;
        kids.0.push(child);
    }
    sleep(Duration::from_millis(700));
    let busy = measure(n, secs);

    let pids: Vec<u32> = kids.0.iter().map(|c| c.id()).collect();
    let saved: Vec<(u32, Saved)> = pids.iter().filter_map(|p| platform::save(*p).ok().map(|s| (*p, s))).collect();
    for (pid, s) in &saved {
        let _ = platform::apply(*pid, s, true);
    }
    sleep(Duration::from_millis(300));
    let eased = measure(n, secs);

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

    for (pid, s) in &saved {
        let _ = platform::restore(*pid, s);
    }
    Ok(Report {
        os: std::env::consts::OS,
        cores: cores(),
        burners,
        secs,
        quiet,
        busy,
        eased,
        pulsed,
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
}

#![cfg_attr(windows, windows_subsystem = "windows")]

use spry::config::{self, Config, VERSION};
use spry::engine::{self, Shared};
use spry::{bench, clean, install, platform, server};
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

const HELP: &str = "Spry keeps an old computer quick: it holds back background apps so the app you are
using gets the machine. Everything is put back when Spry stops.

  spry              start Spry and open its window
  spry run          start Spry quietly (used at sign-in)
  spry open         open the window of the running Spry
  spry status       show what is being held back right now
  spry bench        measure the effect on this computer (under a minute)
  spry clean        show what storage can be cleared; add --yes to clear it
  spry install      start Spry when you sign in      (spry uninstall undoes it)
  spry rescue       put processes back after a crash
  spry front        show the process id of the app in front (checks that Spry can see it)
  spry version

Options: --no-tray (no tray icon), --json (bench, status), --secs N, --burners N and --threads N (bench).";

fn main() {
    platform::attach_console();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |flag: &str| args.iter().any(|a| a == flag);
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();

    match args.first().map(String::as_str).unwrap_or("") {
        "" => start(true, !has("--no-tray")),
        "run" => start(false, !has("--no-tray")),
        "open" => {
            if !open_page() {
                println!("Spry is not running. Start it with: spry");
            }
        }
        "status" => status(has("--json")),
        "bench" => {
            // A stand-in for a smaller machine: the whole measurement runs on the first N threads.
            if let Some(threads) = value("--threads").and_then(|s| s.parse::<usize>().ok())
                && !platform::squeeze(threads.max(1))
            {
                fail("this system cannot limit a program to some of the processor threads");
            }
            let secs = value("--secs").and_then(|s| s.parse().ok()).unwrap_or(4.0f64).clamp(1.0, 60.0);
            let burners =
                value("--burners").and_then(|s| s.parse().ok()).unwrap_or_else(bench::default_burners).clamp(1, 256);
            match bench::run(secs, burners) {
                Ok(report) if has("--json") => {
                    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default())
                }
                Ok(report) => bench::print(&report),
                Err(e) => fail(&format!("benchmark failed: {e}")),
            }
        }
        "burn" => {
            let secs = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(60);
            match args.get(2) {
                Some(note) => bench::burn_counting(secs, std::path::Path::new(note)),
                None => bench::burn(secs),
            }
        }
        "clean" => clean_cli(has("--yes")),
        "install" => match install::install() {
            Ok(message) => println!("{message}"),
            Err(e) => fail(&format!("could not install: {e}")),
        },
        "uninstall" => match install::uninstall() {
            Ok(()) => println!("Spry no longer starts when you sign in."),
            Err(e) => fail(&format!("could not uninstall: {e}")),
        },
        "rescue" => println!("Put {} process(es) back.", engine::recover()),
        "front" => match platform::foreground_pid() {
            Some(pid) => println!("{pid}"),
            None => fail("Spry cannot see which app is in front right now."),
        },
        // Spry's own window. Started by the running Spry, not by hand: it lives as long as its standard input.
        #[cfg(any(windows, target_os = "macos"))]
        "window" => spry::window::run(),
        "version" | "--version" | "-V" => println!("spry {VERSION}"),
        _ => println!("{HELP}"),
    }
    let _ = std::io::stdout().flush();
}

fn fail(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(1)
}

fn start(open: bool, tray: bool) {
    let Some(http) = server::bind() else {
        // Already running: the port is the lock.
        if open {
            open_page();
        }
        return;
    };
    // Started low (by a scheduled task, or by a parent that runs low), Spry would wait behind
    // the very apps it has to hold back and could not answer in time.
    let _normal = platform::NormalPriority::hold();
    let (cfg, first_run) = Config::load();
    if first_run {
        cfg.save();
    }
    engine::recover();
    let shared = Shared::new(cfg);
    server::serve(http, shared.clone());

    let on_signal = shared.clone();
    let _ = ctrlc::set_handler(move || on_signal.quit.store(true, Ordering::Relaxed));
    watch_for_updates(shared.clone());
    let show = open || first_run;

    // With a tray icon Spry has a window of its own for the page.
    #[cfg(any(windows, target_os = "macos"))]
    if tray {
        let for_engine = shared.clone();
        let worker = std::thread::spawn(move || engine::run_loop(&for_engine));
        spry::tray::run(shared, worker, show);
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = tray;
    if show {
        platform::open_url(&server::url(&shared.token));
    }
    engine::run_loop(&shared);
}

/// Once a day, ask GitHub for the newest release. Only tells; never installs anything.
fn watch_for_updates(shared: Arc<Shared>) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(30));
        loop {
            let out = platform::command("curl")
                .args(["-fsSL", "-m", "10", "https://api.github.com/repos/juw12/spry/releases/latest"])
                .output();
            if let Ok(out) = out {
                let latest = serde_json::from_slice::<serde_json::Value>(&out.stdout)
                    .ok()
                    .and_then(|v| v["tag_name"].as_str().map(|s| s.trim_start_matches('v').to_string()));
                if let Some(latest) = latest {
                    let number = |v: &str| v.split('.').filter_map(|p| p.parse::<u32>().ok()).collect::<Vec<_>>();
                    if number(&latest) > number(VERSION) {
                        *shared.update.lock().unwrap() = Some(latest);
                    }
                }
            }
            std::thread::sleep(Duration::from_secs(24 * 3600));
        }
    });
}

/// GET from the running Spry. None when it is not running.
fn ask(path: &str) -> Option<String> {
    server::send("GET", path)
}

fn open_page() -> bool {
    // The running Spry brings its own window to the front when it has one.
    if server::send("POST", "/api/show").is_some_and(|answer| answer.contains(r#""window":true"#)) {
        return true;
    }
    // No window (no tray icon, Linux, or a Spry from before 0.4): the page opens in the browser.
    let running = ask("/api/status").is_some();
    if running {
        platform::open_url(&server::url(&config::token()));
    }
    running
}

fn status(json: bool) {
    let Some(body) = ask("/api/status") else { fail("Spry is not running. Start it with: spry") };
    if json {
        println!("{body}");
        return;
    }
    let s: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
    let state = if !s["enabled"].as_bool().unwrap_or(false) {
        "switched off".to_string()
    } else if s["paused_secs"].as_u64().unwrap_or(0) > 0 {
        format!("paused for {} more seconds", s["paused_secs"])
    } else if !s["fg_known"].as_bool().unwrap_or(false) {
        "on, rules only (this desktop does not tell which app is in front)".to_string()
    } else {
        format!("on, {} is in front", s["fg"].as_str().unwrap_or("?"))
    };
    println!("Spry {VERSION}: {state}");
    println!(
        "CPU {:.0}%, memory free {:.0}%, Spry itself {:.1}% of one core and {}",
        s["cpu"].as_f64().unwrap_or(0.0),
        s["mem_free_pct"].as_f64().unwrap_or(0.0),
        s["self_cpu"].as_f64().unwrap_or(0.0),
        clean::human(s["self_mem"].as_u64().unwrap_or(0))
    );
    let held = s["held"].as_array().cloned().unwrap_or_default();
    if held.is_empty() {
        println!("Nothing is held back right now.");
    }
    for h in held {
        println!(
            "  {:<7}{:<28}{:>6.0}% CPU{:>12}",
            h["level"].as_str().unwrap_or(""),
            h["name"].as_str().unwrap_or(""),
            h["cpu"].as_f64().unwrap_or(0.0),
            clean::human(h["mem"].as_u64().unwrap_or(0))
        );
    }
}

fn clean_cli(yes: bool) {
    let found = clean::scan();
    let total: u64 = found.iter().map(|c| c.bytes).sum();
    for c in &found {
        println!("  {:<18}{:>10}  {}", c.label, clean::human(c.bytes), c.note);
    }
    println!("  {:<18}{:>10}", "Total", clean::human(total));
    if !yes {
        println!("\nNothing was removed. To clear all of the above: spry clean --yes");
        return;
    }
    let ids: Vec<String> = found.iter().map(|c| c.id.to_string()).collect();
    let freed: u64 = clean::clean(&ids).iter().map(|c| c.bytes).sum();
    println!("\nCleared {}. Files that were in use stayed.", clean::human(freed));
}

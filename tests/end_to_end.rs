//! The real program from start to finish: it eases a busy background process, shows it on
//! the dashboard API, puts it back when asked to quit, and `spry rescue` cleans up after a kill.
//! And the other side: while the app in front rests the held apps run at full speed, and the
//! hold is back within seconds when it works again.

use spry::platform;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::thread::sleep;
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

const SPRY: &str = env!("CARGO_BIN_EXE_spry");
const BUSY_NAME: &str = "sprybusy-test";
const FRONT_NAME: &str = "spryfront-test";

/// One whole-program test at a time: each starts its own Spry and its own busy processes.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

struct Kill(Child);

impl Drop for Kill {
    fn drop(&mut self) {
        let _ = platform::resume(self.0.id());
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A busy process that is nobody's child. On a developer's machine the test itself can
/// belong to the app in front, and Spry never touches the family of the app in front.
struct Orphan(u32);

impl Orphan {
    /// Every running process whose name starts with `name`.
    fn find(name: &str) -> Vec<u32> {
        let Ok(mut sys) = System::new() else { return Vec::new() };
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
        sys.processes()
            .iter()
            .filter(|(_, p)| p.name().to_string_lossy().starts_with(name))
            .map(|(pid, _)| pid.as_u32())
            .collect()
    }

    fn start(exe: &Path) -> Self {
        let name = exe.file_stem().expect("a file name").to_string_lossy().to_string();
        let already = Self::find(&name);
        // The shell starts the process and exits at once.
        #[cfg(windows)]
        let mut shell = {
            let mut cmd = Command::new("cmd");
            // "/normal": a child of a low-priority shell would start low, and then
            // there would be nothing for Spry to lower (CI runners start everything low).
            cmd.args(["/c", "start", "", "/b", "/normal"]).arg(exe).args(["burn", "180"]);
            cmd
        };
        #[cfg(unix)]
        let mut shell = {
            let mut cmd = Command::new("sh");
            cmd.arg("-c").arg(r#""$0" burn 180 >/dev/null 2>&1 &"#).arg(exe);
            cmd
        };
        let status =
            shell.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().expect("run the shell");
        assert!(status.success());
        let mut pid = None;
        wait_until("the busy process runs", 15, || {
            pid = Self::find(&name).into_iter().find(|pid| !already.contains(pid));
            pid.is_some()
        });
        sleep(Duration::from_millis(300));
        Orphan(pid.unwrap())
    }
}

impl Drop for Orphan {
    fn drop(&mut self) {
        let _ = platform::resume(self.0);
        if let Ok(mut sys) = System::new() {
            let pid = Pid::from_u32(self.0);
            sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), false, ProcessRefreshKind::nothing());
            if let Some(process) = sys.process(pid) {
                process.kill();
            }
        }
    }
}

fn spry(home: &Path, port: u16, args: &[&str]) -> Command {
    let mut cmd = Command::new(SPRY);
    cmd.args(args).env("SPRY_HOME", home).env("SPRY_PORT", port.to_string()).stdin(Stdio::null());
    cmd
}

/// (status code, body) of a request to the running Spry.
fn http(port: u16, method: &str, path: &str) -> Option<(u16, String)> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let request =
        format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).ok()?;
    let mut reply = String::new();
    stream.read_to_string(&mut reply).ok()?;
    let (head, body) = reply.split_once("\r\n\r\n")?;
    Some((head.split_whitespace().nth(1)?.parse().ok()?, body.to_string()))
}

/// (status code, content type, size of the body) of a GET whose body need not be text.
fn fetch(port: u16, path: &str) -> Option<(u16, String, usize)> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let request = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).ok()?;
    let mut reply = Vec::new();
    stream.read_to_end(&mut reply).ok()?;
    let split = reply.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = String::from_utf8_lossy(&reply[..split]).to_string();
    let kind = head
        .lines()
        .find_map(|line| line.to_ascii_lowercase().strip_prefix("content-type:").map(|v| v.trim().to_string()))
        .unwrap_or_default();
    Some((head.split_whitespace().nth(1)?.parse().ok()?, kind, reply.len() - split - 4))
}

/// Where the running Spry answers, for the report when a wait times out.
static RUNNING: std::sync::Mutex<Option<(u16, PathBuf)>> = std::sync::Mutex::new(None);

/// What Spry itself saw, in a few lines.
fn report() -> String {
    let Some((port, home)) = RUNNING.lock().unwrap().clone() else { return "spry is not running".into() };
    let token = std::fs::read_to_string(home.join("token")).unwrap_or_default();
    let Some((code, body)) = http(port, "GET", &format!("/api/status?t={token}")) else {
        return "spry does not answer".into();
    };
    let status: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
    let busy: Vec<_> = status["apps"]
        .as_array()
        .map(|apps| apps.iter().filter(|a| a["name"].as_str().is_some_and(|n| n.starts_with("sprybusy"))).collect())
        .unwrap_or_default();
    format!(
        "status {code}: enabled={} paused={} fg_known={} fg={} front: {} held={} refused={} busy rows={:?} level now={:?}",
        status["enabled"],
        status["paused_secs"],
        status["fg_known"],
        status["fg"],
        front_seen(&status),
        status["held"],
        status["refused"],
        busy,
        Orphan::find(BUSY_NAME).first().copied().and_then(platform::read_level),
    )
}

/// What Spry saw of the app in front, and what it decided for the busy apps.
fn front_seen(status: &serde_json::Value) -> String {
    let levels: Vec<&str> = status["held"]
        .as_array()
        .map(|held| held.iter().filter_map(|h| h["level"].as_str()).collect())
        .unwrap_or_default();
    format!(
        "used {} now {} rested {} s, held {levels:?}",
        status["front_cpu"], status["front_now"], status["front_rested_secs"]
    )
}

fn wait_until(what: &str, secs: u64, mut done: impl FnMut() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(start.elapsed() < Duration::from_secs(secs), "timed out waiting until {what}\n{}", report());
        sleep(Duration::from_millis(200));
    }
}

fn start_spry(home: &Path, port: u16) -> Kill {
    let child = spry(home, port, &["run", "--no-tray"]).stdout(Stdio::null()).spawn().expect("start spry");
    wait_until("the dashboard answers", 20, || http(port, "GET", "/").is_some());
    *RUNNING.lock().unwrap() = Some((port, home.to_path_buf()));
    Kill(child)
}

#[test]
fn eases_shows_restores_and_rescues() {
    let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let home: PathBuf = std::env::temp_dir().join(format!("spry-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    let port = TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port();

    // A busy process under its own name ("spry" itself is never touched), with a rule for it,
    // so the test also works on a machine without a screen.
    let busy_exe = home.join(format!("{BUSY_NAME}{}", std::env::consts::EXE_SUFFIX));
    std::fs::copy(SPRY, &busy_exe).unwrap();
    std::fs::write(home.join("config.json"), format!(r#"{{"always_slow":["{BUSY_NAME}"]}}"#)).unwrap();
    let busy = Orphan::start(&busy_exe);
    let pid = busy.0;
    let before = platform::read_level(pid).expect("read the starting state");
    let eased = || platform::read_level(pid).is_some_and(|level| level != before);

    // 1. Spry eases it and says so.
    let mut running = start_spry(&home, port);
    wait_until("the busy process is eased", 30, eased);
    let token = std::fs::read_to_string(home.join("token")).unwrap();
    assert_eq!(http(port, "GET", "/api/status").map(|r| r.0), Some(403), "no token, no answer");
    assert_eq!(http(port, "GET", "/api/status?t=wrong").map(|r| r.0), Some(403));
    // The page brings its own typefaces, behind the same token as everything else.
    let page = http(port, "GET", &format!("/?t={token}")).expect("the page").1;
    assert!(page.contains(&format!("/fonts/geist.woff2?t={token}")), "the page asks Spry itself for its typeface");
    assert!(!page.contains("confirm("), "the page asks its own questions: Spry's window has no browser boxes");
    for name in ["geist", "geist-mono", "instrument-serif"] {
        let font = fetch(port, &format!("/fonts/{name}.woff2?t={token}")).expect("an answer");
        assert_eq!((font.0, font.1.as_str()), (200, "font/woff2"), "{name} is served as a typeface");
        assert!(font.2 > 10_000, "{name} arrives whole ({} bytes)", font.2);
        assert_eq!(fetch(port, &format!("/fonts/{name}.woff2")).map(|r| r.0), Some(403), "no token, no typeface");
    }
    // Started without a tray icon, this Spry has no window of its own, and says so to whoever asks for it.
    let shown = http(port, "POST", &format!("/api/show?t={token}")).expect("an answer");
    assert_eq!(shown.0, 200);
    assert!(shown.1.contains(r#""window":false"#), "no window here: {}", shown.1);
    wait_until("the dashboard lists the eased process", 15, || {
        let Some((200, body)) = http(port, "GET", &format!("/api/status?t={token}")) else { return false };
        let status: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
        status["held"].as_array().is_some_and(|held| held.iter().any(|h| h["pid"] == pid && h["level"] == "ease"))
    });
    assert!(home.join("held.json").exists(), "the crash-recovery list is on disk while something is held");

    // 2. Pausing lets go, resuming eases again.
    assert_eq!(http(port, "POST", &format!("/api/pause?secs=600&t={token}")).map(|r| r.0), Some(200));
    wait_until("a pause puts the process back", 15, || platform::read_level(pid) == Some(before));
    assert_eq!(http(port, "POST", &format!("/api/pause?secs=0&t={token}")).map(|r| r.0), Some(200));
    wait_until("the process is eased again after the pause", 30, eased);

    // 3. Quitting puts everything back.
    assert_eq!(http(port, "POST", &format!("/api/quit?t={token}")).map(|r| r.0), Some(200));
    wait_until("spry has quit", 15, || running.0.try_wait().ok().flatten().is_some());
    assert_eq!(platform::read_level(pid), Some(before), "quitting must put the process back exactly");
    assert!(!home.join("held.json").exists(), "nothing held, no list");

    // 4. Killed without warning: the process stays eased until `spry rescue` (or the next start).
    let mut running = start_spry(&home, port);
    wait_until("the busy process is eased again", 30, eased);
    running.0.kill().unwrap();
    running.0.wait().unwrap();
    assert!(eased(), "a kill gives Spry no chance to tidy up");
    let rescue = spry(&home, port, &["rescue"]).output().expect("run spry rescue");
    assert!(rescue.status.success());
    assert_eq!(platform::read_level(pid), Some(before), "rescue must put the process back exactly");
    // Other busy processes on the machine may have been held as well, so the count is 1 or more.
    let said = String::from_utf8_lossy(&rescue.stdout).to_string();
    assert!(said.starts_with("Put ") && !said.starts_with("Put 0 "), "rescue says what it did, it said: {said}");

    drop(busy);
    sleep(Duration::from_millis(300));
    let _ = std::fs::remove_dir_all(&home);
}

/// Held apps are lent the processor while the app in front rests, and the hold is back
/// within three seconds when it works again. The app in front is a stand-in the test can
/// stop and start, named to Spry with SPRY_FRONT_PID, so this also runs without a screen.
#[test]
fn lends_while_the_front_app_rests_and_holds_again_when_it_works() {
    let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    // CI runners on Windows start everything low; with every processor thread busy at normal
    // priority this test and the Spry it starts would hardly get a turn.
    let _normal = platform::NormalPriority::hold();
    let home: PathBuf = std::env::temp_dir().join(format!("spry-lend-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    let port = TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port();
    std::fs::write(home.join("config.json"), "{}").unwrap();

    // One busy app in front, and enough busy apps behind it to fill every processor thread.
    let copy = |name: &str| {
        let exe = home.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
        std::fs::copy(SPRY, &exe).unwrap();
        exe
    };
    let (front_exe, busy_exe) = (copy(FRONT_NAME), copy(BUSY_NAME));
    let front = Orphan::start(&front_exe);
    let threads = std::thread::available_parallelism().map_or(2, |n| n.get()).clamp(2, 8);
    let backs: Vec<Orphan> = (0..threads).map(|_| Orphan::start(&busy_exe)).collect();
    let before: Vec<(u32, i64, Option<u32>)> = backs
        .iter()
        .map(|b| (b.0, platform::read_level(b.0).expect("read the starting state"), platform::cpus_open(b.0)))
        .collect();

    // What the operating system itself reports: is this process held back right now?
    let by_threads = cfg!(not(target_os = "macos")) && platform::kept_free_count() > 0;
    let held = |(pid, level, open): &(u32, i64, Option<u32>)| {
        if by_threads {
            platform::cpus_open(*pid) < *open
        } else {
            platform::read_level(*pid).is_some_and(|now| now != *level)
        }
    };
    // On Linux and Windows without kept-free threads lending shows only in Spry's own report.
    let shows = by_threads || cfg!(target_os = "macos");
    // On macOS and Windows nothing of the hold is left while the time is lent: the state the
    // system reports is the one the process started with. (Linux keeps the idle disk priority.)
    let as_found = cfg!(any(target_os = "macos", windows));
    let level_of = |token: &str, pid: u32| -> Option<String> {
        let (200, body) = http(port, "GET", &format!("/api/status?t={token}"))? else { return None };
        let status: serde_json::Value = serde_json::from_str(&body).ok()?;
        let row = status["held"].as_array()?.iter().find(|h| h["pid"] == pid)?;
        row["level"].as_str().map(str::to_string)
    };

    // 1. The app in front works: the busy apps behind it are held.
    let mut cmd = spry(&home, port, &["run", "--no-tray"]);
    let child = cmd.env("SPRY_FRONT_PID", front.0.to_string()).stdout(Stdio::null()).spawn().expect("start spry");
    let mut running = Kill(child);
    wait_until("the dashboard answers", 20, || http(port, "GET", "/").is_some());
    *RUNNING.lock().unwrap() = Some((port, home.clone()));
    let token = std::fs::read_to_string(home.join("token")).unwrap();
    wait_until("every busy app is held while the app in front works", 40, || {
        before.iter().all(|b| level_of(&token, b.0).as_deref() == Some("ease") && (!shows || held(b)))
    });

    // 2. It rests: they get the processor back, all of it.
    platform::suspend(front.0).expect("the app in front stops working");
    // Printed only when the test fails: what Spry saw, each time it changed.
    let (stopped, mut last) = (Instant::now(), String::new());
    wait_until("the busy apps are lent the idle time", 40, || {
        if let Some((200, body)) = http(port, "GET", &format!("/api/status?t={token}")) {
            let seen = front_seen(&serde_json::from_str(&body).unwrap_or_default());
            if seen != last {
                println!("{:5.1} s after the stop: {seen}", stopped.elapsed().as_secs_f32());
                last = seen;
            }
        }
        before.iter().all(|b| {
            level_of(&token, b.0).as_deref() == Some("lent")
                && (!shows || !held(b))
                && (!as_found || platform::read_level(b.0) == Some(b.1))
        })
    });

    // 3. It works again: the hold is back within three seconds.
    platform::resume(front.0).expect("the app in front works again");
    let resumed = Instant::now();
    let limit = Duration::from_secs(3);
    let back = loop {
        let done = if shows {
            before.iter().all(held)
        } else {
            before.iter().all(|b| level_of(&token, b.0).as_deref() == Some("ease"))
        };
        if done {
            break resumed.elapsed();
        }
        assert!(resumed.elapsed() < Duration::from_secs(20), "the hold never came back\n{}", report());
        sleep(Duration::from_millis(20));
    };
    println!("the hold was back {} ms after the app in front started to work", back.as_millis());
    if let Some(folder) = std::env::var_os("SPRY_NOTES") {
        let _ = std::fs::create_dir_all(&folder);
        let _ = std::fs::write(Path::new(&folder).join("hold-back-ms.txt"), back.as_millis().to_string());
    }
    // Spry's own report is renewed every two seconds, so only the system's answer is timed.
    assert!(!shows || back < limit, "the hold took {} ms to come back, the limit is 3000", back.as_millis());

    // 4. Quitting puts everything back exactly.
    assert_eq!(http(port, "POST", &format!("/api/quit?t={token}")).map(|r| r.0), Some(200));
    wait_until("spry has quit", 15, || running.0.try_wait().ok().flatten().is_some());
    for (pid, level, open) in &before {
        assert_eq!(platform::read_level(*pid), Some(*level), "quitting must put every process back exactly");
        assert_eq!(platform::cpus_open(*pid), *open, "and open every processor thread again");
    }

    drop((front, backs));
    sleep(Duration::from_millis(300));
    let _ = std::fs::remove_dir_all(&home);
}

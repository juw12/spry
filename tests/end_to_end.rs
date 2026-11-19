//! The real program from start to finish: it eases a busy background process, shows it on
//! the dashboard API, puts it back when asked to quit, and `spry rescue` cleans up after a kill.

use spry::platform;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

const SPRY: &str = env!("CARGO_BIN_EXE_spry");
const BUSY_NAME: &str = "sprybusy-test";

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
    fn find() -> Option<u32> {
        let mut sys = System::new().ok()?;
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
        sys.processes()
            .iter()
            .find(|(_, p)| p.name().to_string_lossy().starts_with(BUSY_NAME))
            .map(|(pid, _)| pid.as_u32())
    }

    fn start(exe: &Path) -> Self {
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
            pid = Self::find();
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
        "status {code}: enabled={} paused={} fg_known={} fg={} held={} refused={} busy rows={:?} level now={:?}",
        status["enabled"],
        status["paused_secs"],
        status["fg_known"],
        status["fg"],
        status["held"],
        status["refused"],
        busy,
        Orphan::find().and_then(platform::read_level),
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

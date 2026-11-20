//! Updating, with the real program. An older Spry replaces itself with the newer download and
//! starts again as it; a download that does not match its checksum, or is not the announced
//! version, changes nothing. The "release" is a folder the test makes (`SPRY_UPDATE_FROM`), and
//! the download in it is the program under test, packed the way the release workflow packs it.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::Mutex;
use std::thread::sleep;
use std::time::{Duration, Instant};

const SPRY: &str = env!("CARGO_BIN_EXE_spry");
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// One at a time: each test has its own installed Spry and its own release folder.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// An installed Spry in a folder of its own, and the release folder it looks at.
struct Installed {
    dir: PathBuf,
    /// The program file.
    program: PathBuf,
    /// What an update replaces: the program, or on a Mac the app around it.
    target: PathBuf,
    release: PathBuf,
    asset: &'static str,
    port: u16,
}

fn tool(name: &str) -> Command {
    if cfg!(windows) {
        let root = std::env::var_os("SystemRoot").expect("SystemRoot");
        Command::new(PathBuf::from(root).join("System32").join(format!("{name}.exe")))
    } else {
        Command::new(name)
    }
}

impl Installed {
    /// `as_app`: inside Spry.app, as install.sh puts it on a Mac.
    fn new(name: &str, as_app: bool) -> Self {
        let dir = std::env::temp_dir().join(format!("spry-update-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let place = dir.join("installed");
        let release = dir.join("release");
        let file = format!("spry{}", std::env::consts::EXE_SUFFIX);
        let (program, target) = if as_app {
            (place.join("Spry.app/Contents/MacOS").join(&file), place.join("Spry.app"))
        } else {
            (place.join(&file), place.join(&file))
        };
        std::fs::create_dir_all(program.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&release).unwrap();
        std::fs::create_dir_all(dir.join("home")).unwrap();
        std::fs::copy(SPRY, &program).unwrap();
        let asset = spry::update::asset(&program).expect("a download exists for the system the tests run on");
        let port = TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port();
        let installed = Installed { dir, program, target, release, asset, port };
        installed.publish(VERSION);
        installed
    }

    /// Packs the installed program as the release workflow does, next to its checksum, and announces `version`.
    fn publish(&self, version: &str) {
        let packed = self.release.join(self.asset);
        let place = self.target.parent().unwrap();
        let name = self.target.file_name().unwrap();
        let mut pack = if self.asset == "Spry-macos.zip" {
            let mut ditto = Command::new("ditto");
            ditto.args(["-c", "-k", "--keepParent"]).arg(&self.target).arg(&packed);
            ditto
        } else {
            let mut tar = tool("tar");
            tar.args(if self.asset.ends_with(".zip") { ["-a", "-cf"] } else { ["-c", "-zf"] });
            tar.arg(&packed).arg("-C").arg(place).arg(name);
            tar
        };
        assert!(pack.status().expect("pack the download").success());
        let sum = spry::update::sha256(&packed).unwrap();
        std::fs::write(self.release.join(format!("{}.sha256", self.asset)), format!("{sum}  {}\n", self.asset))
            .unwrap();
        std::fs::write(self.release.join("latest.json"), format!(r#"{{"tag_name":"v{version}"}}"#)).unwrap();
    }

    /// The installed Spry with its own settings folder and port, looking at the test's release folder.
    /// `as_version`: the version it takes itself for when it compares.
    fn spry(&self, args: &[&str], as_version: Option<&str>) -> Command {
        let folder = self.release.to_string_lossy().replace('\\', "/");
        let from = if folder.starts_with('/') { format!("file://{folder}") } else { format!("file:///{folder}") };
        let mut cmd = Command::new(&self.program);
        cmd.args(args)
            .env("SPRY_HOME", self.dir.join("home"))
            .env("SPRY_PORT", self.port.to_string())
            .env("SPRY_UPDATE_FROM", from)
            .env_remove("SPRY_UPDATE_AS")
            .stdin(Stdio::null());
        if let Some(version) = as_version {
            cmd.env("SPRY_UPDATE_AS", version);
        }
        cmd
    }

    fn update(&self, as_version: Option<&str>) -> Output {
        self.spry(&["update"], as_version).output().expect("run spry update")
    }

    /// (status code, body) from the Spry that runs on this test's port.
    fn http(&self, method: &str, path: &str) -> Option<(u16, String)> {
        let token = std::fs::read_to_string(self.dir.join("home/token")).ok()?;
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).ok()?;
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
        let request = format!(
            "{method} {path}?t={token} HTTP/1.0\r\nHost: 127.0.0.1:{}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            self.port
        );
        stream.write_all(request.as_bytes()).ok()?;
        let mut reply = String::new();
        stream.read_to_string(&mut reply).ok()?;
        let (head, body) = reply.split_once("\r\n\r\n")?;
        Some((head.split_whitespace().nth(1)?.parse().ok()?, body.to_string()))
    }

    /// The process id of the Spry that answers on this test's port.
    fn running(&self) -> Option<u64> {
        let (code, body) = self.http("GET", "/api/status")?;
        let status: serde_json::Value = serde_json::from_str(&body).ok()?;
        (code == 200).then(|| status["spry_pid"].as_u64()).flatten()
    }

    /// Nothing of an update is left next to the installed Spry.
    fn nothing_left(&self) -> Vec<String> {
        let place = self.target.parent().unwrap();
        let name = self.target.file_name().unwrap().to_string_lossy().to_string();
        std::fs::read_dir(place)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
            .filter(|found| *found != name)
            .collect()
    }
}

impl Drop for Installed {
    fn drop(&mut self) {
        // The Spry that started again is not this test's child: it is asked to quit.
        let _ = self.http("POST", "/api/quit");
        let asked = Instant::now();
        while self.running().is_some() && asked.elapsed() < Duration::from_secs(10) {
            sleep(Duration::from_millis(200));
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

struct Kill(Child);

impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_until(what: &str, secs: u64, mut done: impl FnMut() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(start.elapsed() < Duration::from_secs(secs), "timed out waiting until {what}");
        sleep(Duration::from_millis(200));
    }
}

fn text(output: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
}

/// Which file is at this place: after an update it is another one, also when its content is the same.
#[cfg(unix)]
fn identity(path: &Path) -> u64 {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).unwrap().ino()
}

fn replaces_itself_and_starts_again(name: &str, as_app: bool) {
    let installed = Installed::new(name, as_app);
    #[cfg(unix)]
    let before = identity(&installed.program);

    // An older Spry is running.
    let old = installed.spry(&["run", "--no-tray"], Some("0.0.1")).stdout(Stdio::null()).spawn().expect("start spry");
    let mut old = Kill(old);
    wait_until("the installed Spry answers", 20, || installed.running().is_some());
    assert_eq!(installed.running(), Some(u64::from(old.0.id())));

    // `spry update` puts the announced version in its place ...
    let update = installed.update(Some("0.0.1"));
    assert!(update.status.success(), "spry update failed: {}", text(&update));
    assert!(text(&update).contains(&format!("Spry {VERSION} is in the place of")), "{}", text(&update));
    #[cfg(unix)]
    assert_ne!(identity(&installed.program), before, "the program file is another one now");

    // ... and the Spry that was running ends and another one answers at the same place.
    wait_until("the old Spry has ended", 20, || matches!(old.0.try_wait(), Ok(Some(_))));
    wait_until("the new Spry answers", 20, || installed.running().is_some());
    assert_ne!(installed.running(), Some(u64::from(old.0.id())), "another process answers now");
    let version = installed.spry(&["version"], None).output().unwrap();
    assert_eq!(text(&version).trim(), format!("spry {VERSION}"));
    // Windows cannot delete the old program while it still runs; it goes at the next start.
    let left: Vec<String> =
        installed.nothing_left().into_iter().filter(|name| !cfg!(windows) || name != "spry.exe.old").collect();
    assert!(left.is_empty(), "left next to Spry after the update: {left:?}");
}

#[test]
fn an_older_spry_replaces_itself_with_the_newer_download_and_starts_again() {
    let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    replaces_itself_and_starts_again("plain", false);
}

#[cfg(target_os = "macos")]
#[test]
fn on_a_mac_the_whole_app_is_replaced() {
    let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    replaces_itself_and_starts_again("app", true);
}

#[test]
fn a_download_that_is_changed_or_not_the_announced_version_changes_nothing() {
    let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let installed = Installed::new("refused", false);
    let untouched = spry::update::sha256(Path::new(SPRY)).unwrap();
    let unchanged = |after: &str| {
        assert_eq!(spry::update::sha256(&installed.program).unwrap(), untouched, "the program changed {after}");
        assert_eq!(installed.nothing_left(), Vec::<String>::new(), "left next to Spry {after}");
    };

    // The newest version is the one that is installed: nothing to do.
    let same = installed.update(None);
    assert!(same.status.success() && text(&same).contains("is the newest"), "{}", text(&same));
    unchanged("when it was the newest already");

    // The release announces a version, but the download is another one.
    installed.publish("99.0.0");
    let other = installed.update(None);
    assert!(!other.status.success(), "a download of another version was accepted: {}", text(&other));
    assert!(text(&other).contains("is not a working Spry 99.0.0"), "{}", text(&other));
    unchanged("after a download of another version");

    // The download was changed after its checksum was published.
    let packed = installed.release.join(installed.asset);
    let mut bytes = std::fs::read(&packed).unwrap();
    let middle = bytes.len() / 2;
    bytes[middle] ^= 1;
    std::fs::write(&packed, bytes).unwrap();
    let changed = installed.update(None);
    assert!(!changed.status.success(), "a changed download was accepted: {}", text(&changed));
    assert!(text(&changed).contains("does not match its published checksum"), "{}", text(&changed));
    unchanged("after a changed download");

    // No checksum published at all.
    std::fs::remove_file(installed.release.join(format!("{}.sha256", installed.asset))).unwrap();
    let unchecked = installed.update(None);
    assert!(!unchecked.status.success(), "a download without a checksum was accepted: {}", text(&unchecked));
    unchanged("after a download without a checksum");
}

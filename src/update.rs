//! Keeping Spry current. Once a day Spry asks GitHub for the newest release. With "Update Spry
//! automatically" ticked (the default) a newer one is downloaded, compared with the checksum
//! published next to it, started once to see that it is the announced version, put in the place
//! of this one, and Spry starts again as the new version. Unticked, Spry only tells.
//! Nothing is replaced unless every step before it held.

use crate::config::VERSION;
use crate::engine::Shared;
use crate::platform;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

const NEWEST: &str = "https://api.github.com/repos/juw12/spry/releases/latest";
const DOWNLOADS: &str = "https://github.com/juw12/spry/releases/download";
/// The folder next to the program in which a download is checked before anything is replaced.
const STAGING: &str = ".spry-update";

/// Where releases come from. `SPRY_UPDATE_FROM` names another place that holds `latest.json` and
/// the downloads (a mirror, or the folder a test made). Set but empty, Spry never looks.
enum Source {
    GitHub,
    Other(String),
    Nowhere,
}

fn source() -> Source {
    match std::env::var("SPRY_UPDATE_FROM") {
        Err(_) => Source::GitHub,
        Ok(from) if from.trim().is_empty() => Source::Nowhere,
        Ok(from) => Source::Other(from.trim().trim_end_matches('/').to_string()),
    }
}

/// The version this Spry compares the newest one with. `SPRY_UPDATE_AS` lets a test stand in
/// for an older Spry; what `spry version` says never changes.
fn current() -> String {
    std::env::var("SPRY_UPDATE_AS").unwrap_or_else(|_| VERSION.to_string())
}

/// "0.6.1" as numbers. Anything that is not digits and dots is not a version Spry acts on.
fn numbers(version: &str) -> Option<Vec<u32>> {
    if !version.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    let mut parts = version.split('.').map(|part| part.parse().ok()).collect::<Option<Vec<u32>>>()?;
    if parts.len() > 4 {
        return None;
    }
    parts.resize(4, 0);
    Some(parts)
}

pub fn newer(latest: &str, current: &str) -> bool {
    matches!((numbers(latest), numbers(current)), (Some(latest), Some(current)) if latest > current)
}

/// curl and tar as they come with the system. On Windows the ones in System32 by their full
/// path: a `tar` from elsewhere reads `C:` as the name of another computer.
fn tool(name: &str) -> std::process::Command {
    if cfg!(windows) {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        platform::command(PathBuf::from(root).join("System32").join(format!("{name}.exe")))
    } else {
        platform::command(name)
    }
}

/// The newest released version, or None when it cannot be found out (no connection, no curl).
pub fn latest() -> Option<String> {
    let url = match source() {
        Source::GitHub => NEWEST.to_string(),
        Source::Other(from) => format!("{from}/latest.json"),
        Source::Nowhere => return None,
    };
    let out = tool("curl").args(["-fsSL", "-m", "15"]).arg(url).stdin(Stdio::null()).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let release: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let version = release["tag_name"].as_str()?.trim_start_matches('v').to_string();
    numbers(&version).map(|_| version)
}

/// The newest version when it is newer than this Spry.
pub fn available() -> Option<String> {
    latest().filter(|latest| newer(latest, &current()))
}

fn download(url: &str, to: &Path) -> bool {
    let mut curl = tool("curl");
    curl.args(["-fsSL", "--retry", "2", "-m", "300", "-o"]).arg(to).arg(url).stdin(Stdio::null());
    curl.status().is_ok_and(|status| status.success())
}

pub fn sha256(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

/// The checksum in a published `.sha256` file ("<64 hex digits>  <file name>").
fn published(text: &str) -> Option<String> {
    let sum = text.split_whitespace().next()?.to_lowercase();
    (sum.len() == 64 && sum.chars().all(|c| c.is_ascii_hexdigit())).then_some(sum)
}

/// The program file of this Spry, as it was when Spry started. After an update the new version
/// is at the same place. (Linux names a replaced program "... (deleted)" when asked later.)
pub fn exe() -> PathBuf {
    static EXE: OnceLock<PathBuf> = OnceLock::new();
    EXE.get_or_init(|| {
        let exe = std::env::current_exe().unwrap_or_default();
        // A link to the program (the `spry` command on a Mac points into Spry.app) leads to the real file.
        if cfg!(unix) { exe.canonicalize().unwrap_or(exe) } else { exe }
    })
    .clone()
}

/// On a Mac Spry lives in an app (Spry.app/Contents/MacOS/spry). Then the whole app is replaced.
fn bundle(exe: &Path) -> Option<PathBuf> {
    let inside = exe.parent()?;
    let app = inside.parent()?.parent()?;
    let is_app = cfg!(target_os = "macos")
        && inside.ends_with("Contents/MacOS")
        && app.extension().is_some_and(|extension| extension == "app");
    is_app.then(|| app.to_path_buf())
}

/// The download for this system, by the name it has on the release page.
pub fn asset(exe: &Path) -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Some("spry-windows-x64.zip"),
        ("macos", _) if bundle(exe).is_some() => Some("Spry-macos.zip"),
        ("macos", _) => Some("spry-macos-universal.tar.gz"),
        ("linux", "x86_64") => Some("spry-linux-x64.tar.gz"),
        ("linux", "aarch64") => Some("spry-linux-arm64.tar.gz"),
        _ => None,
    }
}

/// Can this Spry replace itself? False where no ready-made download exists for the system.
pub fn possible() -> bool {
    !matches!(source(), Source::Nowhere) && asset(&exe()).is_some()
}

fn remove(path: &Path) {
    if std::fs::remove_file(path).is_err() {
        let _ = std::fs::remove_dir_all(path);
    }
}

/// Where the version before this one waits to be deleted: next to the program, with ".old" added.
fn aside(target: &Path) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(".old");
    target.with_file_name(name)
}

/// What the program answers to `version`, without the leading "spry ". None when it does not run.
fn said_version(program: &Path) -> Option<String> {
    let mut child = platform::command(program)
        .arg("version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = Instant::now();
    while matches!(child.try_wait(), Ok(None)) {
        if started.elapsed() > Duration::from_secs(15) {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut text = String::new();
    child.stdout.take()?.read_to_string(&mut text).ok()?;
    text.trim().strip_prefix("spry ").map(str::to_string)
}

/// Clears the version an update left next to the program. Windows does not delete a program that
/// still runs, so the old one stays until the next start.
pub fn tidy() {
    let exe = exe();
    remove(&aside(&bundle(&exe).unwrap_or(exe)));
}

/// Downloads `version`, checks it, and puts it in the place of this Spry. The running Spry goes on
/// as the old version until it starts again. On any error nothing was changed.
pub fn apply(version: &str) -> Result<(), String> {
    let exe = exe();
    let asset = asset(&exe).ok_or("there is no ready-made Spry for this system")?;
    let from = match source() {
        Source::GitHub => format!("{DOWNLOADS}/v{version}"),
        Source::Other(from) => from,
        Source::Nowhere => return Err("looking for a newer Spry is switched off (SPRY_UPDATE_FROM is empty)".into()),
    };
    let target = bundle(&exe).unwrap_or_else(|| exe.clone());
    let folder = target.parent().ok_or("Spry does not know its own folder")?;

    // The staging folder is also the lock: one update at a time. One that was left behind is cleared.
    let staging = folder.join(STAGING);
    if let Ok(left) = std::fs::metadata(&staging) {
        let age = left.modified().ok().and_then(|at| at.elapsed().ok());
        if age.is_some_and(|age| age < Duration::from_secs(600)) {
            return Err("another update of Spry is under way".into());
        }
        remove(&staging);
    }
    std::fs::create_dir(&staging).map_err(|e| format!("Spry cannot write to its folder {} ({e})", folder.display()))?;
    let done = replace(version, asset, &from, &target, &staging);
    remove(&staging);
    done
}

fn replace(version: &str, asset: &str, from: &str, target: &Path, staging: &Path) -> Result<(), String> {
    let file = staging.join(asset);
    let sum = staging.join(format!("{asset}.sha256"));
    if !download(&format!("{from}/{asset}"), &file) || !download(&format!("{from}/{asset}.sha256"), &sum) {
        return Err(format!("Spry {version} could not be downloaded"));
    }
    // The download must match the checksum published next to it, or nothing is replaced.
    let want = std::fs::read_to_string(&sum).ok().and_then(|text| published(&text));
    if want.is_none() || want != sha256(&file).ok() {
        return Err("the download does not match its published checksum, so nothing was changed".into());
    }

    let unpacked = staging.join("new");
    std::fs::create_dir(&unpacked).map_err(|e| e.to_string())?;
    let is_app = asset == "Spry-macos.zip";
    let unpack = if is_app {
        platform::command("ditto").args(["-x", "-k"]).arg(&file).arg(&unpacked).status()
    } else {
        tool("tar").arg("-xf").arg(&file).arg("-C").arg(&unpacked).status()
    };
    if !unpack.is_ok_and(|status| status.success()) {
        return Err("the download could not be unpacked, so nothing was changed".into());
    }

    // What takes the place of `target`, and the program inside it.
    let fresh = unpacked.join(if is_app {
        "Spry.app"
    } else if cfg!(windows) {
        "spry.exe"
    } else {
        "spry"
    });
    let program = if is_app { fresh.join("Contents/MacOS/spry") } else { fresh.clone() };
    // It has to run on this computer and be the version that was announced.
    if said_version(&program).as_deref() != Some(version) {
        return Err(format!("the download is not a working Spry {version}, so nothing was changed"));
    }

    let old = aside(target);
    remove(&old);
    std::fs::rename(target, &old).map_err(|e| format!("the Spry that is there could not be moved aside ({e})"))?;
    if let Err(e) = std::fs::rename(&fresh, target) {
        let _ = std::fs::rename(&old, target);
        return Err(format!("the new Spry could not be put in place ({e}); the old one is back"));
    }
    remove(&old);
    Ok(())
}

/// Puts the newer version in place and has this Spry start again as it. What went wrong stays for the page.
pub fn install(shared: &Shared, version: &str) -> Result<(), String> {
    let done = apply(version);
    *shared.update_error.lock().unwrap() = done.clone().err();
    if done.is_ok() {
        restart(shared);
    }
    done
}

/// Quits this Spry the usual way (every app is put back) and starts the program again afterwards.
pub fn restart(shared: &Shared) {
    shared.restart.store(true, Ordering::Relaxed);
    shared.quit.store(true, Ordering::Relaxed);
}

/// What the next Spry is started with. The command comes first and by name: 0.6.0 left it out
/// when the window was open, the next Spry took the flag for a command it did not know, printed
/// the help and ended, so after an update Spry was gone.
fn again(show: bool, tray: bool) -> Vec<&'static str> {
    let mut args = vec!["run"];
    if show {
        args.push("--show");
    }
    if !tray {
        args.push("--no-tray");
    }
    // This Spry still holds the port for a moment; the next one is told to wait for it.
    args.push("--after-update");
    args
}

/// The last thing a quitting Spry does: when a restart was asked for, it starts the program
/// at its place, which is the new version after an update.
pub fn relaunch(shared: &Shared, tray: bool) {
    if !shared.restart.load(Ordering::Relaxed) {
        return;
    }
    // With its window open a moment ago Spry comes back with the window; otherwise quietly.
    let watched = shared.watched.lock().unwrap().is_some_and(|at| at.elapsed() < Duration::from_secs(10));
    let mut next = platform::command(exe());
    next.args(again(watched && shared.window.load(Ordering::Relaxed), tray));
    next.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Its own session, so it is not ended together with the Spry that started it.
        unsafe {
            next.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }
    let _ = next.spawn();
}

/// Half a minute after the start and then once a day: is there a newer Spry?
pub fn watch(shared: Arc<Shared>) {
    if matches!(source(), Source::Nowhere) {
        return;
    }
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(30));
        loop {
            if let Some(version) = available() {
                *shared.update.lock().unwrap() = Some(version.clone());
                let automatic = shared.cfg.lock().unwrap().auto_update;
                if automatic && possible() && install(&shared, &version).is_ok() {
                    return;
                }
            }
            std::thread::sleep(Duration::from_secs(24 * 3600));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_higher_version_is_newer_and_anything_odd_is_not() {
        assert!(newer("0.6.1", "0.6.0"));
        assert!(newer("0.10.0", "0.9.9"));
        assert!(newer("1.0", "0.99.99"));
        assert!(newer("0.6.0.1", "0.6.0"));
        assert!(!newer("0.6.0", "0.6.0"));
        assert!(!newer("0.6", "0.6.0"));
        assert!(!newer("0.5.9", "0.6.0"));
        for odd in ["", "latest", "0.7.0-rc1", "+1.0.0", "1..0", "0.7.0/../x", "1.2.3.4.5"] {
            assert!(!newer(odd, "0.6.0"), "{odd:?} must not count as a newer version");
        }
    }

    #[test]
    fn the_next_spry_is_always_started_with_the_run_command() {
        assert_eq!(again(false, true), ["run", "--after-update"]);
        assert_eq!(again(true, true), ["run", "--show", "--after-update"]);
        assert_eq!(again(false, false), ["run", "--no-tray", "--after-update"]);
        assert_eq!(again(true, false), ["run", "--show", "--no-tray", "--after-update"]);
    }

    #[test]
    fn a_published_checksum_is_64_hex_digits_before_the_file_name() {
        let sum = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        assert_eq!(published(&format!("{sum}  spry-linux-x64.tar.gz\n")).as_deref(), Some(sum));
        assert_eq!(published(&sum.to_uppercase()).as_deref(), Some(sum));
        assert_eq!(published("not found"), None);
        assert_eq!(published(""), None);
        assert_eq!(published(&sum[..63]), None);
    }

    #[test]
    fn the_checksum_of_a_file_is_its_sha_256() {
        let file = std::env::temp_dir().join(format!("spry-sha-{}", std::process::id()));
        std::fs::write(&file, "abc").unwrap();
        let sum = sha256(&file).unwrap();
        let _ = std::fs::remove_file(&file);
        assert_eq!(sum, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn every_system_with_a_download_gets_the_one_built_for_it() {
        let plain = Path::new("/home/me/.local/bin/spry");
        let in_app = Path::new("/Applications/Spry.app/Contents/MacOS/spry");
        let name = asset(plain);
        match (std::env::consts::OS, std::env::consts::ARCH) {
            ("windows", "x86_64") => assert_eq!(name, Some("spry-windows-x64.zip")),
            ("linux", "x86_64") => assert_eq!(name, Some("spry-linux-x64.tar.gz")),
            ("linux", "aarch64") => assert_eq!(name, Some("spry-linux-arm64.tar.gz")),
            ("macos", _) => {
                assert_eq!(name, Some("spry-macos-universal.tar.gz"));
                assert_eq!(asset(in_app), Some("Spry-macos.zip"));
                assert_eq!(bundle(in_app).as_deref(), Some(Path::new("/Applications/Spry.app")));
            }
            _ => assert_eq!(name, None),
        }
        assert_eq!(bundle(plain), None);
        assert_eq!(aside(Path::new("/Applications/Spry.app")), Path::new("/Applications/Spry.app.old"));
        assert_eq!(aside(Path::new("spry.exe")), Path::new("spry.exe.old"));
    }
}

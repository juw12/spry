//! Settings, file locations and the built-in lists of apps Spry never touches.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Local dashboard port. Binding it is also the single-instance lock.
pub fn port() -> u16 {
    std::env::var("SPRY_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(47633)
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Config {
    pub enabled: bool,
    /// A background process counts as busy above this share of one CPU core (%).
    pub hog_percent: f32,
    /// Whole-machine CPU use (%) that counts as "under pressure".
    pub pressure_cpu: f32,
    /// Free memory (%) below which background apps are asked to give memory back.
    pub pressure_mem_free: f32,
    /// Under pressure, also pause-and-resume the heaviest background processes in short pulses.
    pub strong: bool,
    /// While the app in front rests, held apps run at full speed.
    pub lend: bool,
    pub trim_memory: bool,
    pub freeze_after_secs: u64,
    pub never_slow: Vec<String>,
    pub always_slow: Vec<String>,
    pub freeze: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            enabled: true,
            hog_percent: 10.0,
            pressure_cpu: 80.0,
            pressure_mem_free: 15.0,
            // Since part of the processor is kept free under pressure, pulses add next to
            // nothing (2.36x without, 2.37x with on Linux) and cost held apps most of their time.
            strong: false,
            lend: true,
            trim_memory: true,
            freeze_after_secs: 30,
            never_slow: vec![],
            always_slow: vec![],
            freeze: vec![],
        }
    }
}

impl Config {
    /// Clamp numbers and tidy the app lists (a name lives in one list only).
    pub fn sanitize(mut self) -> Self {
        self.hog_percent = self.hog_percent.clamp(1.0, 100.0);
        self.pressure_cpu = self.pressure_cpu.clamp(30.0, 99.0);
        self.pressure_mem_free = self.pressure_mem_free.clamp(2.0, 60.0);
        self.freeze_after_secs = self.freeze_after_secs.clamp(5, 3600);
        let tidy = |v: &mut Vec<String>| {
            let mut out: Vec<String> = Vec::new();
            for n in v.iter().map(|s| norm(s)).filter(|s| !s.is_empty() && !is_protected(s)) {
                if !out.contains(&n) {
                    out.push(n);
                }
            }
            *v = out;
        };
        tidy(&mut self.never_slow);
        tidy(&mut self.always_slow);
        tidy(&mut self.freeze);
        let never = self.never_slow.clone();
        self.freeze.retain(|n| !never.contains(n));
        let taken: Vec<String> = never.iter().chain(self.freeze.iter()).cloned().collect();
        self.always_slow.retain(|n| !taken.contains(n));
        self
    }

    /// Returns the settings and whether this is the first run (no settings file yet).
    pub fn load() -> (Self, bool) {
        match std::fs::read_to_string(data_dir().join("config.json")) {
            Ok(text) => (serde_json::from_str::<Config>(&text).unwrap_or_default().sanitize(), false),
            Err(_) => (Config::default(), true),
        }
    }

    pub fn save(&self) {
        let dir = data_dir();
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(dir.join("config.json"), text);
        }
    }
}

pub fn home() -> PathBuf {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    PathBuf::from(std::env::var_os(var).unwrap_or_default())
}

/// Where Spry keeps its settings, its token and the crash-recovery list.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("SPRY_HOME") {
        return PathBuf::from(dir);
    }
    if cfg!(windows) {
        PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default()).join("Spry")
    } else if cfg!(target_os = "macos") {
        home().join("Library/Application Support/Spry")
    } else {
        match std::env::var_os("XDG_CONFIG_HOME") {
            Some(x) => PathBuf::from(x).join("spry"),
            None => home().join(".config/spry"),
        }
    }
}

/// Secret that the dashboard must present. Made once, kept for later runs.
pub fn token() -> String {
    let path = data_dir().join("token");
    if let Ok(t) = std::fs::read_to_string(&path) {
        let t = t.trim().to_string();
        if t.len() >= 32 {
            return t;
        }
    }
    use std::hash::{BuildHasher, Hasher};
    let mut t = String::new();
    for _ in 0..2 {
        // RandomState is seeded from the operating system's random source.
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u32(std::process::id());
        t.push_str(&format!("{:016x}", h.finish()));
    }
    let _ = std::fs::create_dir_all(data_dir());
    let _ = std::fs::write(&path, &t);
    t
}

/// Process names are compared in lower case, without ".exe".
pub fn norm(name: &str) -> String {
    let n = name.trim().to_lowercase();
    n.strip_suffix(".exe").unwrap_or(&n).to_string()
}

/// Does the rule name cover this process name? Also covers macOS helper processes
/// ("zoom.us helper (renderer)") and Linux names cut to 15 characters.
pub fn matches(rule: &str, name: &str) -> bool {
    name == rule
        || (name.len() > rule.len() && name.starts_with(rule) && name[rule.len()..].starts_with(" helper"))
        || (name.len() == 15 && rule.len() > 15 && rule.starts_with(name))
}

pub fn matches_any(rules: &[String], name: &str) -> bool {
    rules.iter().any(|r| matches(r, name))
}

/// The desktop itself, sound, input, security, and apps where a stutter is not acceptable
/// (calls, music, recording). Spry never slows these.
const PROTECTED: &[&str] = &[
    "spry",
    // Windows
    "system",
    "registry",
    "smss",
    "csrss",
    "wininit",
    "winlogon",
    "services",
    "lsass",
    "svchost",
    "dwm",
    "explorer",
    "sihost",
    "taskhostw",
    "ctfmon",
    "fontdrvhost",
    "searchhost",
    "startmenuexperiencehost",
    "shellexperiencehost",
    "textinputhost",
    "runtimebroker",
    "audiodg",
    "conhost",
    "taskmgr",
    "lockapp",
    "applicationframehost",
    "dllhost",
    "wudfhost",
    "spoolsv",
    "msmpeng",
    "securityhealthservice",
    "securityhealthsystray",
    "openconsole",
    "windowsterminal",
    // macOS
    "kernel_task",
    "launchd",
    "windowserver",
    "loginwindow",
    "dock",
    "finder",
    "systemuiserver",
    "controlcenter",
    "notificationcenter",
    "coreaudiod",
    "corespeechd",
    "cfprefsd",
    "distnoted",
    "usernoted",
    "sharingd",
    "bluetoothd",
    "powerd",
    "textinputmenuagent",
    "universalcontrol",
    "activity monitor",
    "screencaptureui",
    "airplayuiagent",
    "terminal",
    "iterm2",
    // Linux
    "systemd",
    "init",
    "xorg",
    "xwayland",
    "gnome-shell",
    "plasmashell",
    "kwin_x11",
    "kwin_wayland",
    "mutter",
    "sway",
    "hyprland",
    "pipewire",
    "pipewire-pulse",
    "wireplumber",
    "pulseaudio",
    "dbus-daemon",
    "dbus-broker",
    "ibus-daemon",
    "fcitx5",
    "gdm",
    "sddm",
    "lightdm",
    "gnome-session-binary",
    "xdg-desktop-portal",
    "gnome-terminal-server",
    "konsole",
    "xterm",
    // Calls, music, recording
    "zoom",
    "zoom.us",
    "teams",
    "ms-teams",
    "msteams",
    "discord",
    "skype",
    "webex",
    "facetime",
    "slack",
    "obs",
    "obs64",
    "obs-studio",
    "spotify",
    "music",
    "vlc",
    "mpv",
    "quicktime player",
    "wmplayer",
    "itunes",
    "tidal",
];

/// True when the program file is part of the operating system. Spry leaves these alone
/// whatever their name is, so a system helper it has never heard of is still safe.
/// Apps that ship with the system (Photos, Mail, Safari and its page processes) are not
/// counted: they are apps, and a busy one in the background is what Spry is for.
pub fn is_system_path(path: &std::path::Path) -> bool {
    let path = path.to_string_lossy().replace('\\', "/").to_lowercase();
    if cfg!(windows) {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
        let root = root.replace('\\', "/").to_lowercase();
        return path.strip_prefix(root.trim_end_matches('/')).is_some_and(|rest| rest.starts_with('/'));
    }
    let roots: &[&str] = if cfg!(target_os = "macos") {
        &["/system/", "/usr/libexec/", "/usr/sbin/", "/sbin/", "/library/apple/"]
    } else {
        &["/usr/lib/systemd/", "/lib/systemd/", "/usr/libexec/", "/usr/sbin/", "/sbin/"]
    };
    let an_app = path.contains("/applications/") || path.contains("/webkit.framework/");
    roots.iter().any(|root| path.starts_with(root)) && !an_app
}

pub fn is_protected(name: &str) -> bool {
    PROTECTED.iter().any(|r| matches(r, name))
}

/// Programs that start other apps. An app's "family" stops below these.
const LAUNCHERS: &[&str] = &[
    "explorer",
    "sihost",
    "svchost",
    "services",
    "wininit",
    "winlogon",
    "userinit",
    "system",
    "launchd",
    "loginwindow",
    "finder",
    "dock",
    "systemd",
    "init",
    "gnome-shell",
    "plasmashell",
    "kdeinit5",
    "gnome-session-binary",
    "xfce4-session",
    "dbus-daemon",
    "sshd",
    "tmux",
    "screen",
];

pub fn is_launcher(name: &str) -> bool {
    LAUNCHERS.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_normalised_and_matched() {
        assert_eq!(norm(" Chrome.EXE "), "chrome");
        assert!(matches("zoom.us", "zoom.us helper (renderer)"));
        assert!(matches("chromium-browser", "chromium-browse"));
        assert!(!matches("code", "codex"));
        assert!(is_protected("discord helper (gpu)"));
        assert!(!is_protected("chrome"));
    }

    #[test]
    fn programs_that_belong_to_the_system_are_recognised_by_where_they_live() {
        let is = |path: &str| is_system_path(std::path::Path::new(path));
        if cfg!(windows) {
            let root = std::env::var("SystemRoot").unwrap();
            assert!(is(&format!("{root}\\System32\\SearchProtocolHost.exe")));
            assert!(is(&format!("{}\\SystemApps\\x\\SearchApp.exe", root.to_uppercase())));
            assert!(!is(&format!("{root}Old\\thing.exe")));
            assert!(!is("C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe"));
        } else if cfg!(target_os = "macos") {
            assert!(is("/System/Library/CoreServices/iconservicesagent"));
            assert!(is("/usr/libexec/secinitd"));
            assert!(!is("/System/Applications/Photos.app/Contents/MacOS/Photos"));
            assert!(!is("/System/Volumes/Preboot/Cryptexes/App/System/Applications/Safari.app/Contents/MacOS/Safari"));
            assert!(!is(
                "/System/Library/Frameworks/WebKit.framework/Versions/A/XPCServices/com.apple.WebKit.WebContent.xpc/Contents/MacOS/com.apple.WebKit.WebContent"
            ));
            assert!(!is("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"));
            assert!(!is("/Users/me/.local/bin/spry"));
        } else {
            assert!(is("/usr/lib/systemd/systemd"));
            assert!(is("/usr/libexec/xdg-desktop-portal"));
            assert!(!is("/usr/bin/python3"));
            assert!(!is("/opt/google/chrome/chrome"));
            assert!(!is("/home/me/.local/bin/spry"));
        }
    }

    #[test]
    fn sanitize_keeps_a_name_in_one_list_and_drops_protected() {
        let c = Config {
            hog_percent: 0.0,
            never_slow: vec!["Code.exe".into(), "code".into()],
            freeze: vec!["code".into(), "chrome".into(), "explorer".into()],
            always_slow: vec!["chrome".into(), "ollama".into()],
            ..Config::default()
        }
        .sanitize();
        assert_eq!(c.hog_percent, 1.0);
        assert_eq!(c.never_slow, vec!["code"]);
        assert_eq!(c.freeze, vec!["chrome"]);
        assert_eq!(c.always_slow, vec!["ollama"]);
    }
}

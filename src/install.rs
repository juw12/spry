//! "Start with the computer": a startup entry for the current user, plus an app-menu entry.
//! Nothing needs administrator rights and `uninstall` removes every file again.

use crate::config::home;
use std::io;
use std::path::PathBuf;

/// (file that starts Spry at sign-in, app-menu entry if the system has one)
fn files() -> (PathBuf, Option<PathBuf>) {
    if cfg!(windows) {
        let programs = PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default())
            .join("Microsoft/Windows/Start Menu/Programs");
        (programs.join("Startup/Spry.lnk"), Some(programs.join("Spry.lnk")))
    } else if cfg!(target_os = "macos") {
        (home().join("Library/LaunchAgents/dev.juw12.spry.plist"), None)
    } else {
        let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".config"));
        (config.join("autostart/spry.desktop"), Some(home().join(".local/share/applications/spry.desktop")))
    }
}

pub fn is_installed() -> bool {
    files().0.exists()
}

pub fn uninstall() -> io::Result<()> {
    let (startup, menu) = files();
    for file in [Some(startup), menu].into_iter().flatten() {
        match std::fs::remove_file(&file) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    Ok(())
}

pub fn install() -> io::Result<String> {
    let exe = std::env::current_exe()?;
    let exe = exe.to_string_lossy();
    let (startup, menu) = files();
    for file in [Some(&startup), menu.as_ref()].into_iter().flatten() {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
    }

    #[cfg(windows)]
    {
        let quote = |s: &str| s.replace('\'', "''");
        let link = |path: &PathBuf, args: &str| {
            format!(
                "$s=$w.CreateShortcut('{}');$s.TargetPath='{}';$s.Arguments='{args}';$s.Description='Spry keeps this computer quick';$s.Save();",
                quote(&path.to_string_lossy()),
                quote(&exe)
            )
        };
        let script = format!(
            "$w=New-Object -ComObject WScript.Shell;{}{}",
            link(&startup, "run"),
            link(menu.as_ref().unwrap(), "")
        );
        let out = crate::platform::command("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output()?;
        if !out.status.success() || !startup.exists() {
            return Err(io::Error::other(String::from_utf8_lossy(&out.stderr).trim().to_string()));
        }
    }

    #[cfg(target_os = "macos")]
    {
        let path = exe.replace('&', "&amp;").replace('<', "&lt;");
        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>dev.juw12.spry</string>
<key>ProgramArguments</key><array><string>{path}</string><string>run</string></array>
<key>RunAtLoad</key><true/>
<key>LimitLoadToSessionType</key><string>Aqua</string>
<key>ProcessType</key><string>Interactive</string>
</dict></plist>
"#
        );
        std::fs::write(&startup, plist)?;
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let entry = |args: &str| {
            format!(
                "[Desktop Entry]\nType=Application\nName=Spry\nComment=Keeps this computer quick\nExec=\"{exe}\"{args}\nTerminal=false\nCategories=Utility;System;\n"
            )
        };
        std::fs::write(&startup, entry(" run"))?;
        std::fs::write(menu.as_ref().unwrap(), entry(""))?;
    }

    Ok(format!("Spry will start when you sign in ({}).", startup.display()))
}

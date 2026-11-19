//! Storage: shows what can be cleared safely (caches, temporary files, the bin) and clears
//! only what the user picks. Nothing here touches documents, downloads or app data.

use crate::config::home;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

#[derive(Serialize, Clone, Debug)]
pub struct Category {
    pub id: &'static str,
    pub label: &'static str,
    pub note: &'static str,
    pub bytes: u64,
    pub files: u64,
}

struct Spec {
    id: &'static str,
    label: &'static str,
    note: &'static str,
    roots: Vec<PathBuf>,
    /// Only files untouched for this long are counted and removed.
    min_age: Duration,
}

const DAY: Duration = Duration::from_secs(24 * 3600);

fn env(var: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(var).unwrap_or_default())
}

fn specs() -> Vec<Spec> {
    let h = home();
    let spec = |id, label, note, roots: Vec<PathBuf>, min_age| Spec { id, label, note, roots, min_age };
    let mut list = Vec::new();
    if cfg!(windows) {
        let local = env("LOCALAPPDATA");
        list.push(spec(
            "temp",
            "Temporary files",
            "Older than one day. Apps make new ones as needed.",
            vec![std::env::temp_dir()],
            DAY,
        ));
        list.push(spec(
            "browser",
            "Browser caches",
            "Chrome and Edge. Pages load a little slower the first time afterwards. No passwords or history.",
            vec![
                local.join("Google/Chrome/User Data/Default/Cache"),
                local.join("Google/Chrome/User Data/Default/Code Cache"),
                local.join("Microsoft/Edge/User Data/Default/Cache"),
                local.join("Microsoft/Edge/User Data/Default/Code Cache"),
            ],
            Duration::ZERO,
        ));
        list.push(spec(
            "crash",
            "Crash reports",
            "Old crash dumps from apps.",
            vec![local.join("CrashDumps")],
            Duration::ZERO,
        ));
        list.push(spec(
            "dev",
            "Developer caches",
            "npm and pip download caches. They are downloaded again when needed.",
            vec![local.join("npm-cache/_cacache"), local.join("pip/Cache")],
            Duration::ZERO,
        ));
    } else if cfg!(target_os = "macos") {
        list.push(spec(
            "caches",
            "App caches",
            "Everything in Library/Caches older than one day. Apps rebuild what they need.",
            vec![h.join("Library/Caches")],
            DAY,
        ));
        list.push(spec("logs", "Logs", "App log files older than one day.", vec![h.join("Library/Logs")], DAY));
        list.push(spec(
            "dev",
            "Developer caches",
            "Xcode build data and the npm cache. They are rebuilt or downloaded again when needed.",
            vec![h.join("Library/Developer/Xcode/DerivedData"), h.join(".npm/_cacache")],
            Duration::ZERO,
        ));
        list.push(spec("trash", "Bin", "Files you already threw away.", vec![h.join(".Trash")], Duration::ZERO));
    } else {
        let cache = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).unwrap_or_else(|| h.join(".cache"));
        list.push(spec(
            "caches",
            "App caches",
            "Everything in ~/.cache older than one day. Apps rebuild what they need.",
            vec![cache],
            DAY,
        ));
        list.push(spec(
            "dev",
            "Developer caches",
            "The npm cache. It is downloaded again when needed.",
            vec![h.join(".npm/_cacache")],
            Duration::ZERO,
        ));
        list.push(spec(
            "trash",
            "Bin",
            "Files you already threw away.",
            vec![h.join(".local/share/Trash/files"), h.join(".local/share/Trash/info")],
            Duration::ZERO,
        ));
    }
    list
}

/// A root is only accepted when it sits well inside the home folder, or is the user's own
/// temp folder (Windows often names that one by its short path, outside the home path).
fn safe_root(root: &Path) -> bool {
    let home = home();
    let in_home = home.components().count() >= 2 && root.starts_with(&home) && root != home;
    let deep = root.is_absolute() && root.components().count() >= 4;
    deep && (in_home || root.starts_with(std::env::temp_dir()))
}

/// Counts (and with `delete`, removes) old-enough files below `root`. Never follows links,
/// never removes `root` itself. Files in use simply stay.
pub fn sweep(root: &Path, min_age: Duration, delete: bool) -> (u64, u64) {
    let now = SystemTime::now();
    let (mut bytes, mut files) = (0u64, 0u64);
    let mut todo = vec![root.to_path_buf()];
    let mut dirs = Vec::new();
    while let Some(dir) = todo.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
            if meta.is_symlink() {
                continue;
            }
            if meta.is_dir() {
                dirs.push(path.clone());
                todo.push(path);
                continue;
            }
            let age = meta.modified().ok().and_then(|m| now.duration_since(m).ok()).unwrap_or_default();
            if age < min_age {
                continue;
            }
            if !delete || std::fs::remove_file(&path).is_ok() {
                bytes += meta.len();
                files += 1;
            }
        }
    }
    if delete {
        // Deepest first; a folder that still holds something refuses and stays.
        for dir in dirs.iter().rev() {
            let _ = std::fs::remove_dir(dir);
        }
    }
    (bytes, files)
}

fn run(only: Option<&[String]>, delete: bool) -> Vec<Category> {
    let wanted = |id: &str| only.is_none_or(|ids| ids.iter().any(|x| x == id));
    let mut out = Vec::new();
    for s in specs() {
        if !wanted(s.id) {
            continue;
        }
        let (mut bytes, mut files) = (0, 0);
        for root in s.roots.iter().filter(|r| safe_root(r)) {
            let (b, f) = sweep(root, s.min_age, delete);
            bytes += b;
            files += f;
        }
        out.push(Category { id: s.id, label: s.label, note: s.note, bytes, files });
    }
    #[cfg(windows)]
    if wanted("trash") {
        let (bytes, files) = crate::platform::recycle_bin();
        let done = !delete || files == 0 || crate::platform::empty_recycle_bin();
        out.push(Category {
            id: "trash",
            label: "Recycle Bin",
            note: "Files you already threw away.",
            bytes: if done { bytes } else { 0 },
            files: if done { files } else { 0 },
        });
    }
    out
}

/// What could be cleared, by category. Reads only.
pub fn scan() -> Vec<Category> {
    run(None, false)
}

/// Clears the chosen categories. The numbers returned are what was actually removed.
pub fn clean(ids: &[String]) -> Vec<Category> {
    run(Some(ids), true)
}

pub fn human(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit < 3 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn old(path: &Path, days: u64) {
        let when = SystemTime::now() - DAY * days as u32;
        fs::File::options().write(true).open(path).unwrap().set_modified(when).unwrap();
    }

    #[test]
    fn sweep_removes_only_old_files_and_keeps_the_root() {
        let root = std::env::temp_dir().join(format!("spry-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("sub/deep")).unwrap();
        fs::write(root.join("fresh.txt"), b"12345").unwrap();
        fs::write(root.join("sub/old.bin"), vec![0u8; 1000]).unwrap();
        fs::write(root.join("sub/deep/older.bin"), vec![0u8; 500]).unwrap();
        old(&root.join("sub/old.bin"), 3);
        old(&root.join("sub/deep/older.bin"), 9);

        assert_eq!(sweep(&root, DAY, false), (1500, 2), "a scan counts but removes nothing");
        assert!(root.join("sub/old.bin").exists());

        assert_eq!(sweep(&root, DAY, true), (1500, 2));
        assert!(root.join("fresh.txt").exists(), "new files stay");
        assert!(!root.join("sub").exists(), "emptied folders go");
        assert!(root.exists(), "the root stays");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn only_folders_well_inside_home_or_temp_are_accepted() {
        assert!(!safe_root(Path::new("/")));
        assert!(!safe_root(&home()));
        assert!(!safe_root(Path::new("/tmp")));
        assert!(!safe_root(Path::new("C:\\Windows")));
        if cfg!(windows) {
            assert!(safe_root(&std::env::temp_dir()), "the user's own temp folder can be cleaned");
        }
        assert!(!safe_root(Path::new("relative/cache")));
        if home().components().count() >= 3 {
            assert!(safe_root(&home().join(".cache")));
        }
        for s in specs() {
            for root in &s.roots {
                assert!(safe_root(root) || !root.exists(), "{root:?}");
            }
        }
    }

    #[test]
    fn no_cleaning_folder_is_inside_your_own_files() {
        let yours = ["Documents", "Downloads", "Desktop", "Pictures", "Music", "Movies", "Videos"];
        for s in specs() {
            for root in &s.roots {
                for folder in yours {
                    assert!(!root.starts_with(home().join(folder)), "{}: {root:?}", s.id);
                }
            }
        }
    }

    #[test]
    fn sizes_read_well() {
        assert_eq!(human(1536), "1.5 KB");
        assert_eq!(human(3 << 30), "3.0 GB");
    }
}

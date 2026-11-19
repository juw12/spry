//! A measured, local OFF trace. This is an opt-in timed release of holds, not a prediction of CPU use.
//! It neither changes the enabled flag nor borrows the user's pause timer.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const SETTLE: Duration = Duration::from_secs(5);
const END: Duration = Duration::from_secs(65);

#[derive(Serialize, Deserialize)]
struct Record {
    at: u64,
    // Milliseconds from the recording start and whole-machine CPU percentage (one fresh sample per engine reading).
    points: Vec<(u64, f32)>,
}

impl Record {
    fn complete(&self) -> bool {
        self.points.len() >= 20
            && self.points.first().is_some_and(|p| (5000..=10000).contains(&p.0))
            && self.points.last().is_some_and(|p| (60000..=65000).contains(&p.0))
            && self.points.windows(2).all(|p| p[0].0 < p[1].0 && p[1].0 - p[0].0 <= 6000)
            && self.points.iter().all(|p| p.1.is_finite() && (0.0..=100.0).contains(&p.1))
    }
}

pub struct Capture {
    file: PathBuf,
    since: Option<Instant>,
    points: Vec<(u64, f32)>,
    last: Option<Record>,
    message: &'static str,
}

impl Capture {
    pub fn load(folder: PathBuf) -> Self {
        let file = folder.join("cpu-off-reference.json");
        let last = std::fs::read(&file).ok().and_then(|bytes| serde_json::from_slice::<Record>(&bytes).ok()).filter(Record::complete);
        Self { file, since: None, points: Vec::new(), last, message: "" }
    }

    pub fn active(&self) -> bool {
        self.since.is_some()
    }

    pub fn start(&mut self, now: Instant) -> bool {
        if self.active() {
            return false;
        }
        self.points.clear();
        self.message = "";
        self.since = Some(now);
        true
    }

    pub fn cancel(&mut self) {
        self.since = None;
        self.points.clear();
        self.message = "Recording cancelled.";
    }

    pub fn advance(&mut self, now: Instant, cpu: Option<f32>) {
        let Some(start) = self.since else { return };
        let elapsed = now.saturating_duration_since(start);
        if elapsed >= SETTLE && elapsed <= END && let Some(value) = cpu {
            self.points.push((elapsed.as_millis() as u64, value));
        }
        if elapsed < END {
            return;
        }
        // End the temporary override even on sleep, incomplete data or a disk error.
        self.since = None;
        let at = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        let record = Record { at, points: std::mem::take(&mut self.points) };
        if elapsed > END + Duration::from_secs(5) || !record.complete() {
            self.message = "Incomplete recording. Try again while the computer is awake.";
            return;
        }
        let temp = self.file.with_extension("tmp");
        let saved = serde_json::to_vec(&record).ok().is_some_and(|bytes| {
            std::fs::write(&temp, bytes).and_then(|()| std::fs::rename(&temp, &self.file)).is_ok()
        });
        self.last = Some(record);
        self.message = if saved { "" } else { "Reference available for this session only." };
    }

    pub fn status(&self, now: Instant) -> Value {
        let left = self.since.map(|s| END.saturating_sub(now.saturating_duration_since(s)).as_secs() + 1);
        json!({"recording": self.active(), "left": left, "last": self.last, "message": self.message})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_capture_expires_and_only_replaces_a_good_trace_with_another_complete_trace() {
        let folder = std::env::temp_dir().join(format!("spry-cpu-reference-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let start = Instant::now();
        let mut capture = Capture::load(folder.clone());
        assert!(capture.start(start));
        assert!(!capture.start(start));
        for second in 0..=65 {
            let value = (second % 2 == 0).then_some(37.0);
            capture.advance(start + Duration::from_secs(second), value);
        }
        assert!(!capture.active());
        let saved = Capture::load(folder.clone()).status(start)["last"].clone();
        assert_eq!(saved["points"][0], json!([6000, 37.0]));
        assert_eq!(saved["points"].as_array().unwrap().len(), 30);
        assert!(capture.start(start));
        capture.advance(start + Duration::from_secs(3600), None);
        assert!(!capture.active());
        assert_eq!(capture.status(start)["last"], saved);
        assert!(capture.start(start));
        capture.cancel();
        assert_eq!(capture.status(start)["last"], saved);
        assert!(!capture.active());
        std::fs::remove_dir_all(folder).unwrap();
    }
}

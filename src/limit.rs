//! The load limit: how hard the whole computer may be worked, and for how long. Pure logic.
//!
//! Pressure (policy.rs) is about a moment: the processor is busy right now, so the app in front
//! goes first. The limit is about a stretch of time. A computer that runs at 80% for days is
//! never under pressure for long and never rests either, and it is the days that make the heat.
//! So Spry keeps the processor's average use, minute by minute, over the last hour. When that
//! average reaches the limit, busy apps are held the deeper way until it has fallen again
//! (see `policy::decide`), whatever mode Spry is in and whether or not anyone is at the computer.
//!
//! Spry reads no temperature. It limits the work that makes the heat.

use serde::Serialize;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Once over, the average has to fall this far below the limit before it counts as rested.
pub const REST_POINTS: f32 = 5.0;
/// The stretch is judged once readings cover this share of it: an hour's average needs most of an hour.
const COVERED: f32 = 0.75;
/// No reading for this long: the computer slept or Spry stood still. What came before is forgotten.
const GAP: Duration = Duration::from_secs(180);
const MINUTE: Duration = Duration::from_secs(60);
/// The longest stretch a limit can name, in minutes.
pub const MAX_MINUTES: u64 = 240;

/// What the page and the policy are told.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct View {
    /// Over the limit right now: busy apps are held until the computer has rested.
    pub over: bool,
    /// The processor's average use over the stretch, in percent. None before the first reading.
    pub average: Option<f32>,
    /// How many minutes of the stretch have been seen so far.
    pub minutes: u64,
    /// For how long it has been over, in seconds.
    pub over_secs: u64,
}

#[derive(Default)]
pub struct Limit {
    /// One entry per minute, the oldest first: the sum of that minute's readings and their number.
    minutes: VecDeque<(f32, u32)>,
    minute_began: Option<Instant>,
    last: Option<Instant>,
    over_since: Option<Instant>,
}

impl Limit {
    /// Add one reading of the whole processor (percent). `window` is the stretch in minutes.
    pub fn feed(&mut self, now: Instant, cpu_total: f32, window: u64) {
        if self.last.is_some_and(|last| now.duration_since(last) >= GAP) {
            *self = Limit::default();
        }
        self.last = Some(now);
        let same_minute = self.minute_began.is_some_and(|began| now.duration_since(began) < MINUTE);
        let open = if same_minute { self.minutes.back_mut() } else { None };
        match open {
            Some(minute) => {
                minute.0 += cpu_total;
                minute.1 += 1;
            }
            None => {
                self.minutes.push_back((cpu_total, 1));
                self.minute_began = Some(now);
            }
        }
        let keep = window.clamp(1, MAX_MINUTES) as usize;
        while self.minutes.len() > keep {
            self.minutes.pop_front();
        }
    }

    /// The mean of the minutes kept. Every minute counts the same, however often it was read.
    pub fn average(&self) -> Option<f32> {
        if self.minutes.is_empty() {
            return None;
        }
        let sum: f32 = self.minutes.iter().map(|(sum, count)| sum / *count as f32).sum();
        Some(sum / self.minutes.len() as f32)
    }

    /// Is the computer over `limit_cpu` (percent, averaged over `window` minutes)? Remembers the
    /// answer: once over, it stays over until the average is `REST_POINTS` under the limit.
    pub fn judge(&mut self, now: Instant, limit_cpu: f32, window: u64) -> View {
        let window = window.clamp(1, MAX_MINUTES);
        // Readings stopped (sleep, a long pause): nothing is claimed from an old average.
        let stale = self.last.is_none_or(|last| now.duration_since(last) >= GAP);
        let average = if stale { None } else { self.average() };
        let covered = self.minutes.len() as f32 >= COVERED * window as f32;
        let bar = if self.over_since.is_some() { limit_cpu - REST_POINTS } else { limit_cpu };
        let over = covered && average.is_some_and(|average| average >= bar);
        if !over {
            self.over_since = None;
        } else if self.over_since.is_none() {
            self.over_since = Some(now);
        }
        View {
            over,
            average,
            minutes: if stale { 0 } else { self.minutes.len() as u64 },
            over_secs: self.over_since.map_or(0, |since| now.duration_since(since).as_secs()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One reading every 4 seconds, as the engine takes them, for `minutes` minutes.
    fn work(limit: &mut Limit, from: Instant, minutes: u64, cpu: f32, window: u64) -> Instant {
        let mut now = from;
        for _ in 0..minutes * 15 {
            limit.feed(now, cpu, window);
            now += Duration::from_secs(4);
        }
        now
    }

    #[test]
    fn an_hour_at_eighty_percent_is_over_the_limit() {
        let mut limit = Limit::default();
        let start = Instant::now();
        let now = work(&mut limit, start, 60, 80.0, 60);
        let view = limit.judge(now, 75.0, 60);
        assert!(view.over);
        assert_eq!(view.minutes, 60);
        assert!((view.average.unwrap() - 80.0).abs() < 0.01);
    }

    #[test]
    fn a_busy_quarter_of_an_hour_is_not() {
        let mut limit = Limit::default();
        let start = Instant::now();
        let now = work(&mut limit, start, 45, 20.0, 60);
        let now = work(&mut limit, now, 15, 100.0, 60);
        let view = limit.judge(now, 75.0, 60);
        assert!(!view.over, "a build may use the whole processor: the limit is about the stretch");
        assert!((view.average.unwrap() - 40.0).abs() < 0.01);
    }

    #[test]
    fn it_is_not_judged_before_most_of_the_stretch_was_seen() {
        let mut limit = Limit::default();
        let start = Instant::now();
        let now = work(&mut limit, start, 44, 100.0, 60);
        assert!(!limit.judge(now, 75.0, 60).over);
        let now = work(&mut limit, now, 1, 100.0, 60);
        assert!(limit.judge(now, 75.0, 60).over);
    }

    #[test]
    fn once_over_it_has_to_fall_five_points_under_to_count_as_rested() {
        let mut limit = Limit::default();
        let start = Instant::now();
        let now = work(&mut limit, start, 60, 80.0, 60);
        assert!(limit.judge(now, 75.0, 60).over);
        // An hour at 72%: under the limit, but not rested.
        let now = work(&mut limit, now, 60, 72.0, 60);
        let view = limit.judge(now, 75.0, 60);
        assert!(view.over);
        assert!(view.over_secs >= 3600, "counted from when it went over");
        // An hour at 69%: rested.
        let now = work(&mut limit, now, 60, 69.0, 60);
        let view = limit.judge(now, 75.0, 60);
        assert!(!view.over);
        assert_eq!(view.over_secs, 0);
        // And 72% alone was never over.
        let mut fresh = Limit::default();
        let now = work(&mut fresh, start, 60, 72.0, 60);
        assert!(!fresh.judge(now, 75.0, 60).over);
    }

    #[test]
    fn every_minute_counts_the_same_however_often_it_was_read() {
        let mut limit = Limit::default();
        let start = Instant::now();
        // One minute read 30 times at 100%, one minute read once at 0%.
        for i in 0..30 {
            limit.feed(start + Duration::from_secs(2 * i), 100.0, 60);
        }
        limit.feed(start + Duration::from_secs(61), 0.0, 60);
        assert!((limit.average().unwrap() - 50.0).abs() < 0.01);
    }

    #[test]
    fn only_the_stretch_is_kept() {
        let mut limit = Limit::default();
        let start = Instant::now();
        let now = work(&mut limit, start, 60, 100.0, 30);
        let now = work(&mut limit, now, 30, 10.0, 30);
        let view = limit.judge(now, 75.0, 30);
        assert_eq!(view.minutes, 30);
        assert!((view.average.unwrap() - 10.0).abs() < 0.01);
        assert!(!view.over);
    }

    #[test]
    fn a_computer_that_slept_starts_fresh() {
        let mut limit = Limit::default();
        let start = Instant::now();
        let now = work(&mut limit, start, 60, 90.0, 60);
        assert!(limit.judge(now, 75.0, 60).over);
        // Nothing is claimed while no reading comes in...
        let later = now + Duration::from_secs(3 * 3600);
        let asleep = limit.judge(later, 75.0, 60);
        assert_eq!(asleep, View::default());
        // ...and one busy reading after three hours of sleep is not an hour of work.
        limit.feed(later, 95.0, 60);
        let view = limit.judge(later, 75.0, 60);
        assert!(!view.over);
        assert_eq!(view.minutes, 1);
    }

    #[test]
    fn before_the_first_reading_nothing_is_known() {
        let mut limit = Limit::default();
        assert_eq!(limit.judge(Instant::now(), 75.0, 60), View::default());
    }
}

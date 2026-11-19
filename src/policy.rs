//! The decision: which background processes to hold back, and how. Pure logic, no system calls.

use crate::config::{Config, is_launcher, is_protected, matches_any};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct Proc {
    pub pid: u32,
    pub ppid: Option<u32>,
    /// Lower case, without ".exe".
    pub name: String,
    /// Share of one CPU core in percent, smoothed.
    pub cpu: f32,
    /// The same over the last reading alone, not smoothed.
    pub cpu_now: f32,
    pub mem: u64,
    /// Owned by the user who runs Spry.
    pub mine: bool,
    pub start: u64,
    /// The program file belongs to the operating system (see `config::is_system_path`).
    pub system: bool,
}

#[derive(Clone, Default)]
pub struct Snapshot {
    pub procs: Vec<Proc>,
    /// Process that owns the window in front. None = not known on this desktop.
    pub fg: Option<u32>,
    /// A fresher reading of the app in front (share of one core in percent), taken twice a
    /// second while held apps are lent its idle time.
    pub front_now: Option<f32>,
    pub cpu_total: f32,
    pub mem_free_pct: f32,
    pub mem_total: u64,
    pub self_pid: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// Would be held, but the app in front rests: runs at full speed until it works again.
    Lent,
    /// Lower priority: gets the CPU only when the app in front does not need it.
    Ease,
    /// Eased, and also paused and resumed in short pulses.
    Pulse,
    /// Fully paused until it comes to the front again.
    Freeze,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Rule {
    Auto,
    Never,
    Always,
    Freeze,
}

#[derive(Default)]
pub struct PolicyState {
    last_fg: HashMap<u32, Instant>,
    bg_since: HashMap<u32, Instant>,
    last_trim: HashMap<u32, Instant>,
    sticky: HashSet<u32>,
    pulsed: HashSet<u32>,
    high_since: Option<Instant>,
    calm_since: Option<Instant>,
    front: Option<u32>,
    rest_since: Option<Instant>,
    /// What the app in front, all of it, used at the last reading: share of one core in percent.
    pub front_cpu: f32,
    pub pressure: bool,
}

impl PolicyState {
    /// How long the app in front has been resting. None while it works.
    pub fn rested(&self, now: Instant) -> Option<Duration> {
        self.rest_since.map(|since| now.duration_since(since))
    }
}

#[derive(Default)]
pub struct Plan {
    pub levels: HashMap<u32, Level>,
    pub trim: Vec<u32>,
    pub family: HashSet<u32>,
    /// Machine is under CPU pressure: use the deeper form of easing.
    pub deep: bool,
}

/// An app you just left keeps full speed this long.
const GRACE: Duration = Duration::from_secs(10);
const PRESSURE_ON: Duration = Duration::from_secs(4);
const PRESSURE_OFF: Duration = Duration::from_secs(10);
const TRIM_MIN_BYTES: u64 = 150 * 1024 * 1024;
const TRIM_GRACE: Duration = Duration::from_secs(60);
const TRIM_AGAIN: Duration = Duration::from_secs(300);
const PULSE_CPU: f32 = 50.0;
/// The app in front rests while all of it together uses less than this share of one core...
const FRONT_RESTS_BELOW: f32 = 5.0;
/// ...for this long. Then nothing is gained by holding the others back.
pub const LEND_AFTER: Duration = Duration::from_secs(6);
/// On the half-second reading a short burst counts as working again.
const FRONT_WAKES_AT: f32 = 15.0;

struct Tree<'a> {
    by_pid: HashMap<u32, &'a Proc>,
    kids: HashMap<u32, Vec<u32>>,
}

impl<'a> Tree<'a> {
    fn new(procs: &'a [Proc]) -> Self {
        let by_pid: HashMap<u32, &Proc> = procs.iter().map(|p| (p.pid, p)).collect();
        let mut tree = Tree { by_pid, kids: HashMap::new() };
        for p in procs {
            if let Some(parent) = tree.parent(p) {
                tree.kids.entry(parent.pid).or_default().push(p.pid);
            }
        }
        tree
    }

    /// The real parent. A parent that started after its child is a reused process id.
    fn parent(&self, p: &Proc) -> Option<&'a Proc> {
        let parent = *self.by_pid.get(&p.ppid?)?;
        (parent.pid != p.pid && parent.start <= p.start).then_some(parent)
    }

    /// Parent inside the same app: stops at launchers, system processes and other users.
    fn app_parent(&self, p: &Proc) -> Option<&'a Proc> {
        let parent = self.parent(p)?;
        (parent.pid > 1 && parent.mine && !is_launcher(&parent.name)).then_some(parent)
    }

    fn descendants(&self, root: u32) -> HashSet<u32> {
        let mut seen = HashSet::from([root]);
        let mut todo = vec![root];
        while let Some(pid) = todo.pop() {
            for kid in self.kids.get(&pid).into_iter().flatten() {
                if seen.insert(*kid) {
                    todo.push(*kid);
                }
            }
        }
        seen
    }

    /// The app in front: its top process and everything that process started.
    fn family(&self, fg: Option<u32>) -> HashSet<u32> {
        let Some(fg) = fg else { return HashSet::new() };
        let Some(mut root) = self.by_pid.get(&fg).copied() else { return HashSet::from([fg]) };
        if is_launcher(&root.name) {
            // The desktop itself is in front. Its children are every app, so they do not count.
            return HashSet::from([fg]);
        }
        for _ in 0..32 {
            match self.app_parent(root) {
                Some(parent) => root = parent,
                None => break,
            }
        }
        self.descendants(root.pid)
    }

    /// The user's rule for a process: its own name first, then the app that started it.
    fn rule(&self, cfg: &Config, p: &Proc) -> Rule {
        let mut node = Some(p);
        for _ in 0..32 {
            let Some(n) = node else { break };
            if matches_any(&cfg.never_slow, &n.name) {
                return Rule::Never;
            }
            if matches_any(&cfg.freeze, &n.name) {
                return Rule::Freeze;
            }
            if matches_any(&cfg.always_slow, &n.name) {
                return Rule::Always;
            }
            node = self.app_parent(n);
        }
        Rule::Auto
    }
}

pub fn rule_for_name(cfg: &Config, name: &str) -> Rule {
    if matches_any(&cfg.never_slow, name) {
        Rule::Never
    } else if matches_any(&cfg.freeze, name) {
        Rule::Freeze
    } else if matches_any(&cfg.always_slow, name) {
        Rule::Always
    } else {
        Rule::Auto
    }
}

pub fn decide(cfg: &Config, snap: &Snapshot, st: &mut PolicyState, now: Instant) -> Plan {
    // CPU pressure, with a delay both ways so short spikes change nothing.
    if snap.cpu_total >= cfg.pressure_cpu {
        st.calm_since = None;
        if now.duration_since(*st.high_since.get_or_insert(now)) >= PRESSURE_ON {
            st.pressure = true;
        }
    } else {
        st.high_since = None;
        if snap.cpu_total < cfg.pressure_cpu - 15.0 {
            if now.duration_since(*st.calm_since.get_or_insert(now)) >= PRESSURE_OFF {
                st.pressure = false;
                st.pulsed.clear();
            }
        } else {
            st.calm_since = None;
        }
    }

    let tree = Tree::new(&snap.procs);
    let family = tree.family(snap.fg);
    let own = tree.descendants(snap.self_pid);
    for pid in &family {
        st.last_fg.insert(*pid, now);
    }
    let alive = |pid: &u32| tree.by_pid.contains_key(pid);
    st.last_fg.retain(|p, _| alive(p));
    st.bg_since.retain(|p, _| alive(p));
    st.last_trim.retain(|p, _| alive(p));
    st.sticky.retain(alive);
    st.pulsed.retain(alive);

    let fg_known = snap.fg.is_some();
    let mem_tight = snap.mem_free_pct < cfg.pressure_mem_free;

    // Does the app in front need the processor right now? While it rests, holding the others
    // back gains nothing, so they are lent the idle time. Switching apps counts as working.
    let front_cpu: f32 = snap.procs.iter().filter(|p| family.contains(&p.pid)).map(|p| p.cpu_now).sum();
    let works = front_cpu >= FRONT_RESTS_BELOW || snap.front_now.is_some_and(|cpu| cpu >= FRONT_WAKES_AT);
    let rests = fg_known && !works && st.front == snap.fg;
    st.front = snap.fg;
    st.front_cpu = front_cpu;
    if !rests {
        st.rest_since = None;
    }
    let lend = cfg.lend && rests && now.duration_since(*st.rest_since.get_or_insert(now)) >= LEND_AFTER;

    let mut plan = Plan { family, deep: st.pressure, ..Plan::default() };

    for p in &snap.procs {
        if !p.mine || p.pid <= 1 || plan.family.contains(&p.pid) || own.contains(&p.pid) {
            st.sticky.remove(&p.pid);
            st.pulsed.remove(&p.pid);
            st.bg_since.remove(&p.pid);
            continue;
        }
        if is_protected(&p.name) || p.system {
            continue;
        }
        let rule = tree.rule(cfg, p);
        if rule == Rule::Never {
            continue;
        }
        let since_fg = st.last_fg.get(&p.pid).map(|t| now.duration_since(*t));
        let just_left = since_fg.is_some_and(|d| d < GRACE);
        let bg_for = now.duration_since(*st.bg_since.entry(p.pid).or_insert(now));

        if rule == Rule::Freeze && fg_known {
            // Without a known front app Spry could not wake it again, so it never freezes then.
            if bg_for >= Duration::from_secs(cfg.freeze_after_secs) {
                plan.levels.insert(p.pid, Level::Freeze);
            }
            continue;
        }

        let busy = fg_known && p.cpu >= cfg.hog_percent && !just_left;
        if rule == Rule::Always || busy || (fg_known && st.sticky.contains(&p.pid)) {
            st.sticky.insert(p.pid);
            let pulse = cfg.strong && st.pressure && (p.cpu >= PULSE_CPU || st.pulsed.contains(&p.pid));
            if pulse {
                st.pulsed.insert(p.pid);
            }
            // A rule set by name ("always hold back") is kept whatever the app in front does.
            let level = if lend && rule != Rule::Always {
                Level::Lent
            } else if pulse {
                Level::Pulse
            } else {
                Level::Ease
            };
            plan.levels.insert(p.pid, level);
        }

        if cfg.trim_memory
            && mem_tight
            && fg_known
            && p.mem >= TRIM_MIN_BYTES
            && !since_fg.is_some_and(|d| d < TRIM_GRACE)
            && !st.last_trim.get(&p.pid).is_some_and(|t| now.duration_since(*t) < TRIM_AGAIN)
        {
            st.last_trim.insert(p.pid, now);
            plan.trim.push(p.pid);
        }
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: u32, ppid: u32, name: &str, cpu: f32) -> Proc {
        Proc {
            pid,
            ppid: Some(ppid),
            name: name.into(),
            cpu,
            cpu_now: cpu,
            mem: 200 << 20,
            mine: true,
            start: pid as u64,
            system: false,
        }
    }

    /// explorer(10) -> editor(20) -> helper(21); explorer -> browser(30) -> tab(31); spry(40)
    fn desk(fg: Option<u32>) -> Snapshot {
        Snapshot {
            procs: vec![
                p(10, 1, "explorer", 1.0),
                p(20, 10, "editor", 80.0),
                p(21, 20, "node", 60.0),
                p(30, 10, "browser", 5.0),
                p(31, 30, "browser", 90.0),
                p(40, 10, "spry", 0.1),
            ],
            fg,
            front_now: None,
            cpu_total: 40.0,
            mem_free_pct: 50.0,
            mem_total: 8 << 30,
            self_pid: 40,
        }
    }

    fn run(cfg: &Config, snap: &Snapshot) -> Plan {
        decide(cfg, snap, &mut PolicyState::default(), Instant::now())
    }

    #[test]
    fn busy_background_is_eased_and_the_front_app_is_not() {
        let plan = run(&Config::default(), &desk(Some(20)));
        assert_eq!(plan.levels.get(&31), Some(&Level::Ease));
        assert!(!plan.levels.contains_key(&30), "a quiet process is left alone");
        assert!(!plan.levels.contains_key(&20) && !plan.levels.contains_key(&21), "front app and its children");
        assert!(!plan.levels.contains_key(&10) && !plan.levels.contains_key(&40));
    }

    #[test]
    fn focus_on_a_child_window_covers_the_whole_app() {
        let plan = run(&Config::default(), &desk(Some(21)));
        assert!(plan.family.contains(&20) && plan.family.contains(&21));
        assert!(!plan.levels.contains_key(&20));
    }

    #[test]
    fn desktop_in_front_does_not_exempt_every_app() {
        let plan = run(&Config::default(), &desk(Some(10)));
        assert_eq!(plan.family.len(), 1);
        assert!(plan.levels.contains_key(&20) && plan.levels.contains_key(&31));
    }

    #[test]
    fn unknown_front_app_means_rules_only() {
        let mut cfg = Config::default();
        assert!(run(&cfg, &desk(None)).levels.is_empty(), "never guess which app is in use");
        cfg.always_slow = vec!["browser".into()];
        cfg.freeze = vec!["editor".into()];
        let mut st = PolicyState::default();
        let t0 = Instant::now();
        decide(&cfg, &desk(None), &mut st, t0);
        let plan = decide(&cfg, &desk(None), &mut st, t0 + Duration::from_secs(600));
        assert_eq!(plan.levels.get(&30), Some(&Level::Ease));
        assert_eq!(plan.levels.get(&31), Some(&Level::Ease));
        assert!(!plan.levels.contains_key(&20), "no freeze when it could not be woken on focus");
    }

    #[test]
    fn never_slow_and_protected_are_left_alone() {
        let cfg = Config { never_slow: vec!["browser".into()], ..Config::default() };
        let mut snap = desk(Some(20));
        snap.procs.push(p(50, 10, "zoom", 95.0));
        snap.procs.push(p(51, 10, "other", 95.0));
        snap.procs.last_mut().unwrap().mine = false;
        let plan = run(&cfg, &snap);
        assert!(plan.levels.is_empty(), "{:?}", plan.levels);
    }

    #[test]
    fn rule_on_an_app_covers_what_it_started() {
        let cfg = Config { never_slow: vec!["editor".into()], ..Config::default() };
        assert!(!run(&cfg, &desk(Some(30))).levels.contains_key(&21));
    }

    #[test]
    fn an_app_you_just_left_keeps_full_speed_for_a_moment() {
        let cfg = Config::default();
        let mut st = PolicyState::default();
        let t0 = Instant::now();
        decide(&cfg, &desk(Some(20)), &mut st, t0);
        let soon = decide(&cfg, &desk(Some(30)), &mut st, t0 + Duration::from_secs(3));
        assert!(!soon.levels.contains_key(&20));
        let later = decide(&cfg, &desk(Some(30)), &mut st, t0 + Duration::from_secs(13));
        assert_eq!(later.levels.get(&20), Some(&Level::Ease));
        assert_eq!(later.levels.get(&21), Some(&Level::Ease));
        let back = decide(&cfg, &desk(Some(20)), &mut st, t0 + Duration::from_secs(14));
        assert!(!back.levels.contains_key(&20) && !back.levels.contains_key(&21), "restored on focus");
    }

    #[test]
    fn freeze_waits_then_thaws_on_focus() {
        let cfg = Config { freeze: vec!["browser".into()], ..Config::default() };
        let mut st = PolicyState::default();
        let t0 = Instant::now();
        assert!(!decide(&cfg, &desk(Some(20)), &mut st, t0).levels.contains_key(&30));
        let frozen = decide(&cfg, &desk(Some(20)), &mut st, t0 + Duration::from_secs(31));
        assert_eq!(frozen.levels.get(&30), Some(&Level::Freeze));
        assert_eq!(frozen.levels.get(&31), Some(&Level::Freeze));
        let thawed = decide(&cfg, &desk(Some(31)), &mut st, t0 + Duration::from_secs(32));
        assert!(!thawed.levels.contains_key(&30) && !thawed.levels.contains_key(&31));
    }

    #[test]
    fn pulses_need_strong_mode_and_lasting_pressure() {
        let cfg = Config { strong: true, ..Config::default() };
        let mut snap = desk(Some(20));
        snap.cpu_total = 97.0;
        let mut st = PolicyState::default();
        let t0 = Instant::now();
        assert_eq!(decide(&cfg, &snap, &mut st, t0).levels.get(&31), Some(&Level::Ease), "a spike is not pressure");
        let hot = decide(&cfg, &snap, &mut st, t0 + Duration::from_secs(5));
        assert!(hot.deep);
        assert_eq!(hot.levels.get(&31), Some(&Level::Pulse));
        // The pulsed process now measures lower, and must stay pulsed while pressure lasts.
        snap.procs[4].cpu = 30.0;
        assert_eq!(decide(&cfg, &snap, &mut st, t0 + Duration::from_secs(7)).levels.get(&31), Some(&Level::Pulse));
        snap.cpu_total = 20.0;
        decide(&cfg, &snap, &mut st, t0 + Duration::from_secs(8));
        let calm = decide(&cfg, &snap, &mut st, t0 + Duration::from_secs(19));
        assert!(!calm.deep);
        assert_eq!(calm.levels.get(&31), Some(&Level::Ease));

        let cfg = Config { strong: false, ..Config::default() };
        snap.cpu_total = 97.0;
        let mut st = PolicyState::default();
        decide(&cfg, &snap, &mut st, t0);
        snap.procs[4].cpu = 90.0;
        assert_eq!(decide(&cfg, &snap, &mut st, t0 + Duration::from_secs(5)).levels.get(&31), Some(&Level::Ease));
    }

    #[test]
    fn memory_is_asked_back_only_when_tight_and_not_from_the_front_app() {
        let cfg = Config::default();
        let mut snap = desk(Some(20));
        assert!(run(&cfg, &snap).trim.is_empty());
        snap.mem_free_pct = 6.0;
        let mut st = PolicyState::default();
        let t0 = Instant::now();
        let mut trim = decide(&cfg, &snap, &mut st, t0).trim;
        trim.sort();
        assert_eq!(trim, vec![30, 31]);
        assert!(decide(&cfg, &snap, &mut st, t0 + Duration::from_secs(30)).trim.is_empty(), "not again so soon");
    }

    #[test]
    fn a_reused_process_id_is_not_taken_for_a_parent() {
        let mut snap = desk(Some(20));
        // Process 21 claims parent 20, but "20" started later: not really its parent.
        snap.procs[2].start = 5;
        let plan = run(&Config::default(), &snap);
        assert!(!plan.family.contains(&21));
        assert_eq!(plan.levels.get(&21), Some(&Level::Ease));
    }

    /// The desk with the editor (20, 21) in front and resting; the browser tab (31) stays busy.
    fn resting(front_cpu: f32) -> Snapshot {
        let mut snap = desk(Some(20));
        snap.procs[1].cpu_now = front_cpu;
        snap.procs[2].cpu_now = 0.0;
        snap
    }

    #[test]
    fn held_apps_are_lent_the_time_while_the_front_app_rests() {
        let cfg = Config { strong: true, ..Config::default() };
        let mut st = PolicyState::default();
        let t0 = Instant::now();
        let at = |secs: u64| t0 + Duration::from_secs(secs);
        let mut snap = resting(1.0);
        snap.cpu_total = 97.0;

        // The first look counts as a switch to this app; the rest is timed from the second.
        assert_eq!(decide(&cfg, &snap, &mut st, t0).levels.get(&31), Some(&Level::Ease), "not at once");
        assert_eq!(decide(&cfg, &snap, &mut st, at(4)).levels.get(&31), Some(&Level::Pulse), "pressure, strong mode");
        assert_eq!(decide(&cfg, &snap, &mut st, at(9)).levels.get(&31), Some(&Level::Pulse), "5 s of rest: not yet");
        let lent = decide(&cfg, &snap, &mut st, at(10));
        assert_eq!(lent.levels.get(&31), Some(&Level::Lent), "the front app rested for 6 s");
        assert!(lent.deep, "pressure is still noted, so the full hold is back at once");

        // It works again on the next reading: held at once, as hard as before.
        let mut hot = resting(40.0);
        hot.cpu_total = 97.0;
        assert_eq!(decide(&cfg, &hot, &mut st, at(12)).levels.get(&31), Some(&Level::Pulse));

        // Resting again starts the wait again.
        assert_eq!(decide(&cfg, &snap, &mut st, at(14)).levels.get(&31), Some(&Level::Pulse));
        assert_eq!(decide(&cfg, &snap, &mut st, at(20)).levels.get(&31), Some(&Level::Lent));
        // A short burst seen on the half-second reading ends it too.
        snap.front_now = Some(30.0);
        assert_eq!(decide(&cfg, &snap, &mut st, at(21)).levels.get(&31), Some(&Level::Pulse));
        // Below the wake-up mark the half-second reading changes nothing.
        snap.front_now = Some(8.0);
        decide(&cfg, &snap, &mut st, at(22));
        assert_eq!(decide(&cfg, &snap, &mut st, at(28)).levels.get(&31), Some(&Level::Lent));
    }

    #[test]
    fn switching_apps_counts_as_working() {
        let cfg = Config::default();
        let mut st = PolicyState::default();
        let t0 = Instant::now();
        let at = |secs: u64| t0 + Duration::from_secs(secs);
        let mut snap = resting(0.0);
        decide(&cfg, &snap, &mut st, t0);
        decide(&cfg, &snap, &mut st, at(1));
        assert_eq!(decide(&cfg, &snap, &mut st, at(7)).levels.get(&31), Some(&Level::Lent));
        // You click on the desktop. It is just as quiet, but you are clearly at the machine.
        snap.fg = Some(10);
        assert_eq!(decide(&cfg, &snap, &mut st, at(8)).levels.get(&31), Some(&Level::Ease));
        assert_eq!(decide(&cfg, &snap, &mut st, at(9)).levels.get(&31), Some(&Level::Ease));
        assert_eq!(decide(&cfg, &snap, &mut st, at(14)).levels.get(&31), Some(&Level::Ease));
        assert_eq!(decide(&cfg, &snap, &mut st, at(15)).levels.get(&31), Some(&Level::Lent));
    }

    #[test]
    fn nothing_is_lent_when_switched_off_by_rule_or_without_a_known_front_app() {
        let t0 = Instant::now();
        let later = t0 + Duration::from_secs(60);
        let lent = |cfg: &Config, snap: &Snapshot| {
            let mut st = PolicyState::default();
            decide(cfg, snap, &mut st, t0);
            decide(cfg, snap, &mut st, t0 + Duration::from_secs(1));
            decide(cfg, snap, &mut st, later).levels.get(&31).copied()
        };
        assert_eq!(lent(&Config::default(), &resting(0.0)), Some(Level::Lent));
        assert_eq!(lent(&Config { lend: false, ..Config::default() }, &resting(0.0)), Some(Level::Ease));
        let named = Config { always_slow: vec!["browser".into()], ..Config::default() };
        assert_eq!(lent(&named, &resting(0.0)), Some(Level::Ease), "a rule by name is kept");
        let mut blind = resting(0.0);
        blind.fg = None;
        assert_eq!(lent(&named, &blind), Some(Level::Ease), "front app unknown: nobody knows it rests");
        let frozen = Config { freeze: vec!["browser".into()], ..Config::default() };
        assert_eq!(lent(&frozen, &resting(0.0)), Some(Level::Freeze));
    }

    #[test]
    fn switched_off_rules_are_plain_auto() {
        let cfg = Config { freeze: vec!["a".into()], ..Config::default() };
        assert_eq!(rule_for_name(&cfg, "a"), Rule::Freeze);
        assert_eq!(rule_for_name(&cfg, "b"), Rule::Auto);
    }
}

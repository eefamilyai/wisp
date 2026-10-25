//! The queue of actions waiting for their moment.
//!
//! "Shut down at 9pm" is not a different command from "shut down" — it is the
//! same one, plus the sentence "at 9pm" said about it. So the queue stores the
//! *action* exactly as the runner would have run it, and the trigger separately.
//! Firing it later is the same call, made by the agent's scheduler thread.
//!
//! The file is `%APPDATA%\Wisp\pending.json`: the agent writes it, the settings
//! app reads and edits it, a text editor can too.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::brain::Action;
use crate::schedule::Stamp;
use crate::when::{Needs, Trigger, World};

/// How late a *moment* may be and still fire.
///
/// Beyond this the machine was asleep or off when the moment passed, and doing
/// it now is not what was asked: "shut down at 9" firing at six in the morning
/// would be worse than not firing at all. It is reported instead.
const GRACE_MINUTES: i64 = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pending {
    pub id: u64,
    pub trigger: Trigger,
    /// The actions, exactly as they would have run when they were spoken.
    pub actions: Vec<Action>,
    /// The person's own words for it ("shut down the pc"), so the queue reads
    /// like a note to self rather than a translation.
    pub label: String,
    pub created: Stamp,
    /// The last state an edge trigger observed, so "when I plug the charger in"
    /// means the change and not the status quo.
    #[serde(default)]
    pub last_seen: Option<bool>,
}

/// What to do with a queued action this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Not yet.
    Waiting,
    /// Run it now.
    Fire,
    /// The moment passed while the machine was away. Carries the minutes late.
    Missed(i64),
}

impl Pending {
    /// One tick: has this come true?
    pub fn poll(&mut self, world: &World) -> Verdict {
        let state = self.trigger.state(world);
        if self.trigger.is_moment() {
            let Some(stamp) = self.stamp() else { return Verdict::Waiting };
            let late = stamp.minutes_until(world.now);
            return if late > GRACE_MINUTES {
                Verdict::Missed(late)
            } else if state {
                Verdict::Fire
            } else {
                Verdict::Waiting
            };
        }
        if self.trigger.is_edge() {
            match self.last_seen {
                None => {
                    // First look: remember where we started. "When I unplug the
                    // charger" said while it is unplugged waits for the next one.
                    self.last_seen = Some(state);
                    Verdict::Waiting
                }
                Some(previous) if previous != state => {
                    self.last_seen = Some(state);
                    if state {
                        Verdict::Fire
                    } else {
                        Verdict::Waiting
                    }
                }
                Some(_) => Verdict::Waiting,
            }
        } else if state {
            Verdict::Fire
        } else {
            Verdict::Waiting
        }
    }

    fn stamp(&self) -> Option<Stamp> {
        match self.trigger {
            Trigger::At { stamp } => Some(stamp),
            _ => None,
        }
    }

    /// "shut down the pc at 9 pm" — the whole entry, said as a sentence.
    pub fn sentence(&self) -> String {
        format!("{} {}", self.label, self.trigger.describe())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Pendings {
    #[serde(default)]
    pub next_id: u64,
    #[serde(default)]
    pub items: Vec<Pending>,
}

impl Pendings {
    pub fn path() -> PathBuf {
        crate::paths::data_dir().join("pending.json")
    }

    /// The queue as it is on disk. A missing or broken file is an empty queue,
    /// not an error: a queue is not worth refusing to start over.
    pub fn open() -> Pendings {
        Pendings::open_at(&Pendings::path())
    }

    pub fn open_at(path: &Path) -> Pendings {
        let Ok(text) = std::fs::read_to_string(path) else { return Pendings::default() };
        match serde_json::from_str::<Pendings>(&text) {
            Ok(mut pendings) => {
                // An id has to be unique, whatever the file says.
                let highest = pendings.items.iter().map(|p| p.id).max().unwrap_or(0);
                pendings.next_id = pendings.next_id.max(highest + 1);
                pendings
            }
            Err(e) => {
                log::warn!("pending.json is not readable ({e}); treating the queue as empty");
                Pendings::default()
            }
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&Pendings::path())
    }

    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into());
        std::fs::write(path, text)
    }

    /// Queue one, returning its id.
    pub fn add(&mut self, trigger: Trigger, actions: Vec<Action>, label: String) -> u64 {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        self.items.push(Pending {
            id,
            trigger,
            actions,
            label,
            created: Stamp::now(),
            last_seen: None,
        });
        id
    }

    pub fn remove(&mut self, id: u64) -> Option<Pending> {
        let at = self.items.iter().position(|p| p.id == id)?;
        Some(self.items.remove(at))
    }

    /// Remove every entry whose label or trigger mentions this, or all of them
    /// when it is empty.
    pub fn cancel(&mut self, what: &str) -> Vec<Pending> {
        let needle = crate::fuzzy::squash(what);
        if needle.is_empty() {
            return std::mem::take(&mut self.items);
        }
        let (gone, kept): (Vec<Pending>, Vec<Pending>) = std::mem::take(&mut self.items)
            .into_iter()
            .partition(|p| crate::fuzzy::squash(&format!("{} {}", p.label, p.trigger.describe())).contains(&needle));
        self.items = kept;
        gone
    }

    /// What the queue has to look at this tick.
    pub fn needs(&self) -> Needs {
        self.items.iter().fold(Needs::default(), |needs, p| needs.also(p.trigger.needs()))
    }

    /// Poll every entry, keep the survivors, and hand back what fired and what
    /// was missed. The caller saves and acts.
    pub fn poll_all(&mut self, world: &World) -> (Vec<Pending>, Vec<(Pending, i64)>) {
        let mut fired = Vec::new();
        let mut missed = Vec::new();
        let mut kept = Vec::new();
        for mut entry in std::mem::take(&mut self.items) {
            match entry.poll(world) {
                Verdict::Waiting => kept.push(entry),
                Verdict::Fire => fired.push(entry),
                Verdict::Missed(late) => missed.push((entry, late)),
            }
        }
        self.items = kept;
        (fired, missed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::when::{Trigger, WorldState};
    use std::time::Duration;

    fn queue() -> Pendings {
        Pendings::default()
    }

    fn world() -> World {
        World::with(WorldState::default())
    }

    #[test]
    fn a_moment_fires_when_it_is_due_and_not_before() {
        let mut q = queue();
        q.add(Trigger::At { stamp: Stamp::now().add_minutes(30) }, Vec::new(), "shut down".into());
        let (fired, missed) = q.poll_all(&world());
        assert!(fired.is_empty() && missed.is_empty());
        assert_eq!(q.items.len(), 1);

        // Fast-forward by queueing a moment that has already passed.
        let mut q = queue();
        q.add(Trigger::At { stamp: Stamp::now().add_minutes(-1) }, Vec::new(), "lock the pc".into());
        let (fired, missed) = q.poll_all(&world());
        assert_eq!(fired.len(), 1, "a minute late is due");
        assert!(missed.is_empty());
        assert!(q.items.is_empty(), "a fired entry leaves the queue");
        assert_eq!(fired[0].sentence(), format!("lock the pc {}", fired[0].trigger.describe()));
    }

    /// A moment missed while the machine was off is reported, not run: doing it
    /// hours later is not what was asked.
    #[test]
    fn a_moment_missed_while_away_is_not_run_late() {
        let mut q = queue();
        q.add(Trigger::At { stamp: Stamp::now().add_minutes(-90) }, Vec::new(), "shut down".into());
        let (fired, missed) = q.poll_all(&world());
        assert!(fired.is_empty());
        assert_eq!(missed.len(), 1);
        assert!(missed[0].1 >= 89, "should report how late: {:?}", missed[0].1);
        assert!(q.items.is_empty());
    }

    /// A pinned process is a level: it was running when the person pointed at
    /// it, so gone means gone.
    #[test]
    fn a_pinned_application_fires_when_it_is_gone() {
        let mut q = queue();
        q.add(Trigger::AppCloses { app: String::new(), pid: Some(1) }, Vec::new(), "shut down".into());
        let running = World::with(WorldState { processes: vec![(1, "chrome".into())], ..Default::default() });
        assert!(q.poll_all(&running).0.is_empty());
        let closed = World::with(WorldState { processes: vec![(2, "code".into())], ..Default::default() });
        assert_eq!(q.poll_all(&closed).0.len(), 1);
    }

    /// A named app is an edge: it waits for the next time it closes, so saying
    /// "when spotify closes" with Spotify already closed does not fire at once.
    #[test]
    fn a_named_application_waits_for_the_next_close() {
        let mut q = queue();
        q.add(Trigger::AppCloses { app: "spotify".into(), pid: None }, Vec::new(), "close chrome".into());
        let not_running = World::with(WorldState { processes: vec![(1, "chrome".into())], ..Default::default() });
        // First look arms it, and it does not fire for the status quo.
        assert!(q.poll_all(&not_running).0.is_empty());
        assert!(q.poll_all(&not_running).0.is_empty());
        // It runs...
        let running = World::with(WorldState { processes: vec![(9, "spotify".into())], ..Default::default() });
        assert!(q.poll_all(&running).0.is_empty());
        // ...and closing it is the event.
        assert_eq!(q.poll_all(&not_running).0.len(), 1);
    }

    /// The edge case that would otherwise misfire on the very first tick.
    #[test]
    fn an_edge_waits_for_the_change() {
        let mut q = queue();
        q.add(Trigger::Unplugged, Vec::new(), "pause the music".into());
        let plugged = World::with(WorldState { battery: Some((80, true)), ..Default::default() });
        let unplugged = World::with(WorldState { battery: Some((80, false)), ..Default::default() });
        // First look arms it.
        assert!(q.poll_all(&plugged).0.is_empty());
        // Unplugging is the event.
        assert_eq!(q.poll_all(&unplugged).0.len(), 1);

        // And the other way round: said while already unplugged, it waits for
        // the next plug.
        let mut q = queue();
        q.add(Trigger::PluggedIn, Vec::new(), "open steam".into());
        assert!(q.poll_all(&unplugged).0.is_empty());
        assert!(q.poll_all(&unplugged).0.is_empty());
        assert_eq!(q.poll_all(&plugged).0.len(), 1);
    }

    #[test]
    fn the_queue_round_trips_through_disk() {
        let dir = std::env::temp_dir().join(format!("wisp-pending-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pending.json");
        let mut q = Pendings::open_at(&path);
        assert!(q.items.is_empty(), "a missing file is an empty queue");
        let id = q.add(
            Trigger::At { stamp: Stamp::now().add_minutes(10) },
            vec![Action::LockPc, Action::Machine { op: crate::machine::Op::BatteryStatus, args: Vec::new() }],
            "lock the pc".into(),
        );
        q.save_to(&path).unwrap();

        let mut again = Pendings::open_at(&path);
        assert_eq!(again.items.len(), 1);
        assert_eq!(again.items[0].id, id);
        assert_eq!(again.items[0].label, "lock the pc");
        assert_eq!(again.items[0].actions.len(), 2);
        // The next id must not collide with what is already there.
        let second = again.add(Trigger::Online, Vec::new(), "open chrome".into());
        assert_ne!(second, id);

        // Cancel by whatever the person called it.
        let gone = again.cancel("lock");
        assert_eq!(gone.len(), 1);
        assert_eq!(again.items.len(), 1);
        assert!(again.cancel("").len() == 1, "an empty filter clears the queue");

        // A broken file is an empty queue, not a crash.
        std::fs::write(&path, "{ this is not json").unwrap();
        assert!(Pendings::open_at(&path).items.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn needs_are_gathered_only_for_what_is_queued() {
        let mut q = queue();
        q.add(Trigger::At { stamp: Stamp::now().add_minutes(5) }, Vec::new(), "a".into());
        assert_eq!(q.needs(), Needs::default(), "a timer needs nothing but the clock");
        q.add(Trigger::Idle { minutes: 10 }, Vec::new(), "b".into());
        assert!(q.needs().idle);
        let mut q = queue();
        q.add(Trigger::Online, Vec::new(), "c".into());
        assert!(q.needs().network);
        q.add(Trigger::AppStarts { app: "steam".into() }, Vec::new(), "d".into());
        assert!(q.needs().processes);
    }

    /// The idle trigger is a level: it fires the first time it is observed.
    #[test]
    fn idle_fires_once_it_is_true() {
        let mut q = queue();
        q.add(Trigger::Idle { minutes: 10 }, Vec::new(), "lock the pc".into());
        let busy = World::with(WorldState { idle: Duration::from_secs(30), ..Default::default() });
        assert!(q.poll_all(&busy).0.is_empty());
        let away = World::with(WorldState { idle: Duration::from_secs(700), ..Default::default() });
        assert_eq!(q.poll_all(&away).0.len(), 1);
    }
}

//! Fires alarms, reminders and calendar events — and the queue of actions that
//! were spoken with a "when" attached ("shut down at 9pm", "close chrome when
//! spotify closes").
//!
//! One thread, one tick a second: it re-reads the list when the file changes
//! (the settings app may be editing it), announces anything that has come due,
//! and runs the function attached to it. It works whether or not the
//! microphone is paused — an alarm is not a listening feature.

use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};

use wisp_core::apps::AppIndex;
use wisp_core::brain::Action;
use wisp_core::pending::Pendings;
use wisp_core::when::World;
use wisp_core::schedule::{announcement, Item, ItemKind, Schedule, Stamp};
use wisp_core::{actions, personality, Config};

use crate::bubble::Mode;
use crate::overlay::Shared;
use crate::tts::Tts;

use wisp_core::actions::Speaker;

pub fn spawn(cfg: Config, shared: Arc<Shared>, apps: Arc<RwLock<AppIndex>>, tts: Arc<Tts>) {
    std::thread::Builder::new()
        .name("schedule".into())
        .spawn(move || run(cfg, shared, apps, tts))
        .expect("scheduler thread");
}

fn modified() -> Option<SystemTime> {
    std::fs::metadata(Schedule::path()).and_then(|m| m.modified()).ok()
}

fn modified_pending() -> Option<SystemTime> {
    std::fs::metadata(Pendings::path()).and_then(|m| m.modified()).ok()
}

/// How late an item may be and still be announced as if it just happened.
const ON_TIME_MINUTES: i64 = 5;

fn run(cfg: Config, shared: Arc<Shared>, apps: Arc<RwLock<AppIndex>>, tts: Arc<Tts>) {
    let voice = crate::tts::AgentVoice::new(tts, shared.clone());
    let mut schedule = Schedule::open();
    let mut stamp = modified();
    let mut pendings = Pendings::open();
    let mut pending_stamp = modified_pending();
    loop {
        std::thread::sleep(Duration::from_secs(1));

        // The settings app may have added, moved or removed something.
        let now_stamp = modified();
        if now_stamp != stamp {
            stamp = now_stamp;
            schedule = Schedule::open();
        }
        let pending_now = modified_pending();
        if pending_now != pending_stamp {
            pending_stamp = pending_now;
            pendings = Pendings::open();
        }

        // ── the queue of "when" actions ────────────────────────────────
        if !pendings.items.is_empty() {
            let world = World::gather(pendings.needs());
            let (fired, missed) = pendings.poll_all(&world);
            if !fired.is_empty() || !missed.is_empty() {
                // Saved before anything runs: a shutdown must not leave the
                // entry behind to fire again on the next start.
                if let Err(e) = pendings.save() {
                    log::warn!("couldn't save the queue: {e}");
                }
                pending_stamp = modified_pending();
            }
            for (entry, late) in &missed {
                log::warn!("missed the queued \"{}\" ({} minutes late)", entry.sentence(), late);
                voice.say(&format!(
                    "I missed what you queued — {}. The PC was away when it should have happened.",
                    entry.sentence()
                ));
            }
            for entry in fired {
                log::info!("queued action due: {}", entry.sentence());
                if shared.mode() == Mode::Hidden {
                    shared.set_mode(Mode::Success);
                }
                run_queued(&entry, &cfg, &apps, &voice);
            }
        }

        let now = Stamp::now();
        let due: Vec<Item> = schedule.due(now).into_iter().cloned().collect();
        if due.is_empty() {
            continue;
        }

        let mut on_time: Vec<Item> = Vec::new();
        let mut missed: Vec<(Item, i64)> = Vec::new();
        for item in due {
            let late = item.at.map_or(0, |at| at.on_day_of(now).minutes_until(now).max(0));
            if late > ON_TIME_MINUTES {
                missed.push((item, late));
            } else {
                on_time.push(item);
            }
        }

        // Mark everything as fired before doing anything slow, so a crash or a
        // second agent can't set it off twice.
        for item in on_time.iter().chain(missed.iter().map(|(i, _)| i)) {
            schedule.mark_fired(item.id, now);
        }
        if let Err(e) = schedule.save() {
            log::warn!("couldn't save the schedule: {e}");
        }
        stamp = modified();

        if !missed.is_empty() {
            announce_missed(&missed, &voice, &shared);
        }
        for item in on_time {
            log::info!("{} due: {}", item.kind.label(), item.text);
            if item.chime || item.kind == ItemKind::Alarm {
                crate::chime::play();
            }
            if shared.mode() == Mode::Hidden {
                shared.set_mode(Mode::Success);
            }
            voice.say(&announcement(&item, now));
            run_action(&item, &cfg, &apps, &voice);
        }
    }
}

/// Things that came due while the agent wasn't running: one line, not five.
fn announce_missed(missed: &[(Item, i64)], voice: &crate::tts::AgentVoice, shared: &Shared) {
    for (item, late) in missed {
        let ago = if *late >= 120 {
            format!("{} hours ago", late / 60)
        } else {
            format!("{late} minutes ago")
        };
        log::info!("missed {} ({ago}): {}", item.kind.label(), item.text);
        if shared.mode() == Mode::Hidden {
            shared.set_mode(Mode::Success);
        }
        voice.say(&format!("While you were away ({ago}): {}", announcement(item, Stamp::now())));
    }
}

/// Run the actions that were queued, and say what happened.
fn run_queued(
    entry: &wisp_core::pending::Pending,
    cfg: &Config,
    apps: &Arc<RwLock<AppIndex>>,
    voice: &crate::tts::AgentVoice,
) {
    let index = apps.read().unwrap().clone();
    let mut outcomes = Vec::new();
    for action in &entry.actions {
        match actions::execute_with(action, cfg, &index, voice) {
            Ok(msg) => {
                log::info!("  ✓ {msg}");
                outcomes.push(personality::Outcome::Done(action.clone(), msg));
            }
            Err(e) => {
                log::warn!("  ✗ {e}");
                outcomes.push(personality::Outcome::Failed(action.clone(), e));
            }
        }
    }
    voice.say(&personality::reply(cfg, &outcomes));
}

/// Run the function an item is wired to, if any.
fn run_action(item: &Item, cfg: &Config, apps: &Arc<RwLock<AppIndex>>, voice: &crate::tts::AgentVoice) {
    if item.action.trim().is_empty() {
        return;
    }
    let action = Action::Custom { name: item.action.clone(), params: Vec::new() };
    let index = apps.read().unwrap().clone();
    match actions::execute_with(&action, cfg, &index, voice) {
        Ok(msg) => log::info!("  ✓ {msg}"),
        Err(e) => {
            log::warn!("  ✗ {e}");
            voice.say(&personality::line(cfg.personality, &personality::Moment::Failed(&action, &e)));
        }
    }
}

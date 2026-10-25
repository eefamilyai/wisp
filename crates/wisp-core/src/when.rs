//! *When* a command should happen: "in 30 minutes", "at 9pm", "when I close
//! this application".
//!
//! A trigger is the second half of a sentence like "shut down at 9pm". It is
//! parsed here, stored with the actions it belongs to (see [`crate::pending`]),
//! and evaluated by the agent once a second against the state of the machine.
//!
//! Two kinds of trigger exist, and they behave differently on purpose:
//!
//! * **Moments** — a wall-clock stamp. Due the first time the clock passes it.
//! * **Conditions** — a fact about the machine that becomes true. Some of those
//!   are *edges* ("when I plug the charger in") and only count on the change,
//!   which is why every queued action remembers the last state it observed.
//!
//! Anything that can be said can be deferred, because this layer knows nothing
//! about what the action is. That is the point: "close chrome when spotify
//! closes" is the same mechanism as "shut down at 9pm".

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::fuzzy;
use crate::schedule::Stamp;
use crate::schedule_parse;
use crate::win;

/// Something that becomes true, after which a queued action runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Trigger {
    /// A wall-clock moment: "at 9pm", "in 30 minutes".
    At { stamp: Stamp },
    /// When a program stops running. `app` empty means "whatever is in front
    /// right now", which the caller fills in from the foreground window.
    AppCloses {
        #[serde(default)]
        app: String,
        #[serde(default)]
        pid: Option<u32>,
    },
    /// When a program starts running.
    AppStarts { app: String },
    /// When the battery reaches this percentage or less, off the mains.
    BatteryBelow { percent: u8 },
    /// The charger is connected / disconnected.
    PluggedIn,
    Unplugged,
    /// The network comes back / goes away.
    Online,
    Offline,
    /// The screen is locked / unlocked.
    Locked,
    Unlocked,
    /// No keyboard or mouse input for this long.
    Idle { minutes: u32 },
    /// A path starts existing — a download, a build product, a render.
    FileExists { path: PathBuf },
}

impl Trigger {
    /// Is this a moment, or a condition?
    pub fn is_moment(&self) -> bool {
        matches!(self, Trigger::At { .. })
    }

    /// Edges only count when the state *changes*: "when I plug the charger in"
    /// while it is already plugged in is not an event, it is the status quo.
    pub fn is_edge(&self) -> bool {
        match self {
            Trigger::PluggedIn
            | Trigger::Unplugged
            | Trigger::Online
            | Trigger::Offline
            | Trigger::Locked
            | Trigger::Unlocked
            | Trigger::AppStarts { .. } => true,
            // "when spotify closes" waits for the next time it closes, because
            // an app that is already closed has not closed *again*. A process
            // pinned by hand ("this application") is a different promise: it is
            // running now, and the thing to wait for is its exit.
            Trigger::AppCloses { pid, .. } => pid.is_none(),
            _ => false,
        }
    }

    /// The live facts this trigger has to look at. Gathering the network state
    /// costs most of a second, so a queue with nothing but timers does not.
    pub fn needs(&self) -> Needs {
        let mut needs = Needs::default();
        match self {
            Trigger::At { .. } | Trigger::Idle { .. } => {}
            Trigger::AppCloses { .. } | Trigger::AppStarts { .. } => needs.processes = true,
            Trigger::BatteryBelow { .. } | Trigger::PluggedIn | Trigger::Unplugged => needs.battery = true,
            Trigger::Online | Trigger::Offline => needs.network = true,
            Trigger::Locked | Trigger::Unlocked => needs.desktop = true,
            Trigger::FileExists { .. } => {}
        }
        if matches!(self, Trigger::Idle { .. }) {
            needs.idle = true;
        }
        needs
    }

    /// How to say it: "at 9:00 pm", "when Chrome closes", "when the battery
    /// drops below 20 percent".
    pub fn describe(&self) -> String {
        match self {
            Trigger::At { stamp } => {
                let now = Stamp::now();
                let day = stamp.speak_date(now);
                match day.as_str() {
                    "today" => format!("at {}", stamp.speak_time()),
                    other => format!("{other} at {}", stamp.speak_time()),
                }
            }
            Trigger::AppCloses { app, .. } => {
                if app.is_empty() {
                    "when that app closes".to_string()
                } else {
                    format!("when {app} closes")
                }
            }
            Trigger::AppStarts { app } => format!("when {app} starts"),
            Trigger::BatteryBelow { percent } => format!("when the battery drops below {percent} percent"),
            Trigger::PluggedIn => "when the charger is plugged in".to_string(),
            Trigger::Unplugged => "when the charger is unplugged".to_string(),
            Trigger::Online => "when the internet comes back".to_string(),
            Trigger::Offline => "when the internet drops".to_string(),
            Trigger::Locked => "when the screen locks".to_string(),
            Trigger::Unlocked => "when the screen unlocks".to_string(),
            Trigger::Idle { minutes } => format!("after {minutes} minutes of no input"),
            Trigger::FileExists { path } => {
                let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                if name.is_empty() {
                    format!("when {} appears", path.display())
                } else {
                    format!("when {name} appears")
                }
            }
        }
    }

    /// Is the condition true right now? For edges this is the *state*, and the
    /// queued action compares it against the last one it saw.
    pub fn state(&self, world: &World) -> bool {
        match self {
            Trigger::At { stamp } => world.now >= *stamp,
            Trigger::AppCloses { app, pid } => !world.is_running(app, *pid),
            Trigger::AppStarts { app } => world.is_running(app, None),
            Trigger::BatteryBelow { percent } => {
                world.battery.is_some_and(|(level, on_ac)| !on_ac && level <= *percent)
            }
            Trigger::PluggedIn => world.battery.is_some_and(|(_, on_ac)| on_ac),
            Trigger::Unplugged => world.battery.is_some_and(|(_, on_ac)| !on_ac),
            Trigger::Online => world.online.unwrap_or(true),
            Trigger::Offline => !world.online.unwrap_or(true),
            Trigger::Locked => world.locked.unwrap_or(false),
            Trigger::Unlocked => !world.locked.unwrap_or(false),
            Trigger::Idle { minutes } => world.idle >= Duration::from_secs(*minutes as u64 * 60),
            Trigger::FileExists { path } => path.exists(),
        }
    }
}

/// Which facts to gather for a tick.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Needs {
    pub processes: bool,
    pub battery: bool,
    pub network: bool,
    pub desktop: bool,
    pub idle: bool,
}

impl Needs {
    pub fn also(self, other: Needs) -> Needs {
        Needs {
            processes: self.processes || other.processes,
            battery: self.battery || other.battery,
            network: self.network || other.network,
            desktop: self.desktop || other.desktop,
            idle: self.idle || other.idle,
        }
    }
}

/// The machine as it is right now, gathered only as far as the queue needs it.
pub struct World {
    pub now: Stamp,
    processes: Option<Vec<(u32, String)>>,
    /// (percent, on mains).
    pub battery: Option<(u8, bool)>,
    pub online: Option<bool>,
    pub locked: Option<bool>,
    pub idle: Duration,
}

impl World {
    pub fn gather(needs: Needs) -> World {
        let processes = needs.processes.then(|| {
            win::processes().into_iter().map(|p| (p.pid, exe_stem(&p.exe))).collect::<Vec<_>>()
        });
        World {
            now: Stamp::now(),
            processes,
            battery: if needs.battery { win::battery().and_then(|b| b.percent.map(|p| (p, b.on_ac))) } else { None },
            online: if needs.network { Some(crate::machine::online_now()) } else { None },
            locked: if needs.desktop { Some(win::screen_locked()) } else { None },
            idle: if needs.idle { win::idle_time() } else { Duration::ZERO },
        }
    }

    /// A world with only the clock, for tests and for the parser.
    pub fn at(stamp: Stamp) -> World {
        World { now: stamp, processes: Some(Vec::new()), battery: None, online: None, locked: None, idle: Duration::ZERO }
    }

    /// Test harness: say what is running and what the battery is doing.
    pub fn with(state: WorldState) -> World {
        World {
            now: Stamp::now(),
            processes: Some(state.processes),
            battery: state.battery,
            online: state.online,
            locked: state.locked,
            idle: state.idle,
        }
    }

    pub fn is_running(&self, app: &str, pid: Option<u32>) -> bool {
        let Some(processes) = &self.processes else { return true };
        if let Some(pid) = pid {
            // A pid was recorded when the command was spoken, which is the most
            // precise thing available: gone means gone.
            return processes.iter().any(|(p, _)| *p == pid);
        }
        if app.trim().is_empty() {
            // Nothing to go on: assume it is still running rather than firing a
            // shutdown because we could not tell.
            return true;
        }
        let wanted = fuzzy::squash(&fuzzy::strip_filler(app));
        if wanted.is_empty() {
            return true;
        }
        processes.iter().any(|(_, exe)| {
            let exe = fuzzy::squash(exe);
            exe == wanted || exe.contains(&wanted) || fuzzy::similarity(&exe, &wanted) >= 0.85
        })
    }
}

/// A world described by hand, for tests.
#[derive(Default, Clone)]
pub struct WorldState {
    pub processes: Vec<(u32, String)>,
    pub battery: Option<(u8, bool)>,
    pub online: Option<bool>,
    pub locked: Option<bool>,
    pub idle: Duration,
}

fn exe_stem(exe: &str) -> String {
    exe.split('.').next().unwrap_or(exe).to_lowercase()
}

/// Split a spoken command into the trigger and the command left behind.
///
/// Both orders are accepted — "shut down at 9pm" and "at 9pm shut down" — and
/// the split is only taken when the trigger half really parses *and* there is
/// something left to act on. That is what keeps "look at my downloads" and
/// "what's on my schedule at 9" out of the queue.
pub fn split(command: &str) -> Option<(Trigger, String)> {
    let words: Vec<&str> = command.split_whitespace().collect();
    if words.len() < 2 {
        return None;
    }
    let now = Stamp::now();
    let joined = |range: std::ops::Range<usize>| words[range].join(" ");

    // The usual shape: the trigger is at the end, the command at the front.
    for at in 1..words.len() {
        if let Some(trigger) = parse_trigger(&joined(at..words.len()), now) {
            let head = clean_command(&joined(0..at));
            if !head.is_empty() && !asks_a_question(&head) {
                return Some((trigger, head));
            }
        }
    }
    // And the shape people use when they are thinking out loud.
    for at in (1..words.len()).rev() {
        if let Some(trigger) = parse_trigger(&joined(0..at), now) {
            let head = clean_command(&joined(at..words.len()));
            if !head.is_empty() && !asks_a_question(&head) {
                return Some((trigger, head));
            }
        }
    }
    None
}

/// Is this the question half of a sentence? "what's on my schedule at 9" wants
/// an answer now, not a queue entry — the command it would defer is the question
/// itself, which is never what anyone means.
fn asks_a_question(head: &str) -> bool {
    const ASKS: [&str; 10] = ["what", "whats", "who", "whos", "when", "where", "why", "how", "which", "whom"];
    head.split_whitespace()
        .next()
        .map(crate::fuzzy::squash)
        .is_some_and(|w| ASKS.contains(&w.as_str()))
}

/// Strip the fat a spoken command carries at the edges.
fn clean_command(text: &str) -> String {
    let mut out = text.trim().trim_matches(|c: char| c == ',' || c == '.').trim().to_string();
    for junk in ["then", "please", "and"] {
        if out.to_lowercase().ends_with(&format!(" {junk}")) {
            out.truncate(out.len() - junk.len() - 1);
            out = out.trim_end_matches([',', ' ']).to_string();
        }
    }
    for junk in ["and", "then", "so", "please", "also"] {
        if out.to_lowercase().starts_with(&format!("{junk} ")) {
            out = out[junk.len() + 1..].trim().to_string();
        }
    }
    out.trim().to_string()
}

/// Parse one trigger phrase, or say no.
pub fn parse_trigger(text: &str, now: Stamp) -> Option<Trigger> {
    let t = text.trim().trim_end_matches([',', '.', '?', '!']).trim().to_lowercase();
    if t.is_empty() {
        return None;
    }
    parse_moment(&t, now).or_else(|| parse_event(&t, now))
}

/// A wall-clock moment. "9pm", "at 9pm", "in 20 minutes", "tomorrow at 9".
fn parse_moment(text: &str, now: Stamp) -> Option<Trigger> {
    // Every word has to belong to a time expression. The time parser itself is
    // deliberately forgiving — it ignores words it does not know — which is
    // exactly right for "remind me at 4 about the dentist" and exactly wrong
    // here: without this gate "down in 30 minutes" parses as a time and the
    // command becomes "shut".
    if !is_only_time_words(text) || !starts_like_a_moment(text) {
        return None;
    }
    for candidate in [text.to_string(), format!("at {text}")] {
        if let Ok((stamp, _)) = schedule_parse::parse(&candidate, now) {
            return Some(Trigger::At { stamp });
        }
    }
    None
}

/// Is every word here part of a time expression?
fn is_only_time_words(text: &str) -> bool {
    let mut any = false;
    for word in text.split_whitespace() {
        let w = word.trim_matches(|c: char| !c.is_alphanumeric() && c != ':');
        if w.is_empty() {
            continue;
        }
        any = true;
        let clock = w.contains(':') || is_clock_number(w);
        let allowed = schedule_parse::is_time_word(w)
            || clock
            || matches!(w, "an" | "now" | "from" | "and" | "the" | "left" | "next");
        if !allowed {
            return false;
        }
    }
    any
}

/// Does the phrase *start* like a moment? "for 7:30 am" is a time expression,
/// but it belongs to "set an alarm for 7:30 am", not to the queue.
fn starts_like_a_moment(text: &str) -> bool {
    let first = text.split_whitespace().next().unwrap_or("");
    matches!(first, "at" | "in" | "after" | "by" | "tomorrow" | "tonight" | "today" | "noon" | "midnight" | "next")
        || is_clock_number(first)
}

/// "9", "9pm", "7:30pm" — a number with an optional am/pm stuck to it.
fn is_clock_number(word: &str) -> bool {
    let w = word.trim_end_matches(['.', ',']);
    let core = w.trim_end_matches("pm").trim_end_matches("am").trim_end_matches("p").trim_end_matches("a");
    !core.is_empty() && core.chars().all(|c| c.is_ascii_digit()) && core.len() <= 4
}

/// A condition: "when I close chrome", "when the battery hits 20".
fn parse_event(text: &str, _now: Stamp) -> Option<Trigger> {
    // A condition always starts with one of these, or the sentence was not a
    // condition at all — "shut down at 9" must not be read as an event.
    let mut rest = text.trim();
    let mut prefixed = false;
    for lead in ["whenever ", "as soon as ", "when ", "once ", "after ", "if "] {
        if let Some(tail) = rest.strip_prefix(lead) {
            rest = tail.trim();
            prefixed = true;
            break;
        }
    }
    if !prefixed {
        return None;
    }
    rest = rest.trim_start_matches("i'll ").trim_start_matches("im ").trim_start_matches("i am ").trim();

    parse_power_event(rest)
        .or_else(|| parse_network_event(rest))
        .or_else(|| parse_screen_event(rest))
        .or_else(|| parse_battery_event(rest))
        .or_else(|| parse_idle_event(rest))
        .or_else(|| parse_file_event(rest))
        .or_else(|| parse_app_event(rest))
}

fn parse_battery_event(text: &str) -> Option<Trigger> {
    if !text.contains("battery") && !text.contains("charge") {
        return None;
    }
    if text.contains("unplug") || text.contains("goes off") || text.contains("runs out") || text.contains("dies") {
        return Some(Trigger::Unplugged);
    }
    if text.contains("plug") || text.contains("charger") && text.contains("connect") {
        return Some(Trigger::PluggedIn);
    }
    // "when the battery drops below 20", "hits 15 percent", "is under 30"
    let percent = first_number(text)?;
    if (1..=100).contains(&percent) {
        return Some(Trigger::BatteryBelow { percent: percent as u8 });
    }
    None
}

fn parse_power_event(text: &str) -> Option<Trigger> {
    if text.contains("charger") || text.contains("plug") {
        if text.contains("unplug") || text.contains("out") || text.contains("off") {
            return Some(Trigger::Unplugged);
        }
        return Some(Trigger::PluggedIn);
    }
    None
}

fn parse_network_event(text: &str) -> Option<Trigger> {
    let loses = ["drops", "dies", "goes away", "goes down", "disconnect", "cuts out", "fails", "lost", "lose"];
    let gains = ["comes back", "returns", "reconnect", "connects", "is back", "come back", "get online"];
    let about_net = text.contains("internet")
        || text.contains("network")
        || text.contains("wifi")
        || text.contains("wi fi")
        || text.contains("connection")
        || text.contains("online");
    if !about_net {
        return None;
    }
    if loses.iter().any(|w| text.contains(w)) {
        return Some(Trigger::Offline);
    }
    if gains.iter().any(|w| text.contains(w)) || text.contains("online") {
        return Some(Trigger::Online);
    }
    None
}

fn parse_screen_event(text: &str) -> Option<Trigger> {
    let locking = ["lock", "locks", "locked", "sign out", "log out", "sleeps", "suspends"];
    let unlocking = ["unlock", "unlocks", "unlocked", "log back in", "sign back in", "come back", "wake"];
    // "when I unlock" with nothing after it is still the screen: there is
    // nothing else a person can unlock by walking up to their own PC.
    let bare = matches!(text.trim(), "i" | "im" | "i am" | "its" | "it" | "the screen" | "my screen");
    // "I lock it" has no noun either — a person who says "I" is talking about
    // their own screen.
    let first_person = text.starts_with("i ") || text.starts_with("im ") || text.starts_with("i am ");
    let about_the_screen = text.contains("screen")
        || text.contains("pc")
        || text.contains("computer")
        || text.contains("windows")
        || bare
        || first_person
        || text.split_whitespace().count() <= 2;
    if about_the_screen {
        if unlocking.iter().any(|w| text.contains(w)) {
            return Some(Trigger::Unlocked);
        }
        if locking.iter().any(|w| text.contains(w)) {
            return Some(Trigger::Locked);
        }
    }
    None
}

fn parse_idle_event(text: &str) -> Option<Trigger> {
    let idle_words = [
        "idle", "inactive", "inactivity", "no input", "not touched", "stop using", "away from",
        "leave the desk", "leaves the desk", "step away", "walk away", "get up", "leave the computer",
    ];
    if !idle_words.iter().any(|w| text.contains(w)) {
        return None;
    }
    let minutes = first_number(text).unwrap_or(10);
    // "when I'm idle for 10 minutes", "after 10 minutes of inactivity"
    Some(Trigger::Idle { minutes: minutes.max(1) })
}

fn parse_file_event(text: &str) -> Option<Trigger> {
    let appears = ["appears", "shows up", "exists", "is there", "has downloaded", "finishes downloading", "is done", "finishes"];
    if !appears.iter().any(|w| text.contains(w)) {
        return None;
    }
    let mut name = text.to_string();
    for junk in [
        "the file ", "file ", "a file called ", "the folder ", "folder ", "my ",
        "finishes downloading", "has downloaded", "is done", "shows up", "appears", "exists", "is there", "finishes",
    ] {
        name = name.replace(junk, " ");
    }
    let name = name.trim().trim_matches(['.', ',']).trim();
    if name.is_empty() {
        return None;
    }
    Some(Trigger::FileExists { path: resolve_path(name) })
}

fn parse_app_event(text: &str) -> Option<Trigger> {
    // "this application" and its cousins: resolved by the caller from whatever
    // is in front at the moment the command is spoken.
    const THIS: [&str; 6] = [
        "this application", "this app", "this program", "this window", "the app im using", "the application im using",
    ];
    if THIS.iter().any(|t| text.contains(t)) {
        return Some(Trigger::AppCloses { app: String::new(), pid: None });
    }

    // Closing: "i close chrome", "chrome closes", "im done with chrome".
    let close_lead = ["i close ", "i quit ", "i exit ", "i shut down ", "i kill ", "i finish with ", "i finish ",
                      "im done with ", "i am done with ", "i stop using ", "i close out "];
    let close_tail = [" closes", " closes down", " quits", " exits", " is closed", " has closed", " shuts down",
                      " stops", " ends", " has finished", " finishes", " goes away", " is gone", " is quit"];
    for lead in close_lead {
        if let Some(rest) = text.strip_prefix(lead) {
            let app = clean_app(rest);
            if !app.is_empty() {
                return Some(Trigger::AppCloses { app, pid: None });
            }
        }
    }
    for tail in close_tail {
        if let Some(app) = text.strip_suffix(tail) {
            let app = clean_app(app);
            if !app.is_empty() {
                return Some(Trigger::AppCloses { app, pid: None });
            }
        }
    }

    // Starting: "i open chrome", "chrome starts".
    let open_lead = ["i open ", "i start ", "i launch ", "i run ", "i open up "];
    let open_tail = [" starts", " opens", " launches", " runs", " is open", " is running"];
    for lead in open_lead {
        if let Some(rest) = text.strip_prefix(lead) {
            let app = clean_app(rest);
            if !app.is_empty() {
                return Some(Trigger::AppStarts { app });
            }
        }
    }
    for tail in open_tail {
        if let Some(app) = text.strip_suffix(tail) {
            let app = clean_app(app);
            if !app.is_empty() {
                return Some(Trigger::AppStarts { app });
            }
        }
    }
    None
}

/// "loading a website", "the download finishes" — not an app.
fn clean_app(text: &str) -> String {
    let mut app = text.trim().trim_matches(['.', ',']).to_string();
    for junk in ["the ", "my ", "up ", "again"] {
        if let Some(rest) = app.strip_prefix(junk) {
            app = rest.to_string();
        }
    }
    if app.ends_with(" is closed") || app.ends_with(" is done") {
        app = app.trim_end_matches(" is closed").trim_end_matches(" is done").to_string();
    }
    app.trim().to_string()
}

/// The first number in a phrase, as words or digits.
fn first_number(text: &str) -> Option<u32> {
    const WORDS: [(&str, u32); 20] = [
        ("zero", 0), ("one", 1), ("two", 2), ("three", 3), ("four", 4), ("five", 5), ("six", 6),
        ("seven", 7), ("eight", 8), ("nine", 9), ("ten", 10), ("eleven", 11), ("twelve", 12),
        ("fifteen", 15), ("twenty", 20), ("thirty", 30), ("forty", 40), ("fifty", 50),
        ("sixty", 60), ("hundred", 100),
    ];
    for word in text.split(|c: char| !c.is_alphanumeric() && c != '-') {
        if let Ok(n) = word.parse::<u32>() {
            return Some(n);
        }
        for (name, value) in WORDS {
            if name == word {
                return Some(value);
            }
        }
    }
    None
}

/// Where a bare file name probably means: a known folder, or the literal path.
fn resolve_path(name: &str) -> PathBuf {
    let raw = name.trim().trim_matches(['"', '\'']);
    let literal = PathBuf::from(raw);
    if literal.is_absolute() || raw.contains('\\') || raw.contains('/') {
        return literal;
    }
    for dir in [
        crate::paths::downloads_folder(),
        crate::paths::documents_folder(),
        crate::paths::desktop_folder(),
    ]
    .into_iter()
    .flatten()
    {
        let candidate = dir.join(raw);
        if candidate.exists() {
            return candidate;
        }
    }
    crate::paths::downloads_folder().map(|d| d.join(raw)).unwrap_or(literal)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_moment_is_understood_both_ways_round() {
        let cases = ["shut down in 30 minutes", "shut down at 9pm", "lock my pc at 11 pm", "open chrome in 5 minutes", "in 20 minutes, close chrome", "at 9pm shut down the pc"];
        for case in cases {
            let (trigger, rest) = split(case).unwrap_or_else(|| panic!("{case:?} did not split"));
            assert!(trigger.is_moment(), "{case:?} → {trigger:?}");
            assert!(!rest.is_empty(), "{case:?} lost its command");
            assert!(!rest.to_lowercase().contains("at 9"), "{case:?} kept the trigger: {rest:?}");
        }
        let (trigger, rest) = split("shut down in 30 minutes").unwrap();
        assert_eq!(rest, "shut down");
        match trigger {
            Trigger::At { stamp } => {
                let expected = Stamp::now().add_minutes(30);
                assert_eq!(stamp, expected);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_condition_is_understood() {
        let (trigger, rest) = split("shut down when I close this application").unwrap();
        assert_eq!(rest, "shut down");
        assert_eq!(trigger, Trigger::AppCloses { app: String::new(), pid: None });

        let (trigger, rest) = split("close chrome when spotify closes").unwrap();
        assert_eq!(rest, "close chrome");
        assert_eq!(trigger, Trigger::AppCloses { app: "spotify".into(), pid: None });

        let (trigger, _) = split("lock my pc when i leave the desk for 10 minutes").unwrap();
        assert_eq!(trigger, Trigger::Idle { minutes: 10 });

        let (trigger, _) = split("turn on the lights when the battery drops below 20").unwrap();
        assert_eq!(trigger, Trigger::BatteryBelow { percent: 20 });

        let (trigger, _) = split("open the report when chrome starts").unwrap();
        assert_eq!(trigger, Trigger::AppStarts { app: "chrome".into() });

        let (trigger, _) = split("shut down when the internet comes back").unwrap();
        assert_eq!(trigger, Trigger::Online);

        let (trigger, _) = split("pause the music when i unplug the charger").unwrap();
        assert_eq!(trigger, Trigger::Unplugged);

        // "when I unlock" needs no noun: there is nothing else to unlock.
        let (trigger, rest) = split("dim the screen when I unlock").unwrap();
        assert_eq!(trigger, Trigger::Unlocked);
        assert_eq!(rest, "dim the screen");
        let (trigger, _) = split("dim the screen when I lock it").unwrap();
        assert_eq!(trigger, Trigger::Locked);
    }

    /// The parser must not turn ordinary sentences into queued actions. This is
    /// the failure that would be hardest to notice and worst to live with.
    #[test]
    fn ordinary_commands_are_left_alone() {
        for command in [
            "open chrome",
            "look at my downloads",
            "put this in the recycle bin",
            "find the file budget",
            "what's on my clipboard",
            "set an alarm for 7:30 am",
            "what's on my schedule at 9",
            "play the music in the kitchen",
            "type out hello there",
            "open my notes",
            "close this window",
            "what time is it",
            "never mind",
        ] {
            assert!(split(command).is_none(), "{command:?} should not have split");
        }
    }

    #[test]
    fn triggers_describe_themselves() {
        let world = World::with(WorldState::default());
        let cases = vec![
            (Trigger::AppCloses { app: "Chrome".into(), pid: None }, "when Chrome closes"),
            (Trigger::AppStarts { app: "steam".into() }, "when steam starts"),
            (Trigger::BatteryBelow { percent: 20 }, "when the battery drops below 20 percent"),
            (Trigger::PluggedIn, "when the charger is plugged in"),
            (Trigger::Online, "when the internet comes back"),
            (Trigger::Idle { minutes: 10 }, "after 10 minutes of no input"),
            (Trigger::FileExists { path: PathBuf::from(r"C:\x\build.zip") }, "when build.zip appears"),
        ];
        for (trigger, wanted) in cases {
            assert_eq!(trigger.describe(), wanted);
            let _ = trigger.state(&world);
        }
        let at = Trigger::At { stamp: Stamp::now().add_minutes(90) };
        assert!(at.describe().contains(':'), "{}", at.describe());
    }

    /// The evaluation rules, one by one, against a world described by hand.
    #[test]
    fn conditions_are_evaluated_against_the_machine() {
        let running = World::with(WorldState { processes: vec![(7, "chrome".into()), (9, "code".into())], ..Default::default() });
        let chrome_closed = World::with(WorldState { processes: vec![(9, "code".into())], ..Default::default() });

        let closes = Trigger::AppCloses { app: "chrome".into(), pid: None };
        assert!(!closes.state(&running));
        assert!(closes.state(&chrome_closed));

        let by_pid = Trigger::AppCloses { app: String::new(), pid: Some(7) };
        assert!(!by_pid.state(&running));
        assert!(by_pid.state(&chrome_closed));

        let starts = Trigger::AppStarts { app: "spotify".into() };
        assert!(!starts.state(&running));
        assert!(starts.state(&World::with(WorldState { processes: vec![(11, "spotify".into())], ..Default::default() })));

        let low = Trigger::BatteryBelow { percent: 20 };
        assert!(!low.state(&World::with(WorldState { battery: Some((45, false)), ..Default::default() })));
        assert!(low.state(&World::with(WorldState { battery: Some((18, false)), ..Default::default() })));
        // On the mains it is charging, not running out.
        assert!(!low.state(&World::with(WorldState { battery: Some((18, true)), ..Default::default() })));

        assert!(Trigger::PluggedIn.state(&World::with(WorldState { battery: Some((80, true)), ..Default::default() })));
        assert!(!Trigger::PluggedIn.state(&World::with(WorldState { battery: Some((80, false)), ..Default::default() })));
        assert!(Trigger::Unplugged.state(&World::with(WorldState { battery: Some((80, false)), ..Default::default() })));

        assert!(Trigger::Online.state(&World::with(WorldState { online: Some(true), ..Default::default() })));
        assert!(Trigger::Offline.state(&World::with(WorldState { online: Some(false), ..Default::default() })));

        assert!(Trigger::Locked.state(&World::with(WorldState { locked: Some(true), ..Default::default() })));
        assert!(Trigger::Unlocked.state(&World::with(WorldState { locked: Some(false), ..Default::default() })));

        let idle = Trigger::Idle { minutes: 10 };
        assert!(!idle.state(&World::with(WorldState { idle: Duration::from_secs(60), ..Default::default() })));
        assert!(idle.state(&World::with(WorldState { idle: Duration::from_secs(601), ..Default::default() })));
    }

    #[test]
    fn a_moment_is_due_when_the_clock_passes_it() {
        let now = Stamp::now();
        let past = Trigger::At { stamp: now.add_minutes(-1) };
        let future = Trigger::At { stamp: now.add_minutes(5) };
        let world = World::at(now);
        assert!(past.state(&world));
        assert!(!future.state(&world));
    }

    /// Only edges need remembering, and only the facts a trigger uses are
    /// gathered — the network probe alone costs most of a second.
    #[test]
    fn edges_and_needs_are_classified() {
        assert!(Trigger::PluggedIn.is_edge());
        assert!(Trigger::AppStarts { app: "x".into() }.is_edge());
        assert!(Trigger::At { stamp: Stamp::now() }.is_moment());
        let needs = Trigger::At { stamp: Stamp::now() }.needs();
        assert_eq!(needs, Needs::default(), "a timer needs nothing but the clock");
        assert!(Trigger::Online.needs().network);
        assert!(Trigger::AppCloses { app: "x".into(), pid: None }.needs().processes);
        assert!(Trigger::Idle { minutes: 5 }.needs().idle);
    }
}

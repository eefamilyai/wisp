//! Wisp settings app.
#![windows_subsystem = "windows"]
#![allow(clippy::field_reassign_with_default)]

mod jobs;
mod miccheck;
mod shell;
mod ui;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait};
use eframe::egui::{self, Color32, RichText, Stroke};
use jobs::Job;
use wisp_core::apps::AppIndex;
use wisp_core::brain::Brain;
use wisp_core::config::{
    parse_hex, Alias, ApiProvider, Appearance, BrainKind, Browser, SttEngine, Thinking, ACCENT_PRESETS,
};
use wisp_core::schedule::{describe_when, Item, ItemKind, Repeat, Schedule, Stamp};
use wisp_core::tools::{self, CustomKind, CustomTool, Step, StepKind};
use wisp_core::personality::{self, Moment, Persona};
use wisp_core::voices::{self, Engine};
use wisp_core::{win, Config};

#[derive(PartialEq, Clone, Copy, Debug)]
enum Tab {
    Home,
    General,
    Voice,
    Listening,
    Functions,
    Computer,
    Apps,
    Browser,
    Schedule,
    Brain,
    Test,
}

impl Tab {
    /// In navigation order, which is also the order of the Ctrl+1… shortcuts.
    const ALL: [Tab; 11] = [
        Tab::Home,
        Tab::General,
        Tab::Voice,
        Tab::Listening,
        Tab::Schedule,
        Tab::Brain,
        Tab::Functions,
        Tab::Computer,
        Tab::Apps,
        Tab::Browser,
        Tab::Test,
    ];

    fn index(self) -> usize {
        Tab::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }
}

/// The numbers the dashboard shows, sampled off the UI thread.
///
/// Reading the processor's load means measuring it across a window — a third of
/// a second — and doing that while drawing would make the window stutter every
/// time the home page was open.
#[derive(Default, Clone)]
struct Snapshot {
    cpu: Option<f32>,
    /// (total, available) bytes.
    memory: Option<(u64, u64)>,
    /// (name, free, total) bytes.
    drives: Vec<(String, u64, u64)>,
    /// (percent, charging, on mains).
    battery: Option<(u8, bool, bool)>,
    uptime_secs: u64,
    /// The tail of the agent's log: (line, level) where level is 0 info,
    /// 1 warning, 2 error.
    log: Vec<(String, u8)>,
}

/// Start the sampler. It runs for the life of the window; a detached thread is
/// the whole lifetime it needs.
fn spawn_sampler() -> std::sync::Arc<std::sync::Mutex<Snapshot>> {
    let shared = std::sync::Arc::new(std::sync::Mutex::new(Snapshot::default()));
    let theirs = shared.clone();
    std::thread::Builder::new()
        .name("dashboard".into())
        .spawn(move || loop {
            let mut snap = Snapshot::default();
            snap.cpu = wisp_core::win::cpu_usage(Duration::from_millis(250));
            snap.memory = wisp_core::win::memory();
            snap.drives = wisp_core::win::drives().into_iter().map(|d| (d.name, d.free, d.total)).collect();
            snap.battery = wisp_core::win::battery().and_then(|b| b.percent.map(|p| (p, b.charging, b.on_ac)));
            snap.uptime_secs = wisp_core::win::uptime().as_secs();
            snap.log = tail_log(14);
            if let Ok(mut slot) = theirs.lock() {
                *slot = snap;
            }
            std::thread::sleep(Duration::from_secs(2));
        })
        .ok();
    shared
}

/// The last `count` lines of the agent's log, with each line's level.
fn tail_log(count: usize) -> Vec<(String, u8)> {
    let Ok(text) = std::fs::read_to_string(wisp_core::paths::log_file()) else { return Vec::new() };
    let mut out: Vec<(String, u8)> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let level = match l.split_whitespace().nth(1).unwrap_or("") {
                "WARN" => 1,
                "ERROR" => 2,
                _ => 0,
            };
            (l.to_string(), level)
        })
        .collect();
    if out.len() > count {
        out.drain(..out.len() - count);
    }
    out
}


/// The half-filled "add to the schedule" form.
struct NewItem {
    kind: ItemKind,
    text: String,
    when: String,
    repeat: Repeat,
    action: String,
    chime: bool,
    speak: bool,
}

impl Default for NewItem {
    fn default() -> Self {
        NewItem {
            kind: ItemKind::Alarm,
            text: String::new(),
            when: String::new(),
            repeat: Repeat::Once,
            action: String::new(),
            chime: true,
            speak: true,
        }
    }
}

const SEARCH_ENGINES: [(&str, &str); 4] = [
    ("Google", "https://www.google.com/search?q={}"),
    ("Bing", "https://www.bing.com/search?q={}"),
    ("DuckDuckGo", "https://duckduckgo.com/?q={}"),
    ("Brave Search", "https://search.brave.com/search?q={}"),
];

/// Whisper models offered in Settings: file, label, download size in MB.
/// The times are measured on the machine this was built on (Ryzen 5 2600,
/// 4 threads, a 3-second command), which is what makes them worth showing.
const WHISPER_MODELS: [(&str, &str, u32); 5] = [
    ("ggml-tiny.en-q5_1.bin", "Tiny — instant, but mishears words", 31),
    ("ggml-base.en-q5_1.bin", "Base — quick, fine for clear speech", 57),
    ("ggml-small.en-q5_1.bin", "Small — the best balance of the small ones", 181),
    ("ggml-medium.en-q5_0.bin", "Medium — most accurate that still runs on a CPU (default)", 514),
    ("ggml-large-v3-turbo-q5_0.bin", "Turbo — bigger, and no faster than Medium here", 547),
];

struct App {
    cfg: Config,
    /// Where settings are read from and written to. `--config <file>` overrides
    /// the usual `%APPDATA%\Wisp\config.toml`, which is handy for
    /// testing a set of functions without touching the live ones.
    cfg_file: PathBuf,
    saved: Config,
    /// Set when the settings file exists but couldn't be read, so it is
    /// obvious why everything suddenly looks like a fresh install.
    load_error: Option<String>,
    tab: Tab,
    status: Option<(String, Instant, bool)>,
    agent_running: bool,
    /// When the running state was last asked for.
    agent_checked: std::time::Instant,
    /// Set while waiting for Start to actually take effect.
    starting_since: Option<std::time::Instant>,

    mics: Vec<String>,
    system_voices: Vec<String>,
    apps: AppIndex,
    app_filter: String,
    new_alias: (String, String),

    jobs: Vec<(String, Job)>,
    test_input: String,
    test_result: Option<String>,
    test_job: Option<Job>,
    brain: std::sync::Arc<std::sync::Mutex<Option<Brain>>>,
    mic_check: Option<miccheck::MicCheck>,

    /// The wake-word sub-word tokeniser, loaded once (None when the model
    /// isn't installed).
    bpe: Option<wisp_core::bpe::Bpe>,
    /// The "add to the schedule" form.
    new_item: NewItem,
    /// Editing buffers for fields whose stored value is a list or a parsed
    /// date. Those can't be edited in place: the frame after each keystroke
    /// would rebuild the text from the parsed value, so a freshly typed newline
    /// or a trailing comma would vanish under the caret.
    drafts: std::collections::HashMap<String, String>,
    /// The schedule, held rather than re-read: the tab used to call
    /// `Schedule::open()` on every frame, which is a file read plus a JSON
    /// parse sixty times a second while the page was open.
    schedule: ScheduleStore,
    /// The last answer from [`wisp_core::win::find_python`], and when it was
    /// asked for, so the Functions page does not search PATH every frame.
    python_found: Option<PathBuf>,
    python_searched: String,
    python_checked: Instant,

    /// Live numbers for the dashboard.
    stats: std::sync::Arc<std::sync::Mutex<Snapshot>>,
    /// The API key being typed on the Brain page. It is only written to the
    /// settings when Save is pressed, so a half-typed key never lands on disk.
    api_key_draft: String,
    show_key: bool,
    /// The models the current key can use, once it has been asked.
    model_list: Option<Vec<String>>,
    /// The last few commands tried on the Test page or the dashboard.
    test_history: Vec<(String, String)>,
    /// The filter box on the Functions page.
    fn_filter: String,
    /// What was typed in the dashboard's command box.
    home_command: String,
    /// The command the runner is working on, so the history can label it.
    test_command: String,
    /// Actions queued for a moment or a condition, as the agent will see them.
    pendings: wisp_core::pending::Pendings,
    /// When the queue file was last read, so it is not re-read every frame.
    pendings_at: Option<std::time::SystemTime>,
}

/// The schedule file, with a cached copy of what was last read from it.
///
/// The list is shared with the agent and is edited by hand often enough that
/// the file still has to be re-read, but only when it has actually changed —
/// or when this window was the one that changed it.
#[derive(Default)]
struct ScheduleStore {
    /// Kept so the `&Schedule` handed to the page outlives the call.
    cache: Option<Schedule>,
    stamp: Option<std::time::SystemTime>,
    dirty: bool,
}

impl ScheduleStore {
    fn modified() -> Option<std::time::SystemTime> {
        std::fs::metadata(Schedule::path()).and_then(|m| m.modified()).ok()
    }

    /// The current list, re-reading the file only when it has moved on.
    fn read(&mut self) -> &Schedule {
        let stamp = Self::modified();
        if self.dirty || self.cache.is_none() || stamp != self.stamp {
            self.stamp = stamp;
            self.dirty = false;
            self.cache = Some(Schedule::open());
        }
        self.cache.as_ref().expect("just filled")
    }

    /// A copy to edit. The cache is refreshed on the next read, so a partial
    /// edit never leaks back into the file.
    fn edit(&mut self) -> Schedule {
        let copy = self.read().clone();
        self.dirty = true;
        copy
    }

    /// Put an edited copy back, noting the file's new timestamp so the next
    /// frame does not read our own write.
    fn store(&mut self, schedule: Schedule) -> std::io::Result<()> {
        let r = schedule.save();
        self.stamp = Self::modified();
        self.dirty = false;
        self.cache = Some(schedule);
        r
    }
}

impl App {
    fn new(cc: &eframe::CreationContext) -> Self {
        let args: Vec<String> = std::env::args().collect();
        let cfg_file = args
            .iter()
            .position(|a| a == "--config")
            .and_then(|i| args.get(i + 1))
            .map(PathBuf::from)
            .unwrap_or_else(wisp_core::paths::config_file);
        let (cfg, load_error) = match Config::try_load_from(&cfg_file) {
            Ok(c) => (c, None),
            Err(e) => (Config::default(), Some(e)),
        };
        let theme = ui::Theme::new(cfg.appearance.is_dark(), accent32(cfg.accent_rgb()));
        ui::style(&cc.egui_ctx, &theme);
        ui::install_fonts(&cc.egui_ctx);
        let tab = match args.iter().skip_while(|a| *a != "--tab").nth(1).map(String::as_str) {
            Some("home") => Tab::Home,
            Some("computer") | Some("windows") => Tab::Computer,
            Some("voice") => Tab::Voice,
            Some("listening") => Tab::Listening,
            Some("schedule") | Some("alarms") => Tab::Schedule,
            Some("brain") => Tab::Brain,
            Some("browser") => Tab::Browser,
            Some("apps") => Tab::Apps,
            Some("functions") => Tab::Functions,
            Some("test") => Tab::Test,
            _ => Tab::General,
        };
        let mics = cpal::default_host()
            .input_devices()
            .map(|it| it.filter_map(|d| d.description().ok().map(|d| d.name().to_string())).collect())
            .unwrap_or_default();
        win::com_init();
        let system_voices = wisp_core::voice::list_voices();
        Self {
            saved: cfg.clone(),
            cfg,
            load_error,
            cfg_file,
            tab,
            status: None,
            agent_running: win::is_running(wisp_core::AGENT_EXE),
            agent_checked: std::time::Instant::now(),
            starting_since: None,
            mics,
            system_voices,
            apps: AppIndex::load_cache().unwrap_or_default(),
            app_filter: String::new(),
            new_alias: Default::default(),
            jobs: Vec::new(),
            test_input: "what is the haber process".into(),
            test_result: None,
            test_job: None,
            brain: Default::default(),
            mic_check: None,
            bpe: wisp_core::bpe::Bpe::load(&wisp_core::wake_model::dir().join("bpe.model")),
            new_item: NewItem::default(),
            drafts: Default::default(),
            schedule: ScheduleStore::default(),
            // Long enough in the past that the first frame looks it up.
            python_found: None,
            python_searched: String::new(),
            python_checked: Instant::now() - Duration::from_secs(60),
            stats: spawn_sampler(),
            api_key_draft: String::new(),
            show_key: false,
            model_list: None,
            test_history: Vec::new(),
            fn_filter: String::new(),
            home_command: String::new(),
            test_command: String::new(),
            pendings: wisp_core::pending::Pendings::open(),
            pendings_at: None,
        }
    }

    /// Re-read the queue if the agent has changed it.
    fn refresh_pendings(&mut self) {
        let path = wisp_core::pending::Pendings::path();
        let stamp = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        if stamp != self.pendings_at {
            self.pendings_at = stamp;
            self.pendings = wisp_core::pending::Pendings::open();
        }
    }

    /// The dashboard's numbers, copied out of the shared slot.
    fn snapshot(&self) -> Snapshot {
        self.stats.lock().map(|s| s.clone()).unwrap_or_default()
    }

    fn toast(&mut self, msg: impl Into<String>, ok: bool) {
        self.status = Some((msg.into(), Instant::now(), ok));
    }

    fn agent_exe() -> PathBuf {
        wisp_core::paths::sibling_exe(wisp_core::AGENT_EXE)
    }

    fn save(&mut self) {
        self.cfg.sanitize();
        match self.cfg.save_to(&self.cfg_file) {
            Ok(()) => {
                if let Err(e) = win::set_autostart(self.cfg.start_with_windows, &Self::agent_exe()) {
                    self.toast(format!("Saved, but couldn't set auto-start: {e}"), false);
                } else if self.agent_running {
                    self.toast("Saved — the assistant is restarting with your changes", true);
                } else {
                    self.toast("Saved", true);
                }
                self.saved = self.cfg.clone();
                // Every text box is rebuilt from the stored value next frame:
                // after normalising, a kept draft can hold something the config
                // no longer says — a name that was trimmed, a phrase that was
                // lowercased — and the box would go on showing it.
                self.drafts.clear();
                self.load_error = None;
            }
            Err(e) => self.toast(format!("Couldn't save: {e}"), false),
        }
    }

    fn start_agent(&mut self) {
        if self.agent_running {
            self.toast("The assistant is already running", true);
            return;
        }
        match win::spawn_detached(&Self::agent_exe(), &[]) {
            // Launching is not the same as running: the second copy of a
            // singleton exits immediately, and a crash looks the same. Wait
            // until it is really up before saying anything.
            Ok(()) => self.starting_since = Some(std::time::Instant::now()),
            Err(e) => self.toast(format!("Couldn't start {}: {e}", Self::agent_exe().display()), false),
        }
    }

    /// Stop the assistant: the same thing the tray's Exit does.
    fn stop_agent(&mut self) {
        if win::stop_process(wisp_core::AGENT_EXE) {
            self.toast("Assistant stopped", true);
            self.agent_running = false;
        } else {
            self.toast("The assistant wasn't running", false);
        }
    }

    fn job_running(&self, key: &str) -> Option<&Job> {
        self.jobs.iter().find(|(k, j)| k == key && !j.done()).map(|(_, j)| j)
    }

    fn start_job(
        &mut self,
        ctx: &egui::Context,
        key: &str,
        label: &str,
        work: impl FnOnce(&dyn Fn(f32)) -> Result<String, String> + Send + 'static,
    ) {
        if self.job_running(key).is_some() {
            return;
        }
        self.jobs.retain(|(k, _)| k != key);
        self.jobs.push((key.into(), Job::spawn(ctx, label, work)));
    }

    /// Collect finished jobs into toasts.
    fn poll_jobs(&mut self) {
        self.poll_test();
        // The agent can be started or stopped from the tray, the Start menu or a
        // crash, so "is it running" has to be asked again rather than decided
        // once at startup — which is why the button used to say "Start" at a
        // running assistant, and did nothing when pressed.
        if self.agent_checked.elapsed() > std::time::Duration::from_millis(700) {
            self.agent_checked = std::time::Instant::now();
            self.agent_running = win::is_running(wisp_core::AGENT_EXE);
        }
        self.starting_since = match self.starting_since {
            Some(_) if self.agent_running => None,
            Some(t) if t.elapsed() > std::time::Duration::from_secs(8) => {
                self.toast("The assistant did not start — see the log in the settings folder", false);
                None
            }
            other => other,
        };
        let mut finished = Vec::new();
        self.jobs.retain(|(k, j)| {
            let st = j.0.lock().unwrap();
            match &st.result {
                Some(r) => {
                    finished.push((k.clone(), r.clone()));
                    false
                }
                None => true,
            }
        });
        for (key, r) in finished {
            match r {
                Ok(msg) => {
                    if key == "scan" {
                        self.apps = AppIndex::load_cache().unwrap_or_default();
                    }
                    if key == "wake_model" {
                        // Pick up the freshly installed tokeniser right away.
                        self.bpe = wisp_core::bpe::Bpe::load(&wisp_core::wake_model::dir().join("bpe.model"));
                    }
                    self.toast(msg, true)
                }
                Err(e) => self.toast(e, false),
            }
        }
    }

    fn preview_voice(&mut self, text: Option<String>) {
        let text = text.unwrap_or_else(|| {
            let a = wisp_core::brain::Action::OpenApp("Google Chrome".into());
            personality::line(self.cfg.personality, &Moment::Done(&a))
        });
        // A file of its own per preview: two windows previewing at once used to
        // share one path, and the second write could land between the first
        // window's read and the agent's.
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let tmp = std::env::temp_dir().join(format!("wisp-preview-{}-{stamp}.toml", std::process::id()));
        let mut c = self.cfg.clone();
        c.voice_enabled = true;
        if let Err(e) = c.save_to(&tmp) {
            self.toast(format!("Preview failed: {e}"), false);
            return;
        }
        let tmp_s = tmp.display().to_string();
        if let Err(e) = win::spawn_detached(&Self::agent_exe(), &["--say", &text, "--config", &tmp_s]) {
            let _ = std::fs::remove_file(&tmp);
            self.toast(format!("Preview failed: {e}"), false);
        } else {
            self.toast("Playing preview… (neural voices take a second to load)", true);
        }
    }

    /// The editing buffer for a field: what has been typed so far, or the
    /// stored value the first time round.
    fn buffer(&self, key: &str, stored: &str) -> String {
        self.drafts.get(key).cloned().unwrap_or_else(|| stored.to_string())
    }

    /// Remember a buffer for the next frame.
    fn keep(&mut self, key: &str, text: String) {
        self.drafts.insert(key.to_string(), text);
    }

    /// The dashboard: is it running, what can it do, and is it healthy.
    fn tab_home(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let t = ui::theme(ui);
        let snap = self.snapshot();
        let brain_label = match self.cfg.brain_kind {
            BrainKind::Local => "Needle 3, on this PC".to_string(),
            BrainKind::Api => self.cfg.api_provider.label().to_string(),
        };
        let mic = if self.cfg.microphone.is_empty() { "Windows default microphone".to_string() } else { self.cfg.microphone.clone() };

        // ── hero ────────────────────────────────────────────────────────
        egui::Frame::new()
            .fill(t.surface)
            .stroke(t.hairline())
            .corner_radius(egui::CornerRadius::same(14))
            .inner_margin(egui::Margin::symmetric(18, 16))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui::live_mark(ui, t.accent, 58.0, self.agent_running && !self.cfg.paused);
                    ui.add_space(16.0);
                    ui.vertical(|ui| {
                        let headline = if self.cfg.paused {
                            "Paused — the microphone is closed".to_string()
                        } else if self.agent_running {
                            format!("Listening for \u{201C}{}\u{201D}", self.cfg.wake_phrase())
                        } else {
                            "The assistant isn't running".to_string()
                        };
                        ui.label(RichText::new(headline).size(20.0).strong().color(t.text));
                        ui.add_space(3.0);
                        let sub = if self.agent_running {
                            "Say the wake word, then what you want. Everything stays on this PC."
                        } else {
                            "Press Start, then say the wake word followed by a command."
                        };
                        ui.label(RichText::new(sub).size(12.5).color(t.muted));
                        ui.add_space(9.0);
                        ui.horizontal(|ui| {
                            let (colour, text) = if self.agent_running { (t.ok, "Running") } else { (t.faint, "Stopped") };
                            ui::pill(ui, text, colour);
                            ui::pill(ui, &mic, t.muted);
                            ui::pill(ui, &brain_label, t.muted);
                            ui::pill(ui, self.cfg.stt_engine.label(), t.muted);
                        });
                    });
                    // The controls sit in one column of their own: a row of
                    // them next to the text collided with it as soon as the
                    // window was narrow.
                    let (label, enabled) = match (self.agent_running, self.starting_since) {
                        (true, _) => ("Stop", true),
                        (false, Some(_)) => ("Starting…", false),
                        (false, None) => ("Start", true),
                    };
                    let running = self.agent_running;
                    ui.vertical(|ui| {
                        ui.set_width(132.0);
                        let button = egui::Button::new(RichText::new(label).size(13.5).strong().color(if enabled {
                            if running { t.text } else { t.on_accent }
                        } else {
                            t.muted
                        }))
                        .fill(if running { t.inset } else { t.primary_fill(enabled) })
                        .stroke(if running { Stroke::new(1.0, t.border_strong) } else { Stroke::NONE })
                        .corner_radius(egui::CornerRadius::same(10))
                        .min_size(egui::vec2(132.0, 34.0));
                        if ui.add_enabled(enabled, button).clicked() {
                            if running {
                                self.stop_agent();
                            } else {
                                self.start_agent();
                            }
                        }
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            let mut paused = self.cfg.paused;
                            if ui::switch(ui, &mut paused, "Pause").changed() {
                                self.cfg.paused = paused;
                            }
                        });
                    });
                });
            });
        ui.add_space(12.0);

        // ── try it ──────────────────────────────────────────────────────
        let mut run = false;
        let mut do_it = false;
        ui::card(ui, "Try a command", "Type what you would say after the wake word", |ui| {
            ui.horizontal(|ui| {
                let field = egui::TextEdit::singleline(&mut self.home_command)
                    .hint_text("open chrome, how much battery is left, take a note …")
                    .desired_width(ui.available_width() - 190.0)
                    .margin(egui::Margin::symmetric(9, 7));
                let response = ui.add(field);
                run = ui::primary(ui, "Understand", true).clicked()
                    || (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                do_it = ui::ghost(ui, "and do it").clicked();
                if self.test_job.is_some() {
                    ui.spinner();
                }
            });
            if let Some(result) = &self.test_result {
                ui.add_space(10.0);
                let lines: Vec<(String, Color32)> = result
                    .lines()
                    .map(|l| {
                        let colour = if l.trim_start().starts_with('✔') {
                            t.ok
                        } else if l.trim_start().starts_with('✖') {
                            t.danger
                        } else if l.starts_with("It would say") {
                            t.accent
                        } else {
                            t.muted
                        };
                        (l.to_string(), colour)
                    })
                    .collect();
                ui::code_lines(ui, &lines, 150.0);
            }
        });
        if run || do_it {
            let command = self.home_command.clone();
            self.run_command(ctx, command, do_it);
        }

        // ── the machine underneath ──────────────────────────────────────
        let spacing = 10.0;
        let width = (ui.available_width() - spacing * 3.0) / 4.0;
        ui.horizontal(|ui| {
            // The row is measured by hand, so the default gap between items
            // would push the last tile past the card's edge.
            ui.spacing_mut().item_spacing.x = 0.0;
            let cpu = snap.cpu.unwrap_or(0.0);
            ui::stat(ui, "Processor", &format!("{cpu:.0}%"), "", Some(cpu / 100.0), t.accent, width);
            ui.add_space(spacing);
            match snap.memory {
                Some((total, available)) => {
                    let used = total.saturating_sub(available);
                    let fraction = if total > 0 { used as f32 / total as f32 } else { 0.0 };
                    let detail = format!("{:.1} of {:.1} GB", used as f64 / 1073741824.0, total as f64 / 1073741824.0);
                    ui::stat(ui, "Memory", &format!("{:.0}%", fraction * 100.0), &detail, Some(fraction), t.accent, width);
                }
                None => {
                    ui::stat(ui, "Memory", "?", "", None, t.accent, width);
                }
            }
            ui.add_space(spacing);
            match snap.drives.iter().find(|d| d.0.starts_with('C')) .or_else(|| snap.drives.first()) {
                Some((name, free, total)) => {
                    let used = total.saturating_sub(*free);
                    let fraction = if *total > 0 { used as f32 / *total as f32 } else { 0.0 };
                    let detail = format!("{} — {:.0} GB free", name, *free as f64 / 1073741824.0);
                    ui::stat(ui, "Disk", &format!("{:.0}%", fraction * 100.0), &detail, Some(fraction), t.accent, width);
                }
                None => {
                    ui::stat(ui, "Disk", "?", "", None, t.accent, width);
                }
            }
            ui.add_space(spacing);
            match snap.battery {
                Some((percent, charging, on_ac)) => {
                    let state = if charging { "charging" } else if on_ac { "on mains" } else { "on battery" };
                    ui::stat(
                        ui,
                        "Battery",
                        &format!("{percent}%"),
                        state,
                        Some(percent as f32 / 100.0),
                        if percent < 20 && !charging { t.warn } else { t.ok },
                        width,
                    );
                }
                None => {
                    let hours = snap.uptime_secs / 3600;
                    let minutes = (snap.uptime_secs / 60) % 60;
                    ui::stat(ui, "Uptime", &format!("{hours}h {minutes}m"), "no battery", None, t.ok, width);
                }
            }
        });
        ui.add_space(12.0);

        // ── what it can do ──────────────────────────────────────────────
        let off = self.cfg.disabled_tools.len();
        let on = tools::BUILTINS.len().saturating_sub(off);
        let own = self.cfg.custom_tools.len();
        let scheduled = self.schedule.read().items.len();
        let mut go = None;
        ui::card(ui, "What it can do", "", |ui| {
            ui.horizontal_wrapped(|ui| {
                ui::pill(ui, &format!("{on} functions on"), t.ok);
                if off > 0 {
                    ui::pill(ui, &format!("{off} off"), t.muted);
                }
                ui::pill(ui, &format!("{own} of your own"), if own > 0 { t.accent } else { t.muted });
                ui::pill(ui, &format!("{scheduled} scheduled"), t.muted);
                let queued = self.pendings.items.len();
                if queued > 0 {
                    ui::pill(ui, &format!("{queued} queued"), t.accent);
                }
            });
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                if ui::ghost(ui, "\u{2192}  Everything it can do").clicked() {
                    go = Some(Tab::Functions);
                }
                if ui::ghost(ui, "\u{2192}  What it can touch on this PC").clicked() {
                    go = Some(Tab::Computer);
                }
                if ui::ghost(ui, "\u{2192}  Choose the brain").clicked() {
                    go = Some(Tab::Brain);
                }
                if ui::ghost(ui, "\u{2192}  Alarms & reminders").clicked() {
                    go = Some(Tab::Schedule);
                }
            });
        });
        if let Some(tab) = go {
            self.tab = tab;
        }

        // ── is it healthy ───────────────────────────────────────────────
        ui::card_rows(ui, "Recent activity", "Straight from the agent's log", |ui| {
            if snap.log.is_empty() {
                ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    ui::hint(ui, "Nothing logged yet. Start the assistant and say something.");
                });
                ui.add_space(8.0);
            } else {
                let lines: Vec<(String, Color32)> = snap
                    .log
                    .iter()
                    .map(|(line, level)| {
                        let colour = match level {
                            2 => t.danger,
                            1 => t.warn,
                            _ => t.muted,
                        };
                        (line.clone(), colour)
                    })
                    .collect();
                ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width() - 28.0);
                        ui::code_lines(ui, &lines, 132.0);
                    });
                });
            }
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                if ui::ghost(ui, "Open the whole log").clicked() {
                    let _ = win::shell_open(&wisp_core::paths::log_file().display().to_string(), None);
                }
                if ui::ghost(ui, "Open the settings folder").clicked() {
                    let _ = win::shell_open(&wisp_core::paths::data_dir().display().to_string(), None);
                }
            });
            ui.add_space(6.0);
        });
    }

    /// What the assistant is allowed to touch on this machine, grouped the way
    /// a person thinks about it, with one switch per group.
    fn tab_computer(&mut self, ui: &mut egui::Ui) {
        let t = ui::theme(ui);
        ui::notice(
            ui,
            ui::Notice::Info,
            "These are the things it can do to your PC. Everything runs locally and only when you ask for it — \
             there is no background access, and nothing is sent anywhere.",
        );

        // The groups to show, and what each one means in plain words.
        const GROUPS: [(&str, &str); 5] = [
            ("Windows", "Switch between windows, move them around, snap them to a side"),
            ("Files & notes", "Find files in your own folders, open them, put them in the recycle bin"),
            ("Clipboard & typing", "Read and write the clipboard, type into whatever has the focus"),
            ("System", "Ask about the battery, memory, disk and network, and what is using them"),
            ("Power & screen", "Sleep, shut down, restart, sign out, change the brightness"),
        ];
        let mut pending: Option<(Vec<&'static str>, bool)> = None;
        for (group, blurb) in GROUPS {
            let names: Vec<&'static str> = tools::BUILTINS.iter().filter(|b| b.group == group).map(|b| b.name).collect();
            if names.is_empty() {
                continue;
            }
            let on_count = names.iter().filter(|n| !self.cfg.disabled_tools.iter().any(|d| d == *n)).count();
            let mut on = on_count == names.len();
            let before = on;
            ui::card_rows(ui, group, blurb, |ui| {
                ui::toggle_row(
                    ui,
                    &format!("Allow {group}"),
                    &if on { format!("All {} available.", names.len()) } else { format!("{on_count} of {} available.", names.len()) },
                    &mut on,
                );
                for name in &names {
                    let enabled = !self.cfg.disabled_tools.iter().any(|d| d == name);
                    ui.horizontal(|ui| {
                        ui.add_space(14.0);
                        ui::dot(ui, if enabled { t.ok } else { t.faint }, 6.0);
                        ui.label(RichText::new(pretty_name(name)).size(12.5).color(if enabled { t.text } else { t.faint }));
                        if let Some(tool) = tools::builtin(name) {
                            ui.label(
                                RichText::new(format!("\u{201C}{}\u{201D}", tool.example))
                                    .size(11.0)
                                    .color(if enabled { t.faint } else { t.faint.linear_multiply(0.7) }),
                            );
                        }
                    });
                    ui.add_space(5.0);
                }
                ui.add_space(4.0);
            });
            if on != before {
                pending = Some((names, on));
            }
        }
        if let Some((names, on)) = pending {
            for name in names {
                self.cfg.disabled_tools.retain(|d| d != name);
                if !on {
                    self.cfg.disabled_tools.push(name.to_string());
                }
            }
        }

        ui::card(ui, "How it is kept safe", "", |ui| {
            for line in [
                "Deleting a file moves it to the Recycle Bin — it is always recoverable.",
                "Shut down and restart wait 30 seconds, and \"cancel the shutdown\" stops them.",
                "It only touches the window, folder or setting you name; there is no blanket access.",
                "Each one is the same Win32 call a person would make with the mouse.",
            ] {
                ui.horizontal(|ui| {
                    ui::glyph(ui, ui::ICON_CHECK, 12.0, t.ok);
                    ui.label(RichText::new(line).size(12.0).color(t.muted));
                });
                ui.add_space(4.0);
            }
        });
        ui::hint(
            ui,
            "Each individual function can also be switched on its own, on the Functions page.",
        );
    }

    fn tab_general(&mut self, ui: &mut egui::Ui, accent: Color32) {
        let _ = accent;
        ui::card_rows(ui, "Wake word", "How it knows you are talking to it", |ui| {
            ui::row(ui, "Assistant name", "Two syllables or more is harder to trigger by accident", |ui| {
                ui::text_field(ui, &mut self.cfg.agent_name, "Wisp", 220.0);
            });
            let mut prefixes = self.buffer("wake:prefixes", &self.cfg.wake_prefixes.join(", "));
            ui::row(ui, "Words before the name", "Comma separated", |ui| {
                if ui::text_field(ui, &mut prefixes, "hey, ok", 220.0).changed() {
                    self.cfg.wake_prefixes =
                        prefixes.split(',').map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).collect();
                }
            });
            self.keep("wake:prefixes", prefixes);
            ui::row(ui, "Answer to the bare name", "Wake on the name alone, without a word in front", |ui| {
                ui::switch(ui, &mut self.cfg.allow_name_only, "");
            });
            let mut extra = self.buffer("wake:extra", &self.cfg.wake_extra_names.join(", "));
            ui::row(ui, "Also answers to", "Spellings it keeps mis-hearing, comma separated", |ui| {
                if ui::text_field(ui, &mut extra, "no va, hey novah", 220.0).changed() {
                    self.cfg.wake_extra_names =
                        extra.split(',').map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).collect();
                }
            });
            self.keep("wake:extra", extra);
            ui::row(ui, "Sensitivity", "Higher wakes more easily, and more often by mistake", |ui| {
                ui.add(egui::Slider::new(&mut self.cfg.wake_sensitivity, 0.0..=1.0).show_value(true));
            });
            ui::row(ui, "Chime on wake", "The instant the name is recognised", |ui| {
                ui::switch(ui, &mut self.cfg.wake_sound, "");
            });
            ui::row(ui, "Say \"Yes?\" on wake", "Clearer, but it talks over your first word", |ui| {
                ui::switch(ui, &mut self.cfg.wake_reply, "");
            });
            ui.add_space(2.0);
            self.wake_advanced(ui);
        });

        ui::card(ui, "Bubble", "The floating circle at the bottom of the screen", |ui| {
            ui::switch(ui, &mut self.cfg.show_overlay, "Show the bubble while listening");
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui::switch(ui, &mut self.cfg.live_transcript, "Show what you are saying, above the bubble");
            });
            if self.cfg.live_transcript && !wisp_core::streaming_installed() {
                ui.horizontal(|ui| {
                    ui.add_space(26.0);
                    ui::pill(ui, "streaming model not installed", ui::theme(ui).warn);
                    ui::hint(ui, "until it is, there is nothing to show while you speak");
                });
            }
            if self.cfg.live_transcript {
                ui.horizontal(|ui| {
                    ui.add_space(26.0);
                    ui::hint(
                        ui,
                        "Shown by a streaming model - one that writes words as they are spoken, without waiting \
                         for the sentence to end (NeMo FastConformer, 126 MB; words appear about 150 ms after they \
                         are said). The engines above cannot do this: they only see a finished sentence.",
                    );
                });
            }
            ui.add_space(10.0);
            ui::field(ui, "Accent colour", "Used across the bubble, this window and the tray icon", |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (name, hex) in ACCENT_PRESETS {
                        let (r, g, b) = parse_hex(hex).unwrap();
                        let c = accent32((r, g, b));
                        let selected = self.cfg.accent_color.eq_ignore_ascii_case(hex);
                        if ui::swatch(ui, c, selected).on_hover_text(*name).clicked() {
                            self.cfg.accent_color = hex.to_string();
                        }
                    }
                    let (r, g, b) = self.cfg.accent_rgb();
                    let mut rgb = [(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8];
                    if ui.color_edit_button_srgb(&mut rgb).on_hover_text("Custom colour").changed() {
                        self.cfg.accent_color = format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]);
                    }
                });
            });
            ui::field(ui, "Appearance", "Dark, light, or whatever Windows is set to", |ui| {
                let mut appearance = self.cfg.appearance;
                if ui::segmented(ui, &mut appearance, &Appearance::ALL.map(|a| (a, a.label()))) {
                    self.cfg.appearance = appearance;
                }
            });
            ui::field(ui, "Bubble size", "", |ui| {
                ui.add(egui::Slider::new(&mut self.cfg.overlay_size, 40..=160).suffix(" px"));
            });
            ui::field(ui, "Distance from the bottom", "", |ui| {
                ui.add(egui::Slider::new(&mut self.cfg.overlay_margin, 0..=300).suffix(" px"));
            });
        });

        ui::card_rows(ui, "Windows", "How it fits in with the rest of the system", |ui| {
            ui::row(ui, "Start with Windows", "Launch the assistant when you sign in", |ui| {
                ui::switch(ui, &mut self.cfg.start_with_windows, "");
            });
            ui::row(ui, "Paused", "Keep the microphone closed until you unpause", |ui| {
                ui::switch(ui, &mut self.cfg.paused, "");
            });
            ui::row(ui, "Files", "Settings, the log, and the schedule", |ui| {
                if ui::ghost(ui, "Open settings folder").clicked() {
                    let _ = win::shell_open(&wisp_core::paths::data_dir().display().to_string(), None);
                }
                if ui::ghost(ui, "Open the log").clicked() {
                    let _ = win::shell_open(&wisp_core::paths::log_file().display().to_string(), None);
                }
            });
        });
    }

    fn tab_voice(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, accent: Color32) {
        let _ = accent;
        let t = ui::theme(ui);
        ui::card(ui, "Personality", "How it talks back to you", |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                for persona in Persona::ALL {
                    let selected = self.cfg.personality == persona;
                    let text = RichText::new(persona.label()).size(12.5).color(if selected { t.on_accent } else { t.muted });
                    let button = egui::Button::new(text)
                        .fill(if selected { t.accent } else { t.inset })
                        .stroke(t.hairline())
                        .corner_radius(egui::CornerRadius::same(8))
                        .min_size(egui::vec2(0.0, 28.0));
                    if ui.add(button).clicked() {
                        self.cfg.personality = persona;
                    }
                }
            });
            ui.add_space(10.0);
            let action = wisp_core::brain::Action::WebSearch("the Haber process".into());
            let sample = personality::example(self.cfg.personality, &Moment::Done(&action));
            ui.label(RichText::new(format!("\u{201C}{sample}\u{201D}")).size(13.0).italics().color(t.muted));
        });

        let mut preview = false;
        ui::card_rows(ui, "Voice", "What it sounds like out loud", |ui| {
            ui::row(ui, "Talk back out loud", "", |ui| {
                ui::switch(ui, &mut self.cfg.voice_enabled, "");
            });
            ui.add_enabled_ui(self.cfg.voice_enabled, |ui| {
                ui::row(ui, "Engine", "", |ui| {
                    let mut engine = self.cfg.voice_engine;
                    if ui::segmented(ui, &mut engine, &Engine::ALL.map(|e| (e, e.label_short()))) {
                        self.cfg.voice_engine = engine;
                    }
                });
                ui::row(ui, "Speed", "", |ui| {
                    ui.add(egui::Slider::new(&mut self.cfg.voice_speed, 0.6..=1.6));
                });
                ui::row(ui, "Volume", "", |ui| {
                    ui.add(egui::Slider::new(&mut self.cfg.voice_volume, 0..=100));
                });
                ui::row(ui, "Hear it", "Speaks a sample line in the voice you picked", |ui| {
                    if ui::ghost(ui, "Preview voice").clicked() {
                        preview = true;
                    }
                });
            });
        });
        if preview {
            self.preview_voice(None);
        }

        if !self.cfg.voice_enabled {
            ui::hint(ui, "With this off, it answers on screen and in the log only.");
            return;
        }
        match self.cfg.voice_engine {
            Engine::Piper => self.piper_list(ui, ctx, accent),
            Engine::Kokoro => self.kokoro_panel(ui, ctx, accent),
            Engine::System => {
                ui::card_rows(ui, "Windows voice", "The classic built-in voices", |ui| {
                    ui::row(ui, "Voice", "", |ui| {
                        egui::ComboBox::from_id_salt("sysvoice")
                            .width(280.0)
                            .selected_text(if self.cfg.system_voice.is_empty() {
                                "System default".to_string()
                            } else {
                                self.cfg.system_voice.clone()
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.cfg.system_voice, String::new(), "System default");
                                for v in &self.system_voices {
                                    ui.selectable_value(&mut self.cfg.system_voice, v.clone(), v);
                                }
                            });
                    });
                    ui.horizontal(|ui| {
                        ui.add_space(14.0);
                        ui::hint(ui, "Robotic but instant. Natural and Ultra-realistic sound human.");
                    });
                    ui.add_space(6.0);
                });
            }
        }
    }

    fn piper_list(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, _accent: Color32) {
        let t = ui::theme(ui);
        let mut to_download: Option<&'static str> = None;
        let mut to_preview: Option<String> = None;
        let mut to_remove: Option<&'static str> = None;
        ui::card_rows(ui, "Natural voices", "About 64 MB each, downloaded once and then offline", |ui| {
            for v in voices::PIPER_VOICES {
                let installed = voices::is_installed(v.id);
                let selected = self.cfg.piper_voice == v.id;
                ui::row(ui, v.label, if selected { "in use" } else { "" }, |ui| {
                    if installed {
                        let text = RichText::new(if selected { "In use" } else { "Use this" })
                            .size(12.5)
                            .color(if selected { t.on_accent } else { t.text });
                        let button = egui::Button::new(text)
                            .fill(if selected { t.accent } else { t.inset })
                            .stroke(if selected { Stroke::NONE } else { Stroke::new(1.0, t.border_strong) })
                            .corner_radius(egui::CornerRadius::same(8))
                            .min_size(egui::vec2(0.0, 26.0));
                        if ui.add_enabled(!selected, button).clicked() {
                            self.cfg.piper_voice = v.id.into();
                            to_preview = Some(v.id.into());
                        }
                        if !selected && v.id != voices::DEFAULT_PIPER
                            && ui::icon_button(ui, ui::ICON_DELETE, "Delete this voice").clicked()
                        {
                            to_remove = Some(v.id);
                        }
                    } else if let Some(job) = self.job_running(v.id) {
                        let progress = job.0.lock().map(|s| s.progress).unwrap_or(-1.0);
                        ui.add(egui::ProgressBar::new(progress.max(0.0)).desired_width(140.0).fill(t.accent));
                    } else if ui::ghost(ui, &format!("Download · {} MB", v.size_mb)).clicked() {
                        to_download = Some(v.id);
                    }
                });
            }
        });
        if let Some(id) = to_download {
            self.start_job(ctx, id, &format!("Downloading {id}"), move |p| {
                voices::download(id, |d, t| p(if t > 0 { d as f32 / t as f32 } else { -1.0 }))
                    .map(|_| "Voice downloaded — click it to select".to_string())
            });
        }
        if let Some(id) = to_remove {
            let _ = voices::remove(id);
            self.toast("Voice deleted", true);
        }
        if to_preview.is_some() {
            self.preview_voice(None);
        }
    }

    fn kokoro_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, _accent: Color32) {
        let t = ui::theme(ui);
        let installed = voices::is_installed(voices::KOKORO_PACK);
        let mut download = false;
        let mut preview = false;
        if !installed {
            ui::card(ui, "Ultra-realistic voices (Kokoro)", "The most human-sounding offline engine", |ui| {
                ui.label(
                    RichText::new(format!(
                        "A one-time {} MB download with {} English voices. It needs more processor: expect \
                         one to three seconds before it starts talking.",
                        voices::KOKORO_SIZE_MB,
                        voices::KOKORO_VOICES.len()
                    ))
                    .size(12.5)
                    .color(t.muted),
                );
                ui.add_space(10.0);
                if let Some(job) = self.job_running(voices::KOKORO_PACK) {
                    let progress = job.0.lock().map(|s| s.progress).unwrap_or(-1.0);
                    ui.add(egui::ProgressBar::new(progress.max(0.0)).desired_width(280.0).show_percentage().fill(t.accent));
                } else if ui::primary(ui, &format!("Download Kokoro · {} MB", voices::KOKORO_SIZE_MB), true).clicked() {
                    download = true;
                }
            });
        } else {
            ui::card(ui, "Ultra-realistic voices (Kokoro)", "Click a voice to select it and hear a preview", |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                    for v in voices::KOKORO_VOICES.iter() {
                        let selected = self.cfg.kokoro_voice == v.name;
                        let text = RichText::new(v.label).size(12.0).color(if selected { t.on_accent } else { t.muted });
                        let button = egui::Button::new(text)
                            .fill(if selected { t.accent } else { t.inset })
                            .stroke(t.hairline())
                            .corner_radius(egui::CornerRadius::same(8))
                            .min_size(egui::vec2(0.0, 26.0));
                        if ui.add(button).clicked() {
                            self.cfg.kokoro_voice = v.name.into();
                            preview = true;
                        }
                    }
                });
            });
        }
        if download {
            self.start_job(ctx, voices::KOKORO_PACK, "Downloading Kokoro voices", |p| {
                voices::download(voices::KOKORO_PACK, |d, t| p(if t > 0 { d as f32 / t as f32 } else { -1.0 }))
                    .map(|_| "Kokoro installed — pick a voice".to_string())
            });
        }
        if preview {
            self.preview_voice(None);
        }
    }

    fn tab_listening(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, accent: Color32) {
        self.wake_model_ui(ui, ctx, accent);
        self.mic_check_ui(ui, ctx, accent);

        ui::card_rows(ui, "Microphone", "Which device it listens to, and how it hears you", |ui| {
            ui::row(ui, "Device", "", |ui| {
                egui::ComboBox::from_id_salt("mic")
                    .width(300.0)
                    .selected_text(if self.cfg.microphone.is_empty() {
                        "Windows default microphone".to_string()
                    } else {
                        self.cfg.microphone.clone()
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.cfg.microphone, String::new(), "Windows default microphone");
                        for m in &self.mics {
                            ui.selectable_value(&mut self.cfg.microphone, m.clone(), m);
                        }
                    });
            });
            ui::row(ui, "Noise filtering", "Higher ignores more background noise, but can miss quiet speech", |ui| {
                ui.add(egui::Slider::new(&mut self.cfg.vad_aggressiveness, 0..=3));
            });
            ui::row(ui, "Mic boost", "Applied before anything else hears it", |ui| {
                ui.add(egui::Slider::new(&mut self.cfg.mic_gain_db, 0.0..=40.0).suffix(" dB"));
            });
            ui::row(ui, "Minimum loudness", "Quieter than this is treated as silence", |ui| {
                ui.add(egui::Slider::new(&mut self.cfg.min_speech_db, -75.0..=-20.0).suffix(" dB"));
            });
            ui::row(ui, "Pause that ends a command", "Silence this long finishes the sentence", |ui| {
                ui.add(egui::Slider::new(&mut self.cfg.end_silence_ms, 300..=2500).suffix(" ms"));
            });
            ui::row(ui, "Wait after waking", "How long it stays ready for a command", |ui| {
                ui.add(egui::Slider::new(&mut self.cfg.command_timeout_secs, 2.0..=15.0).suffix(" s"));
            });
            ui::row(ui, "Bluetooth headsets", "Use another microphone when the headset's is a hands-free one", |ui| {
                ui::switch(ui, &mut self.cfg.avoid_bluetooth_mic, "");
            });
            let in_use = self.cfg.microphone.clone();
            if in_use.to_lowercase().contains("hands-free")
                || in_use.to_lowercase().starts_with("headset")
                || in_use.to_lowercase().contains("(headset")
            {
                ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    ui::pill(ui, "hands-free microphone", ui::theme(ui).warn);
                    ui::hint(
                        ui,
                        "This is the headset's own mic. While it is open the headset plays in mono, \
                         telephone quality — pick another microphone (a webcam one, say) for full \
                         sound, or switch this off to keep using it.",
                    );
                });
                ui.add_space(6.0);
            }
            ui::row(ui, "Set these for me", "The microphone check measures your voice and room", |ui| {
                if ui::ghost(ui, "Run the check").clicked() {
                    self.mic_check = Some(miccheck::MicCheck::open());
                    if let Some(mc) = self.mic_check.as_mut() {
                        mc.start();
                    }
                }
            });
        });

        let models = wisp_core::paths::models_dir();
        let mut download: Option<(&'static str, u32)> = None;
        let mut get_moonshine = false;
        ui::card_rows(ui, "Speech recognition", "Turns what you said into text", |ui| {
            ui::row(ui, "Engine", "", |ui| {
                let mut engine = self.cfg.stt_engine;
                if ui::segmented(ui, &mut engine, &SttEngine::ALL.map(|e| (e, e.label()))) {
                    self.cfg.stt_engine = engine;
                }
            });
            // What each engine is, and whether its model is here. Choosing one
            // with no model used to leave the assistant unable to hear anything.
            for (engine, blurb) in [
                (
                    SttEngine::Moonshine,
                    "Moonshine - for short commands, English, about a quarter of a second, 135 MB in memory.",
                ),
                (
                    SttEngine::SenseVoice,
                    "SenseVoice - one model, reads Chinese too. Same speed, 226 MB in memory; slightly behind Moonshine on clear speech.",
                ),
                (
                    SttEngine::Whisper,
                    "Whisper - not recommended on this build: one 3-second clip measured 39 minutes, and it holds 0.8 GB with medium.en. It is never used automatically.",
                ),
            ] {
                let present = match engine {
                    SttEngine::Whisper => wisp_core::whisper_model_path(&self.cfg.whisper_model).exists(),
                    SttEngine::Moonshine => wisp_core::moonshine_installed(),
                    SttEngine::SenseVoice => wisp_core::paths::models_dir()
                        .join("sensevoice")
                        .join(wisp_core::SENSEVOICE_PACK)
                        .join("model.int8.onnx")
                        .exists(),
                };
                ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    let selected = self.cfg.stt_engine == engine;
                    let colour = if selected { ui::theme(ui).text } else { ui::theme(ui).faint };
                    ui.label(RichText::new(blurb).size(11.5).color(colour));
                    if !present {
                        ui::pill(ui, "not installed", ui::theme(ui).warn);
                    }
                });
            }
            // What happens when the chosen engine mis-hears something. This is
            // the answer to "it doesn't know my function": usually the function is
            // fine and the words never arrived.
            ui::row(
                ui,
                "If it mis-hears you",
                "Another recogniser re-hears the same clip before it gives up",
                |ui| {
                    let theme = ui::theme(ui);
                    let mut shown = false;
                    if self.cfg.live_transcript && wisp_core::streaming_installed() {
                        ui::pill(ui, "the caption", theme.ok);
                        shown = true;
                    }
                    for (engine, label) in [
                        (SttEngine::Moonshine, "Moonshine"),
                        (SttEngine::SenseVoice, "SenseVoice"),
                        (SttEngine::Whisper, "Whisper"),
                    ] {
                        if engine == self.cfg.stt_engine {
                            continue;
                        }
                        let ready = match engine {
                            SttEngine::Whisper => wisp_core::whisper_model_path(&self.cfg.whisper_model).exists(),
                            SttEngine::Moonshine => wisp_core::moonshine_installed(),
                            SttEngine::SenseVoice => wisp_core::paths::models_dir()
                                .join("sensevoice")
                                .join(wisp_core::SENSEVOICE_PACK)
                                .join("model.int8.onnx")
                                .exists(),
                        };
                        if ready {
                            ui::pill(ui, label, theme.muted);
                            shown = true;
                        }
                    }
                    if !shown {
                        ui::hint(ui, "nothing else installed — it will ask you to say it again");
                    }
                },
            );
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                ui::hint(ui, "When the first transcript leads nowhere it tries the caption you already saw, then another engine, and only then asks you to repeat it. Anything big is let go again once it has spoken.");
            });
            ui.add_space(6.0);
            ui::row(
                ui,
                "Save what it hears",
                "Keeps every clip it recognised in the settings folder's clips\\ directory",
                |ui| {
                    ui::switch(ui, &mut self.cfg.save_clips, "");
                },
            );
            if self.cfg.stt_engine == SttEngine::Moonshine {
                // English-only, and either on disk or one download away.
                ui::row(ui, "Moonshine", "English only", |ui| {
                    if wisp_core::moonshine_installed() {
                        ui::pill(ui, "installed", ui::theme(ui).ok);
                    } else if self.job_running("moonshine").is_some() {
                        let progress = self
                            .job_running("moonshine")
                            .and_then(|j| j.0.lock().ok().map(|s| s.progress))
                            .unwrap_or(-1.0);
                        ui.add(egui::ProgressBar::new(progress.max(0.0)).desired_width(200.0).fill(ui::theme(ui).accent));
                    } else {
                        ui::pill(ui, "not downloaded", ui::theme(ui).warn);
                        if ui::ghost(ui, "Download (270 MB)").clicked() {
                            get_moonshine = true;
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    ui::hint(ui, "Until it is downloaded the assistant keeps using Whisper.");
                });
                ui.add_space(8.0);
            } else {
            for (file, label, mb) in WHISPER_MODELS {
                let have = models.join(file).exists();
                ui::row(ui, label, if have { "" } else { "not downloaded yet" }, |ui| {
                    if have {
                        let selected = self.cfg.whisper_model == *file;
                        let text = RichText::new(if selected { "In use" } else { "Use this" })
                            .size(12.5)
                            .color(if selected { ui::theme(ui).on_accent } else { ui::theme(ui).muted });
                        let button = egui::Button::new(text)
                            .fill(if selected { ui::theme(ui).accent } else { ui::theme(ui).inset })
                            .stroke(ui::theme(ui).hairline())
                            .corner_radius(egui::CornerRadius::same(8))
                            .min_size(egui::vec2(0.0, 26.0));
                        if ui.add_enabled(!selected, button).clicked() {
                            self.cfg.whisper_model = file.to_string();
                        }
                    } else if let Some(job) = self.job_running(file) {
                        let progress = job.0.lock().map(|s| s.progress).unwrap_or(-1.0);
                        ui.add(egui::ProgressBar::new(progress.max(0.0)).desired_width(140.0).fill(ui::theme(ui).accent));
                    } else if ui::ghost(ui, &format!("Download ({mb} MB)")).clicked() {
                        download = Some((file, mb));
                    }
                });
            }
            }
        });
        if get_moonshine {
            self.start_job(ctx, "moonshine", "Downloading Moonshine", move |p| {
                let dir = wisp_core::moonshine_dir();
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                let files = [
                    "preprocess.onnx",
                    "encode.int8.onnx",
                    "uncached_decode.int8.onnx",
                    "cached_decode.int8.onnx",
                    "tokens.txt",
                ];
                for (i, name) in files.iter().enumerate() {
                    let url = format!(
                        "https://huggingface.co/csukuangfj/sherpa-onnx-moonshine-base-en-int8/resolve/main/{name}"
                    );
                    let done = i as f32;
                    jobs::download_file(&url, &dir.join(name), &|fraction| {
                        p((done + fraction.max(0.0)) / files.len() as f32)
                    })?;
                }
                Ok("Moonshine installed — commands are transcribed in about a fifth of a second".to_string())
            });
        }
        if let Some((file, _)) = download {
            let dest = models.join(file);
            self.start_job(ctx, file, &format!("Downloading {file}"), move |p| {
                let url = format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{file}");
                jobs::download_file(&url, &dest, p).map(|_| format!("{file} downloaded"))
            });
        }
    }

    /// Whose mind answers: the model on this PC, or a hosted one with a key.
    fn tab_brain(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let t = ui::theme(ui);

        // ── choose the mind ─────────────────────────────────────────────
        let mut chosen = self.cfg.brain_kind;
        let half = (ui.available_width() - 44.0) / 2.0;
        ui::card(ui, "Who answers", "", |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                if ui::choice_tile(
                    ui,
                    chosen == BrainKind::Local,
                    "On this PC",
                    "Needle 3 — offline, instant, private, free",
                    half,
                )
                .on_hover_text(BrainKind::Local.hint())
                .clicked()
                {
                    chosen = BrainKind::Local;
                }
                ui.add_space(10.0);
                if ui::choice_tile(
                    ui,
                    chosen == BrainKind::Api,
                    "A hosted model",
                    "Stronger, and can answer questions — needs a key",
                    half,
                )
                .on_hover_text(BrainKind::Api.hint())
                .clicked()
                {
                    chosen = BrainKind::Api;
                }
            });
            ui.add_space(10.0);
            ui::hint(ui, chosen.hint());
        });
        if chosen != self.cfg.brain_kind {
            self.cfg.brain_kind = chosen;
            // A key that has never been set is the first thing to look at.
            self.model_list = None;
        }

        match self.cfg.brain_kind {
            BrainKind::Local => self.brain_local(ui),
            BrainKind::Api => self.brain_api(ui, ctx),
        }

        // ── what both of them share ─────────────────────────────────────
        ui::card_rows(ui, "Speed and memory", "How the model is run and when it is let go", |ui| {
            ui::row(ui, "Instant commands", "Skip the model for obvious requests like \"open chrome\"", |ui| {
                ui::switch(ui, &mut self.cfg.instant_commands, "");
            });
            ui::row(ui, "Search when an app is missing", "Otherwise it says it couldn't find it", |ui| {
                ui::switch(ui, &mut self.cfg.search_when_app_missing, "");
            });
            let max = std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(8);
            let want = wisp_core::config::recommended_threads();
            ui::row(ui, "CPU threads", &format!("This PC has {max} logical threads, about {want} real cores"), |ui| {
                ui.add(egui::Slider::new(&mut self.cfg.threads, 1..=max));
                if self.cfg.threads != want && ui::ghost(ui, &format!("Use {want}")).clicked() {
                    self.cfg.threads = want;
                }
            });
            ui::row(ui, "Free memory after", "Models are loaded only while needed", |ui| {
                ui.add(egui::Slider::new(&mut self.cfg.unload_after_secs, 10..=600).suffix(" s"));
            });
        });
        let _ = t;
    }

    /// Needle 3, on this machine.
    fn brain_local(&mut self, ui: &mut egui::Ui) {
        let t = ui::theme(ui);
        let model = wisp_core::paths::models_dir().join(wisp_core::NEEDLE_MODEL);
        let installed = model.exists();
        if !installed {
            ui::notice(
                ui,
                ui::Notice::Error,
                &format!(
                    "The model file is missing from {}. Without it, commands can only be matched against the \
                     instant phrase list.",
                    wisp_core::paths::models_dir().display()
                ),
            );
        }
        let mb = std::fs::metadata(&model).map(|m| m.len() / (1024 * 1024)).unwrap_or(0);
        ui::card_rows(ui, "Needle 3", "A 121M-parameter model that picks one of your functions", |ui| {
            ui::row(ui, "Status", "", |ui| {
                if installed {
                    ui::pill(ui, &format!("installed · {mb} MB"), t.ok);
                } else {
                    ui::pill(ui, "not installed", t.danger);
                    ui::hint(ui, "run scripts\\fetch-models.ps1");
                }
            });
            ui::row(ui, "Depth", "20 is the whole model; 12 is about a third faster", |ui| {
                ui.add(egui::Slider::new(&mut self.cfg.needle_depth, 6..=20));
            });
        });
        ui::hint(
            ui,
            "No key, no network, no account — and it never leaves this PC. It can pick a function, but it \
             cannot answer a general question; for that, switch to a hosted model.",
        );
    }

    /// A hosted model, with the user's key.
    fn brain_api(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let t = ui::theme(ui);
        let has_key = !wisp_core::secret::reveal(&self.cfg.api_key).trim().is_empty();
        if has_key {
            let model = if self.cfg.api_model.trim().is_empty() {
                self.cfg.api_provider.default_model().to_string()
            } else {
                self.cfg.api_model.clone()
            };
            ui::notice(
                ui,
                ui::Notice::Ok,
                &format!("{} will answer with {model}. Use Test the connection below to check the key.", self.cfg.api_provider.label()),
            );
        }
        if !has_key {
            ui::notice(
                ui,
                ui::Notice::Warn,
                "There is no key yet, so the assistant keeps using Needle 3 on this PC. Add one below to switch.",
            );
        }

        // ── service and key ─────────────────────────────────────────────
        let mut provider = self.cfg.api_provider;
        let mut save_key = false;
        let mut clear_key = false;
        ui::card(ui, "Service and key", "The key is stored encrypted by Windows, and only for this account", |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                for p in ApiProvider::ALL {
                    let selected = provider == p;
                    let text = RichText::new(p.short()).size(12.5).color(if selected { t.on_accent } else { t.muted });
                    let button = egui::Button::new(text)
                        .fill(if selected { t.accent } else { t.inset })
                        .stroke(t.hairline())
                        .corner_radius(egui::CornerRadius::same(8))
                        .min_size(egui::vec2(0.0, 28.0));
                    if ui.add(button).clicked() && !selected {
                        provider = p;
                    }
                }
            });
            if provider != self.cfg.api_provider {
                // The old model almost certainly belongs to the old service.
                let stale = self.cfg.api_model.is_empty()
                    || ApiProvider::ALL.iter().any(|p| p.default_model() == self.cfg.api_model);
                self.cfg.api_provider = provider;
                if stale {
                    self.cfg.api_model = provider.default_model().to_string();
                }
                self.model_list = None;
            }
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui::glyph(ui, ui::ICON_KEY, 13.0, if has_key { t.ok } else { t.faint });
                ui::key_field(ui, &mut self.api_key_draft, &mut self.show_key, 300.0);
                save_key = ui::primary(ui, "Save key", !self.api_key_draft.trim().is_empty()).clicked();
                if has_key {
                    if ui::ghost(ui, "Forget it").clicked() {
                        clear_key = true;
                    }
                    ui::pill(ui, &format!("key set · {}", wisp_core::secret::hint(&self.cfg.api_key)), t.ok);
                } else {
                    ui::switch(ui, &mut self.show_key, "show");
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let page = self.cfg.api_provider.keys_page();
                if !page.is_empty() {
                    ui::hint(ui, "Get a key:");
                    if ui::link(ui, page) {
                        let _ = win::shell_open(page, None);
                    }
                } else {
                    ui::hint(ui, "Anything that speaks the OpenAI API: Ollama, LM Studio, vLLM, a gateway.");
                }
            });
        });
        if save_key {
            let key = wisp_core::secret::protect(self.api_key_draft.trim());
            self.cfg.api_key = key;
            self.api_key_draft.clear();
            self.show_key = false;
            self.model_list = None;
            self.toast("Key saved — encrypted, and only usable by your Windows account", true);
        }
        if clear_key {
            self.cfg.api_key.clear();
            self.model_list = None;
            self.toast("Key removed — the assistant will use Needle 3 on this PC", true);
        }

        // ── model ───────────────────────────────────────────────────────
        let default_model = self.cfg.api_provider.default_model();
        let placeholder = if default_model.is_empty() { "the model name" } else { default_model };
        let mut fetch = false;
        ui::card_rows(ui, "Model", "Which model on that service answers", |ui| {
            ui::row(ui, "Model", if default_model.is_empty() { "" } else { "Empty uses the service's default" }, |ui| {
                ui::text_field(ui, &mut self.cfg.api_model, placeholder, 260.0);
                if ui::ghost(ui, "Fetch the list").clicked() {
                    fetch = true;
                }
                if let Some(list) = self.model_list.clone() {
                    egui::ComboBox::from_id_salt("models")
                        .width(260.0)
                        .selected_text(if self.cfg.api_model.is_empty() { placeholder.to_string() } else { self.cfg.api_model.clone() })
                        .show_ui(ui, |ui| {
                            for name in list {
                                ui.selectable_value(&mut self.cfg.api_model, name.clone(), name);
                            }
                        });
                }
            });
            ui::row(ui, "Thinking", "How long it may think before answering", |ui| {
                ui.horizontal(|ui| {
                    let mut thinking = self.cfg.api_thinking;
                    if ui::segmented(ui, &mut thinking, &Thinking::ALL.map(|th| (th, th.label()))) {
                        self.cfg.api_thinking = thinking;
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                ui::hint(ui, self.cfg.api_thinking.hint());
            });
            ui.add_space(8.0);
        });
        if fetch {
            let cfg = self.cfg.clone();
            self.start_job(ctx, "models", "Asking for the model list", move |_| {
                let list = wisp_core::chat::list_models(&cfg)?;
                Ok(list.join("\n"))
            });
        }
        // A finished fetch lands in the list the ComboBox reads.
        if let Some(job) = self.job_running("models") {
            let done = job.0.lock().map(|s| s.result.clone()).unwrap_or(None);
            if let Some(Ok(text)) = done {
                self.model_list = Some(text.lines().map(String::from).collect());
            }
        }

        // ── advanced ────────────────────────────────────────────────────
        ui::card(ui, "Advanced", "Usually left alone", |ui| {
            ui::field(ui, "Address", "Override the service's own URL — a proxy, a gateway, or a local server", |ui| {
                let hint_text = self.cfg.api_provider.default_base().to_string();
                ui::text_field(ui, &mut self.cfg.api_base_url, &hint_text, 420.0);
            });
            ui::row(ui, "Longest answer", "In tokens; a spoken answer needs very few", |ui| {
                ui.add(egui::Slider::new(&mut self.cfg.api_max_tokens, 64..=4096));
            });
            ui::row(ui, "Give up after", "How long to wait before falling back to this PC", |ui| {
                ui.add(egui::Slider::new(&mut self.cfg.api_timeout_secs, 5..=120).suffix(" s"));
            });
            ui::field(ui, "Style", "Warmth, brevity, and anything else you want it to remember", |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.cfg.api_system_prompt)
                        .hint_text("e.g. Always call me Captain. Keep answers under 20 words.")
                        .desired_rows(2)
                        .desired_width(420.0),
                );
            });
            ui::row(ui, "Temperature", "Lower is more predictable; higher is more inventive", |ui| {
                ui.add(egui::Slider::new(&mut self.cfg.api_temperature, 0.0..=1.5));
            });
        });

        // ── prove it works ──────────────────────────────────────────────
        let ready = has_key || self.cfg.api_provider == ApiProvider::Custom;
        ui.horizontal(|ui| {
            if ui::primary(ui, "Test the connection", ready).clicked() {
                let cfg = self.cfg.clone();
                self.start_job(ctx, "probe", "Asking the service", move |_| wisp_core::chat::probe(&cfg));
            }
            if self.job_running("probe").is_some() {
                ui.spinner();
            }
            ui::hint(ui, "Sends one short question, and costs a fraction of a cent.");
        });
        let _ = t;
    }

    /// Alarms, reminders, to-dos and calendar events.
    fn tab_schedule(&mut self, ui: &mut egui::Ui, accent: Color32) {
        let t = ui::theme(ui);

        // ── queued for later ────────────────────────────────────────────
        // Anything said with a "when" lives here until it happens: it is the
        // only place the queue can be seen, and the only place it can be
        // called off.
        let mut cancel: Option<u64> = None;
        let mut cancel_all = false;
        ui::card_rows(ui, "Queued for later", "Waiting for a moment, or for something to happen", |ui| {
            if self.pendings.items.is_empty() {
                ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width() - 28.0);
                        ui::hint(
                            ui,
                            "Say \"shut down in 30 minutes\", \"lock the pc at 11pm\", \"close chrome when spotify closes\", \
                             \"play a chime when the battery drops below 20\" — anything it can do now, it can do later.",
                        );
                    });
                });
                ui.add_space(8.0);
            } else {
                for entry in &self.pendings.items {
                    ui.horizontal(|ui| {
                        ui.add_space(14.0);
                        ui::dot(ui, t.accent, 6.0);
                        ui.vertical(|ui| {
                            ui.set_width(ui.available_width() - 70.0);
                            ui.label(RichText::new(&entry.label).size(13.0).color(t.text));
                            ui.label(RichText::new(entry.trigger.describe()).size(11.5).color(t.muted));
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui::icon_button(ui, ui::ICON_DELETE, "Cancel this").clicked() {
                                cancel = Some(entry.id);
                            }
                        });
                    });
                    ui.add_space(8.0);
                }
                ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    if ui::ghost(ui, "Cancel them all").clicked() {
                        cancel_all = true;
                    }
                });
                ui.add_space(6.0);
            }
        });
        if cancel_all {
            self.pendings.items.clear();
            if let Err(e) = self.pendings.save() {
                self.toast(format!("couldn't save the queue: {e}"), false);
            } else {
                self.toast("Queue cleared", true);
            }
            self.pendings_at = None;
        }
        if let Some(id) = cancel {
            if self.pendings.remove(id).is_some() {
                if let Err(e) = self.pendings.save() {
                    self.toast(format!("couldn't save the queue: {e}"), false);
                } else {
                    self.toast("Cancelled", true);
                }
            }
            self.pendings_at = None;
        }
        let _ = accent;
        let now = Stamp::now();
        let mut schedule = self.schedule.edit();
        let names: Vec<String> = self.cfg.custom_tools.iter().map(|t| t.name.clone()).collect();
        let mut dirty = false;

        // ── what's next ────────────────────────────────────────────────
        let next = schedule.next_up(now).map(|(at, item)| {
            format!("{} — {} at {}", item.kind.label(), item.text, describe_when(at, item.repeat, now))
        });
        section(ui, "Next up", |ui| {
            match &next {
                Some(line) => {
                    ui.label(RichText::new(line).color(accent).size(15.0));
                }
                None => hint(ui, "Nothing scheduled. Say \"hey <name>, set an alarm for 7:30 am\" or add one below."),
            }
            let todos = schedule.todos(false).len();
            if todos > 0 {
                hint(ui, &format!("{todos} thing(s) on the to-do list"));
            }
        });

        // ── add one ────────────────────────────────────────────────────
        section(ui, "Add", |ui| {
            ui.horizontal(|ui| {
                for kind in ItemKind::ALL {
                    ui.selectable_value(&mut self.new_item.kind, kind, kind.label());
                }
            });
            ui::row(ui, "What", "", |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_item.text)
                        .hint_text(match self.new_item.kind {
                            ItemKind::Alarm => "wake up",
                            ItemKind::Reminder => "call my mum",
                            ItemKind::Todo => "buy milk",
                            ItemKind::Event => "team lunch",
                        })
                        .desired_width(300.0),
                );
            });
            if self.new_item.kind.timed() {
                let mut when = self.new_item.when.clone();
                ui::row(ui, "When", "7:30 am · in 20 minutes · tomorrow at 9 · every monday at 8", |ui| {
                    if ui.add(egui::TextEdit::singleline(&mut when).desired_width(300.0)).changed() {
                        self.new_item.when = when.clone();
                    }
                });
                // Say what we understood, as it is typed.
                if self.new_item.when.trim().is_empty() {
                    hint(ui, "Spoken times work as written: \"in 20 minutes\", \"tonight\", \"every weekday at 8\".");
                } else {
                    match wisp_core::schedule_parse::parse(&self.new_item.when, now) {
                        Ok((at, repeat)) => {
                            let text = describe_when(at, repeat, now);
                            ui.label(RichText::new(format!("→ {text}")).color(accent));
                        }
                        Err(e) => {
                            ui.label(RichText::new(format!("→ {e}")).color(ui::theme(ui).warn));
                        }
                    }
                }
            }
            if self.new_item.kind.timed() {
                ui::row(ui, "Repeat", "", |ui| {
                    egui::ComboBox::from_id_salt("new-repeat")
                        .width(180.0)
                        .selected_text(self.new_item.repeat.label())
                        .show_ui(ui, |ui| {
                            for r in Repeat::ALL {
                                if ui.selectable_value(&mut self.new_item.repeat, r, r.label()).clicked() {
                                    // A repeating item always has a time.
                                    if self.new_item.when.trim().is_empty() {
                                        self.new_item.when = "9 am".into();
                                    }
                                }
                            }
                        });
                });
            }
            ui::row(ui, "Run a function", "Runs when it goes off, e.g. open your work apps", |ui| {
                egui::ComboBox::from_id_salt("new-action")
                    .width(220.0)
                    .selected_text(if self.new_item.action.is_empty() { "(nothing)" } else { &self.new_item.action })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.new_item.action, String::new(), "(nothing)");
                        for name in &names {
                            ui.selectable_value(&mut self.new_item.action, name.clone(), pretty_name(name));
                        }
                    });
                ui::switch(ui, &mut self.new_item.chime, "chime");
                ui::switch(ui, &mut self.new_item.speak, "speak");
            });

            let needs_time = self.new_item.kind.timed();
            // A to-do has no time, so nothing the "when" box contains matters —
            // including whatever was left in it from the last kind that was
            // selected. It used to be parsed anyway, which left Add greyed out
            // whenever that leftover text was not a time.
            let parsed = if needs_time {
                wisp_core::schedule_parse::parse(&self.new_item.when, now)
            } else {
                Ok((now, Repeat::Once))
            };
            let ready = !self.new_item.text.trim().is_empty() && parsed.is_ok();
            if ui.add_enabled(ready, egui::Button::new(RichText::new("＋  Add").size(13.0))).clicked() {
                match parsed {
                    Ok((at, repeat)) => {
                        let repeat = if needs_time { repeat } else { Repeat::Once };
                        let id = schedule.add(Item {
                            kind: self.new_item.kind,
                            text: self.new_item.text.trim().to_string(),
                            at: needs_time.then_some(at),
                            repeat,
                            action: self.new_item.action.clone(),
                            chime: self.new_item.chime,
                            speak: self.new_item.speak,
                            ..Default::default()
                        });
                        let _ = id;
                        dirty = true;
                        let label = self.new_item.text.trim().to_string();
                        self.new_item = NewItem { kind: self.new_item.kind, ..Default::default() };
                        self.toast(format!("Added \"{label}\""), true);
                    }
                    Err(e) => self.toast(e, false),
                }
            }
        });

        // ── the list ───────────────────────────────────────────────────
        let title = if schedule.items.is_empty() {
            "Nothing yet".to_string()
        } else {
            format!("Scheduled ({})", schedule.items.len())
        };
        section(ui, &title, |ui| {
            // Soonest first; finished to-dos at the bottom. Sorting by index
            // rather than by id keeps the ids unique even if a hand-edited file
            // repeats one — the lookup below used to assume that could not
            // happen and would panic on the assumption.
            let mut order: Vec<usize> = (0..schedule.items.len()).collect();
            order.sort_by_key(|i| {
                let item = &schedule.items[*i];
                (item.done, item.next_after(now).unwrap_or(Stamp::new(9999, 1, 1, 0, 0)))
            });
            let ids: Vec<u32> = order.iter().map(|i| schedule.items[*i].id).collect();
            let mut remove: Option<u32> = None;
            let mut test: Option<String> = None;
            ui.set_width(ui.available_width());
            for id in ids {
                let Some(index) = schedule.items.iter().position(|i| i.id == id) else { continue };
                let item = &mut schedule.items[index];
                // Two lines per item: when and how on top, what and which
                // function underneath. One line would need a wider window than
                // most people have.
                ui.horizontal(|ui| {
                    let mut done = item.done;
                    if ui::switch(ui, &mut done, "").on_hover_text("Done").changed() {
                        item.done = done;
                        item.fired = done.then_some(now);
                        dirty = true;
                    }
                    ui.add_sized([70.0, 18.0], egui::Label::new(RichText::new(item.kind.label()).color(accent).strong()));

                    if item.kind.timed() {
                        let stored = item.at.map(|at| at.date() + " " + &at.clock()).unwrap_or_default();
                        let key = format!("when:{id}");
                        let mut when = self.buffer(&key, &stored);
                        let resp = ui.add(egui::TextEdit::singleline(&mut when).desired_width(115.0));
                        if resp.changed() {
                            let text = when.clone();
                            if let Some(at) = Stamp::parse_iso(&text) {
                                item.at = Some(at);
                                dirty = true;
                            } else if let Ok((at, repeat)) = wisp_core::schedule_parse::parse(&text, now) {
                                item.at = Some(at);
                                if repeat != Repeat::Once || item.repeat != Repeat::Once {
                                    item.repeat = repeat;
                                }
                                dirty = true;
                            }
                        }
                        // Once it parses, the field snaps back to the canonical
                        // form; until then the half-typed text stays put.
                        if when != stored && (Stamp::parse_iso(&when).is_some() || wisp_core::schedule_parse::parse(&when, now).is_ok()) {
                            self.drafts.remove(&key);
                        } else {
                            self.keep(&key, when);
                        }
                        // The item's own time, not the resolved next occurrence:
                        // "every day at 7:30 am" must not read "today at 7:30 am",
                        // and a spent one-off says so instead of showing a time.
                        let label = if item.done {
                            item.time_phrase(now).trim().to_string()
                        } else {
                            match item.next_after(now) {
                                None => "gone".to_string(),
                                Some(_) => {
                                    let repeat = item.repeat_label();
                                    let repeat =
                                        if repeat.is_empty() { String::new() } else { format!(" {repeat}") };
                                    format!("{}{repeat}", item.time_phrase(now).trim())
                                }
                            }
                        };
                        ui.add_sized(
                            [170.0, 18.0],
                            egui::Label::new(RichText::new(label).small().color(ui::theme(ui).faint)),
                        );
                        egui::ComboBox::from_id_salt(format!("rep{id}"))
                            .width(170.0)
                            .selected_text(item.repeat.label())
                            .show_ui(ui, |ui| {
                                for r in Repeat::ALL {
                                    if ui.selectable_value(&mut item.repeat, r, r.label()).clicked() {
                                        dirty = true;
                                    }
                                }
                            });
                    } else {
                        ui.add_sized([355.0, 18.0], egui::Label::new(""));
                    }
                    if ui::icon_button(ui, ui::ICON_PLAY, "Say it now").clicked() {
                        test = Some(wisp_core::schedule::announcement(item, now));
                    }
                    if ui::icon_button(ui, ui::ICON_DELETE, "Delete").clicked() {
                        remove = Some(id);
                    }
                });
                ui.horizontal(|ui| {
                    ui.add_space(24.0);
                    let mut text = item.text.clone();
                    if ui
                        .add(egui::TextEdit::singleline(&mut text).hint_text("what to say").desired_width(260.0))
                        .changed()
                    {
                        item.text = text;
                        dirty = true;
                    }
                    let action_label = if item.action.is_empty() {
                        "run no function".to_string()
                    } else {
                        pretty_name(&item.action)
                    };
                    egui::ComboBox::from_id_salt(format!("act{id}"))
                        .width(190.0)
                        .selected_text(action_label)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut item.action, String::new(), "run no function");
                            for name in &names {
                                if ui.selectable_value(&mut item.action, name.clone(), pretty_name(name)).clicked() {
                                    dirty = true;
                                }
                            }
                        });
                    if item.chime {
                        ui::glyph(ui, "\u{E7ED}", 11.0, ui::theme(ui).faint);
                    }
                });
                ui.add_space(2.0);
            }
            if let Some(id) = remove {
                schedule.remove(id);
                dirty = true;
            }
            if let Some(line) = test {
                self.preview_voice(Some(line));
            }
        });

        if dirty {
            if let Err(e) = self.schedule.store(schedule) {
                self.toast(format!("Couldn't save: {e}"), false);
            }
        }

        let mut export = false;
        section(ui, "Take it with you", |ui| {
            hint(ui, "Alarms, reminders and events can be exported as an .ics file — import it into Outlook, Google Calendar or your phone.");
            if ui::button(ui, "Export as calendar file").clicked() {
                export = true;
            }
        });
        if export {
            let dir = wisp_core::win::documents_folder().unwrap_or_else(wisp_core::paths::data_dir);
            let path = dir.join("Wisp.ics");
            match std::fs::write(&path, self.schedule.read().to_ics(now)) {
                Ok(()) => {
                    let _ = win::shell_open(&dir.display().to_string(), None);
                    self.toast(format!("Wrote {}", path.display()), true);
                }
                Err(e) => self.toast(format!("Couldn't write the calendar file: {e}"), false),
            }
        }
        hint(
            ui,
            &format!(
                "Kept in {} — the assistant announces these even while the microphone is paused.",
                Schedule::path().display()
            ),
        );
    }

    /// Where Python is, and where its scripts go. Only shown when a Python
    /// function exists or the interpreter needs attention.
    fn python_ui(&mut self, ui: &mut egui::Ui) {
        let wants_python = self.cfg.custom_tools.iter().any(|t| t.kind == CustomKind::Python);
        // Looking for Python walks the install folders and, failing that, every
        // entry on PATH. Asking once a second is plenty and keeps the page
        // responsive on a machine that does not have it at all.
        let changed = self.cfg.python_path != self.python_searched;
        if self.python_checked.elapsed() > Duration::from_secs(1) || changed {
            self.python_checked = Instant::now();
            self.python_searched = self.cfg.python_path.clone();
            self.python_found = wisp_core::win::find_python(&self.cfg.python_path);
        }
        let found = self.python_found.clone();
        if !wants_python && found.is_some() {
            return; // nothing to say: it just works
        }
        ui::card(ui, "Python scripts", "Where its interpreter is, and where scripts live", |ui| {
            match &found {
                Some(path) => {
                    ui::pill(ui, &format!("Python found · {}", path.display()), ui::theme(ui).ok);
                }
                None => {
                    ui::pill(ui, "Python wasn't found", ui::theme(ui).warn);
                    ui::hint(ui, "Install it from python.org, or point at python.exe below.");
                    hint(ui, "The Windows Store alias for python.exe doesn't count — install Python from python.org.");
                }
            }
            ui.horizontal(|ui| {
                ui.label("python.exe");
                ui.add(
                    egui::TextEdit::singleline(&mut self.cfg.python_path)
                        .hint_text("leave empty to find it automatically")
                        .desired_width(400.0),
                );
            });
            ui.horizontal(|ui| {
                if ui::button(ui, "Open scripts folder").clicked() {
                    let _ = win::shell_open(&self.cfg.scripts_dir().display().to_string(), None);
                }
                if ui::button(ui, "Look again").clicked() {
                    self.cfg.python_path = String::new();
                }
            });
            hint(ui, "A script prints its answer; the last line is spoken. Parameters arrive as arguments and as WISP_PARAM_<NAME>.");
        });
    }

    /// The tuning numbers, folded away. The defaults suit almost everyone, but
    /// a quiet voice or a noisy room can want more.
    fn wake_advanced(&mut self, ui: &mut egui::Ui) {
        ui.add_space(2.0);
        egui::CollapsingHeader::new(RichText::new("Advanced wake tuning").size(12.5).color(ui::theme(ui).muted))
            .id_salt("wakeadv")
            .show(ui, |ui| {
                let automatic = wisp_core::wake_model::Tuning::is_automatic(&self.cfg);
                let mut manual = !automatic;
                ui::row(ui, "Set the trigger by hand", "Otherwise the sensitivity slider decides", |ui| {
                    if ui::switch(ui, &mut manual, "").changed() {
                        if manual {
                            let t = wisp_core::wake_model::Tuning::automatic(&self.cfg);
                            self.cfg.wake_score = t.boost;
                            self.cfg.wake_threshold = t.threshold;
                        } else {
                            self.cfg.wake_score = 0.0;
                            self.cfg.wake_threshold = 0.0;
                        }
                    }
                });
                ui.add_enabled_ui(manual, |ui| {
                    ui::row(ui, "Keyword boost", "Higher is easier to trigger", |ui| {
                        ui.add(egui::Slider::new(&mut self.cfg.wake_score, 0.5..=6.0));
                    });
                    ui::row(ui, "Trigger threshold", "Lower is easier to trigger", |ui| {
                        ui.add(egui::Slider::new(&mut self.cfg.wake_threshold, 0.02..=0.9));
                    });
                    ui.horizontal(|ui| {
                        ui.add_space(14.0);
                        if ui::ghost(ui, "Reset to automatic").clicked() {
                            self.cfg.wake_score = 0.0;
                            self.cfg.wake_threshold = 0.0;
                        }
                        let t = wisp_core::wake_model::Tuning::for_config(&self.cfg);
                        ui.label(
                            RichText::new(format!("now: boost {:.2}, threshold {:.3}", t.boost, t.threshold))
                                .size(11.5)
                                .color(ui::theme(ui).faint),
                        );
                    });
                    ui.add_space(6.0);
                });
            });
    }

    /// What the spotter is listening for, straight from the tokeniser.
    fn wake_preview(&self, ui: &mut egui::Ui) {
        let t = ui::theme(ui);
        let Some(bpe) = &self.bpe else { return };
        let phrases = wisp_core::wake_model::wake_phrases(&self.cfg);
        ui::row(ui, "Answers to", "", |ui| {
            ui.label(RichText::new(phrases.join("  ·  ")).size(12.5).color(t.text));
        });
        let odd = wisp_core::wake_model::unpronounceable(&self.cfg, bpe);
        if !odd.is_empty() {
            ui::row(ui, "Unpronounceable", "The model has no sound for these", |ui| {
                ui::pill(ui, &format!("{} — try a simpler name", odd.join(", ")), t.warn);
            });
        }
        egui::CollapsingHeader::new(RichText::new("What the model is given").size(11.5).color(t.faint))
            .id_salt("tokens")
            .show(ui, |ui| {
                let tokens = wisp_core::wake_model::keyword_lines(&self.cfg, bpe).replace('▁', " ");
                ui.add_space(4.0);
                ui.label(RichText::new(tokens).size(11.0).monospace().color(t.faint));
            });
    }

    /// Install or remove the small keyword model the instant wake-up needs.
    fn wake_model_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, accent: Color32) {
        let _ = accent;
        let installed = wisp_core::wake_model::is_installed();
        let t = ui::theme(ui);
        let mut install = false;
        let mut remove = false;
        ui::card_rows(
            ui,
            "Wake word",
            if installed { "Heard the moment you say it, without Whisper" } else { "Needs the small keyword model" },
            |ui| {
                ui::row(ui, "Spotter", "", |ui| {
                    if installed {
                        ui::pill(ui, "instant", t.ok);
                    } else {
                        ui::pill(ui, "Whisper fallback", t.warn);
                    }
                });
                if installed {
                    self.wake_preview(ui);
                } else {
                    hint(
                        ui,
                        "Without it, waking waits for Whisper to finish transcribing the whole sentence. One 5 MB download, then it works offline.",
                    );
                }
                ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    if !installed {
                        if let Some(job) = self.job_running("wake_model") {
                            let progress = job.0.lock().map(|s| s.progress).unwrap_or(-1.0);
                            ui.add(egui::ProgressBar::new(progress.max(0.0)).desired_width(200.0).fill(t.accent));
                        } else if ui::ghost(ui, "Download the wake-word model (5 MB)").clicked() {
                            install = true;
                        }
                    } else if ui::ghost(ui, "Remove the model").clicked() {
                        remove = true;
                    }
                    if ui::ghost(ui, "Hear the wake chime").clicked() {
                        let _ = win::spawn_detached(&Self::agent_exe(), &["--chime"]);
                    }
                });
                ui.add_space(6.0);
            },
        );
        if install {
            self.start_job(ctx, "wake_model", "Downloading the wake-word model", |p| {
                wisp_core::wake_model::download(|d, t| p(if t > 0 { d as f32 / t as f32 } else { -1.0 }))
                    .map(|_| "Wake-word model installed — waking is instant now".to_string())
            });
        }
        if remove {
            match wisp_core::wake_model::remove() {
                Ok(()) => {
                    self.bpe = None;
                    self.toast("Removed the wake-word model", true);
                }
                Err(e) => self.toast(format!("Couldn't remove it: {e}"), false),
            }
        }
    }

    fn mic_check_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, accent: Color32) {
        let Some(mc) = self.mic_check.as_mut() else { return };
        mc.tick(ctx, &self.cfg);
        let wake = self.cfg.wake_phrase();
        let mut apply: Option<miccheck::Calibration> = None;
        ui::card(ui, "Microphone check", "Watch the bars, then let it tune itself to your voice and room", |ui| {
            hint(ui, "Talk and watch the bars — the mic you speak into should jump.");
            ui.add_space(4.0);
            for m in &mc.mics {
                let lvl = m.level();
                let frac = ((lvl + 70.0) / 60.0).clamp(0.0, 1.0);
                ui.horizontal(|ui| {
                    let mut name = m.name.clone();
                    if m.is_default {
                        name.push_str("  (Windows default)");
                    }
                    if self.cfg.microphone == m.name || (self.cfg.microphone.is_empty() && m.is_default) {
                        name.push_str("  ← in use");
                    }
                    ui.add_sized([330.0, 18.0], egui::Label::new(RichText::new(name).small()).truncate());
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(260.0, 12.0), egui::Sense::hover());
                    let p = ui.painter();
                    p.rect_filled(rect, 4.0, ui::theme(ui).inset);
                    let mut fill = rect;
                    fill.set_width(rect.width() * frac);
                    p.rect_filled(fill, 4.0, accent);
                    ui.label(RichText::new(format!("{lvl:>4.0} dB")).monospace().small());
                });
            }
            ui.add_space(8.0);
            match &mc.phase {
                miccheck::Phase::Idle => {
                    if ui.add(egui::Button::new(RichText::new("▶  Run microphone check").strong())).clicked() {
                        mc.start();
                    }
                }
                miccheck::Phase::Quiet(_) => {
                    let s = miccheck::remaining(&mc.phase).unwrap_or(0.0);
                    ui.label(RichText::new(format!("Step 1 of 2 — stay quiet… {s:.0}")).size(16.0).color(accent));
                    hint(ui, "Measuring your room's background noise.");
                }
                miccheck::Phase::Speak(_) => {
                    ui.label(RichText::new(format!("Step 2 of 2 — now say: \"{wake}, open Notepad\"")).size(16.0).strong().color(accent));
                    hint(ui, "Speak normally, like you would to the assistant. It stops automatically when you finish.");
                }
                miccheck::Phase::Analyzing => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Listening back to what you said…");
                    });
                }
            }

            if let Some(cal) = &mc.result {
                ui.separator();
                for r in &cal.mics {
                    let good = cal.best.as_deref() == Some(r.name.as_str());
                    let text = format!(
                        "{} {}  — background {:.0} dB, your voice {:.0} dB (+{:.0} dB clearer)",
                        if good { "✔" } else { "•" },
                        r.name,
                        r.noise_db,
                        r.speech_db,
                        r.snr().max(0.0)
                    );
                    ui.label(RichText::new(text).color(if good { accent } else { ui::theme(ui).muted }));
                }
                match &cal.best {
                    None => {
                        ui.label(
                            RichText::new("Couldn't hear you on any microphone.")
                                .strong()
                                .color(ui::theme(ui).danger),
                        );
                        hint(ui, "Check the mic isn't muted (Windows Settings → System → Sound → Input) and that desktop apps are allowed to use it (Privacy & security → Microphone), then try again.");
                    }
                    Some(best) => {
                        match &cal.transcript {
                            Some(t) if cal.wake_ok => {
                                ui.label(RichText::new(format!("✔ Heard: \"{t}\" — wake word recognised!")).strong().color(accent));
                            }
                            Some(t) => {
                                ui.label(RichText::new(format!("Heard: \"{t}\"")).strong());
                                match cal.sensitivity {
                                    Some(s) => hint(ui, &format!("The name wasn't recognised at your current sensitivity — it will be raised to {:.2}.", s + 0.05)),
                                    None => hint(ui, &format!("It didn't catch \"{wake}\". Try again speaking a little slower, or pick a more distinctive name on the General tab.")),
                                }
                            }
                            None => hint(ui, "Couldn't run speech recognition — the levels below will still be applied."),
                        }
                        ui.label(format!(
                            "Recommended: use \"{best}\", boost +{:.0} dB, speech threshold {:.0} dB, noise filtering {}",
                            cal.gain_db, cal.min_speech_db, cal.vad
                        ));
                        if ui.add(egui::Button::new(RichText::new("✔  Apply & save these settings").strong().color(Color32::BLACK)).fill(accent)).clicked() {
                            apply = Some(cal.clone());
                        }
                    }
                }
            }
        });
        if let Some(cal) = apply {
            if let Some(best) = cal.best {
                self.cfg.microphone = best;
            }
            self.cfg.mic_gain_db = cal.gain_db;
            self.cfg.min_speech_db = cal.min_speech_db;
            self.cfg.vad_aggressiveness = cal.vad;
            if !cal.wake_ok {
                if let Some(s) = cal.sensitivity {
                    self.cfg.wake_sensitivity = (s + 0.05).min(1.0);
                }
            }
            self.save();
        }
    }

    fn tab_browser(&mut self, ui: &mut egui::Ui, accent: Color32) {
        let _ = accent;
        ui::card_rows(ui, "Where links open", "", |ui| {
            ui::row(ui, "Which browser", "", |ui| {
                let mut browser = self.cfg.browser;
                if ui::segmented(ui, &mut browser, &Browser::ALL.map(|b| (b, b.label()))) {
                    self.cfg.browser = browser;
                }
            });
            if self.cfg.browser == Browser::Custom {
                ui::row(ui, "Path to the .exe", "", |ui| {
                    ui::text_field(ui, &mut self.cfg.browser_path, r"C:\Program Files\...", 320.0);
                });
            }
            match (&self.cfg.browser, wisp_core::actions::browser_exe(&self.cfg)) {
                (Browser::Default, _) => {
                    ui::row(ui, "Status", "", |ui| {
                        ui::pill(ui, "Windows default", ui::theme(ui).muted);
                    });
                }
                (_, Some(found)) => {
                    let name = found.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    ui::row(ui, "Found", &found.display().to_string(), |ui| {
                        ui::pill(ui, &name, ui::theme(ui).ok);
                    });
                }
                (_, None) => {
                    ui::row(ui, "Found", "The default browser will be used instead", |ui| {
                        ui::pill(ui, "not on this PC", ui::theme(ui).warn);
                    });
                }
            }
        });

        ui::card(ui, "Search engine", "Used for questions and \"search for ...\"", |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                for (name, url) in SEARCH_ENGINES {
                    let selected = self.cfg.search_url == *url;
                    let text = RichText::new(name.to_string()).size(12.5).color(if selected {
                        ui::theme(ui).on_accent
                    } else {
                        ui::theme(ui).muted
                    });
                    let button = egui::Button::new(text)
                        .fill(if selected { ui::theme(ui).accent } else { ui::theme(ui).inset })
                        .stroke(ui::theme(ui).hairline())
                        .corner_radius(egui::CornerRadius::same(8))
                        .min_size(egui::vec2(0.0, 26.0));
                    if ui.add(button).clicked() {
                        self.cfg.search_url = url.to_string();
                    }
                }
            });
            ui.add_space(10.0);
            ui::field(ui, "Address", "{} is replaced with what you asked", |ui| {
                ui::text_field(ui, &mut self.cfg.search_url, "https://...?q={}", 420.0);
            });
        });
    }

    fn tab_apps(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, accent: Color32) {
        let _ = accent;
        let mut rescan = false;
        ui::card_rows(ui, "Custom names", "Say \"open code\" for Visual Studio Code", |ui| {
            let mut remove = None;
            for (i, a) in self.cfg.aliases.iter().enumerate() {
                let (phrase, target) = (a.phrase.clone(), a.target.clone());
                ui::row(ui, &format!("\"{phrase}\""), "", |ui| {
                    ui.label(RichText::new(target).size(13.0).color(ui::theme(ui).muted));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui::icon_button(ui, ui::ICON_DELETE, "Remove").clicked() {
                            remove = Some(i);
                        }
                    });
                });
            }
            if let Some(i) = remove {
                self.cfg.aliases.remove(i);
            }
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                ui::text_field(ui, &mut self.new_alias.0, "when I say...", 150.0);
                ui::glyph(ui, ui::ICON_CHEVRON_RIGHT, 12.0, ui::theme(ui).faint);
                ui::text_field(ui, &mut self.new_alias.1, "this app, URL or path", 250.0);
                if ui::ghost(ui, "Add").clicked()
                    && !self.new_alias.0.trim().is_empty()
                    && !self.new_alias.1.trim().is_empty()
                {
                    self.cfg.aliases.push(Alias {
                        phrase: self.new_alias.0.trim().to_lowercase(),
                        target: self.new_alias.1.trim().into(),
                    });
                    self.new_alias = Default::default();
                }
            });
            ui.add_space(6.0);
        });

        let filter = self.app_filter.to_lowercase();
        let shown = self.apps.apps.iter().filter(|a| a.name.to_lowercase().contains(&filter)).count();
        let total = self.apps.apps.len();
        let scanning = self.job_running("scan").is_some();
        let mut toggled: Vec<(String, bool)> = Vec::new();
        ui::card(ui, &format!("Detected apps ({total})"), "New ones are picked up every 30 minutes", |ui| {
            ui.horizontal(|ui| {
                ui::glyph(ui, ui::ICON_SEARCH, 13.0, ui::theme(ui).faint);
                ui::text_field(ui, &mut self.app_filter, "filter", 200.0);
                if scanning {
                    ui.spinner();
                    ui.label(RichText::new("scanning...").size(12.0).color(ui::theme(ui).muted));
                } else if ui::ghost(ui, "Rescan now").clicked() {
                    rescan = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("{shown} shown, untick to hide from voice"))
                            .size(11.5)
                            .color(ui::theme(ui).faint),
                    );
                });
            });
            ui.add_space(8.0);
            let excluded = self.cfg.excluded_apps.clone();
            egui::ScrollArea::vertical().max_height(340.0).id_salt("apps").show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                for app in self.apps.apps.iter().filter(|a| a.name.to_lowercase().contains(&filter)) {
                    let name = app.name.clone();
                    let mut on = !excluded.iter().any(|e| e.eq_ignore_ascii_case(&name));
                    ui.horizontal(|ui| {
                        if ui::switch(ui, &mut on, "").changed() {
                            toggled.push((name.clone(), on));
                        }
                        let colour = if on { ui::theme(ui).text } else { ui::theme(ui).faint };
                        ui.label(RichText::new(&name).size(13.0).color(colour));
                    });
                }
            });
        });
        for (name, on) in toggled {
            if on {
                self.cfg.excluded_apps.retain(|e| !e.eq_ignore_ascii_case(&name));
            } else {
                self.cfg.excluded_apps.push(name);
            }
        }
        if rescan {
            let cfg = self.cfg.clone();
            self.start_job(ctx, "scan", "Scanning apps", move |_| {
                let idx = AppIndex::scan(&cfg);
                let n = idx.apps.len();
                idx.save_cache();
                Ok(format!("Found {n} apps"))
            });
        }
    }

    /// Everything Needle is allowed to do: the built-ins it may call, and the
    /// functions you wrote yourself.
    fn tab_functions(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, _accent: Color32) {
        let _ = ctx;
        self.python_ui(ui);

        let off = self.cfg.disabled_tools.len();
        let on = tools::BUILTINS.len() - off;
        let t = ui::theme(ui);
        let filter = self.fn_filter.trim().to_lowercase();
        let matching = |b: &tools::Builtin| {
            filter.is_empty()
                || b.name.contains(&filter)
                || b.description.to_lowercase().contains(&filter)
                || b.group.to_lowercase().contains(&filter)
                || b.example.to_lowercase().contains(&filter)
        };
        let shown = tools::BUILTINS.iter().filter(|b| matching(b)).count();
        ui::card_rows(
            ui,
            "Built-in functions",
            "Understood instantly, without waiting for the model",
            |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    ui::pill(ui, &format!("{on} on"), if off == 0 { t.ok } else { t.warn });
                    if off > 0 {
                        ui::pill(ui, &format!("{off} off"), t.muted);
                    }
                    ui.add_space(10.0);
                    ui::glyph(ui, ui::ICON_SEARCH, 12.0, t.faint);
                    ui::text_field(ui, &mut self.fn_filter, "find a function", 180.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(if filter.is_empty() {
                                format!("{} functions", tools::BUILTINS.len())
                            } else {
                                format!("{shown} matching")
                            })
                            .size(11.5)
                            .color(t.faint),
                        );
                    });
                });
                ui.add_space(10.0);
                egui::ScrollArea::vertical().max_height(430.0).id_salt("builtins").show(ui, |ui| {
                    for group in tools::GROUPS {
                        let items: Vec<&'static tools::Builtin> =
                            tools::BUILTINS.iter().filter(|b| b.group == group && matching(b)).collect();
                        if items.is_empty() {
                            continue;
                        }
                        let off_here = items.iter().filter(|b| self.cfg.disabled_tools.iter().any(|d| d == b.name)).count();
                        ui::group_label(
                            ui,
                            group,
                            &if off_here == 0 {
                                format!("{} available", items.len())
                            } else {
                                format!("{} on, {off_here} off", items.len() - off_here)
                            },
                        );
                        for tool in items {
                            let mut enabled = !self.cfg.disabled_tools.iter().any(|d| d == tool.name);
                            ui.horizontal(|ui| {
                                ui.add_space(14.0);
                                if ui::switch(ui, &mut enabled, "").changed() {
                                    if enabled {
                                        self.cfg.disabled_tools.retain(|d| d != tool.name);
                                    } else {
                                        self.cfg.disabled_tools.push(tool.name.to_string());
                                    }
                                }
                                let colour = if enabled { t.text } else { t.faint };
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(pretty_name(tool.name)).size(12.5).color(colour));
                                        ui.label(
                                            RichText::new(format!("\u{201C}{}\u{201D}", tool.example))
                                                .size(11.0)
                                                .color(t.faint),
                                        );
                                    });
                                    if !filter.is_empty() {
                                        ui.label(RichText::new(tool.description).size(11.0).color(t.muted));
                                    }
                                });
                            });
                            ui.add_space(5.0);
                        }
                        ui.add_space(6.0);
                    }
                    if shown == 0 {
                        ui::empty(ui, ui::ICON_SEARCH, "Nothing matches", "Try another word — or clear the box to see everything.");
                    }
                });
            },
        );

        // ── your own functions ──────────────────────────────────────────
        let mut delete: Option<usize> = None;
        let mut try_it: Option<usize> = None;
        let count = self.cfg.custom_tools.len();
        let function_names: Vec<String> = self.cfg.custom_tools.iter().map(|t| t.name.clone()).collect();
        let scripts_dir = self.cfg.scripts_dir();
        let custom_title = if count == 0 { "Your functions".to_string() } else { format!("Your functions ({count})") };
        section(ui, &custom_title, |ui| {
            hint(
                ui,
                "A trigger phrase runs these without the model at all, so they work even offline. Put {query} in a phrase to capture what you said.",
            );
            if count == 0 {
                hint(ui, "Nothing here yet — add one from a template below.");
            }
            for i in 0..count {
                // Edited as a local copy: fields whose stored form is a list
                // (parameters, phrases) need an editing buffer of their own, and
                // that means touching `self` while the function is borrowed.
                let mut tool = self.cfg.custom_tools[i].clone();
                let label =
                    if tool.enabled { pretty_name(&tool.label()) } else { format!("{} (off)", pretty_name(&tool.label())) };
                // The card is identified by its position, never by the name
                // inside it. The name is edited *within* the card, and an id that
                // follows it gives the whole subtree a new identity on every
                // keystroke: the box the person is typing in stops existing, and
                // the next frame hands focus back to nobody. That was the "it
                // deselects after one letter" bug.
                egui::CollapsingHeader::new(label)
                    .id_salt(format!("fn{i}"))
                    .default_open(true)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui::switch(ui, &mut tool.enabled, "Enabled");
                            ui.separator();
                            ui.label("name");
                            ui.add(egui::TextEdit::singleline(&mut tool.name).desired_width(150.0));
                        });
                        hint(ui, "Use it as the name Needle calls; keep it snake_case, e.g. focus_mode.");
                        ui.horizontal(|ui| {
                            ui.label("what it does");
                            ui.add(egui::TextEdit::singleline(&mut tool.description).desired_width(360.0));
                        });
                        hint(ui, "Needle reads this to decide when to use it, so say it plainly.");
                        ui.horizontal(|ui| {
                            ui.label("parameters");
                            let key = format!("params:{i}");
                            let mut params = self.buffer(&key, &tool.params.join(", "));
                            if ui.add(egui::TextEdit::singleline(&mut params).hint_text("query").desired_width(240.0)).changed() {
                                tool.params = params
                                    .split(',')
                                    .map(tools::sanitize_name)
                                    .filter(|p| !p.is_empty())
                                    .collect();
                            }
                            self.keep(&key, params);
                        });
                        hint(ui, "Comma-separated. Use them below as {name}.");
                        ui.horizontal(|ui| {
                            ui.label("action");
                            egui::ComboBox::from_id_salt(format!("kind{i}"))
                                .width(280.0)
                                .selected_text(tool.kind.label())
                                .show_ui(ui, |ui| {
                                    for k in CustomKind::ALL {
                                        ui.selectable_value(&mut tool.kind, k, k.label());
                                    }
                                });
                        });
                        hint(ui, tool.kind.hint());
                        if tool.kind.uses_target() {
                            ui.horizontal(|ui| {
                                ui.label(match tool.kind {
                                    CustomKind::OpenUrl => "URL",
                                    CustomKind::Python => "script",
                                    _ => "target",
                                });
                                ui.add(
                                    egui::TextEdit::singleline(&mut tool.target)
                                        .hint_text(tool.kind.target_hint())
                                        .desired_width(400.0),
                                );
                            });
                        }
                        if tool.kind == CustomKind::Run {
                            ui.horizontal(|ui| {
                                ui.label("arguments");
                                ui.add(egui::TextEdit::singleline(&mut tool.args).desired_width(400.0));
                            });
                        }
                        if tool.kind == CustomKind::Python {
                            ui.horizontal(|ui| {
                                ui.label("arguments");
                                ui.add(
                                    egui::TextEdit::singleline(&mut tool.args)
                                        .hint_text("optional — {param} placeholders, or empty to pass the parameters in order")
                                        .desired_width(400.0),
                                );
                            });
                            hint(
                                ui,
                                &format!(
                                    "Scripts live in {} — parameters also arrive as WISP_PARAM_<NAME>. Whatever the script prints last is spoken.",
                                    scripts_dir.display()
                                ),
                            );
                        }
                        if tool.kind == CustomKind::Sequence {
                            ui.label("steps, in order");
                            let mut remove_step: Option<usize> = None;
                            let mut move_step: Option<(usize, i32)> = None;
                            let steps = tool.steps.len();
                            for si in 0..steps {
                                let step = &mut tool.steps[si];
                                ui.horizontal(|ui| {
                                    egui::ComboBox::from_id_salt(format!("step{i}-{si}"))
                                        .width(160.0)
                                        .selected_text(step.kind.label())
                                        .show_ui(ui, |ui| {
                                            for k in StepKind::ALL {
                                                ui.selectable_value(&mut step.kind, k, k.label());
                                            }
                                        });
                                    ui.add(
                                        egui::TextEdit::singleline(&mut step.target)
                                            .hint_text(step.kind.hint())
                                            .desired_width(300.0),
                                    );
                                    if step.kind.needs_args() {
                                        ui.add(
                                            egui::TextEdit::singleline(&mut step.args)
                                                .hint_text("arguments")
                                                .desired_width(150.0),
                                        );
                                    }
                                    if si > 0 && ui::icon_button(ui, "\u{E70E}", "Move up").clicked() {
                                        move_step = Some((si, -1));
                                    }
                                    if si + 1 < steps && ui::icon_button(ui, "\u{E70D}", "Move down").clicked() {
                                        move_step = Some((si, 1));
                                    }
                                    if ui::icon_button(ui, ui::ICON_DELETE, "Remove this step").clicked() {
                                        remove_step = Some(si);
                                    }
                                });
                            }
                            if let Some(si) = remove_step {
                                tool.steps.remove(si);
                            }
                            if let Some((si, delta)) = move_step {
                                let to = (si as i32 + delta).clamp(0, tool.steps.len() as i32 - 1) as usize;
                                tool.steps.swap(si, to);
                            }
                            if ui::ghost(ui, "+  Add a step").clicked() {
                                tool.steps.push(Step::default());
                            }
                            hint(
                                ui,
                                &format!(
                                    "Use {{output}} for what the previous script printed, and {{param}} for your parameters. \
                                     Both spellings work, with one brace or two. Functions you can call: {}",
                                    if function_names.is_empty() { "(none yet)".to_string() } else { function_names.join(", ") }
                                ),
                            );
                        }
                        let phrase_key = format!("phrases:{i}");
                        let mut phrases = self.buffer(&phrase_key, &tool.phrases.join("\n"));
                        ui.label("say any of these");
                        phrases_field(ui, &mut phrases, &mut tool.phrases);
                        self.keep(&phrase_key, phrases);
                        hint(ui, "One per line — press Enter for the next one. A missing parameter captures the rest of the sentence.");
                        ui.horizontal(|ui| {
                            ui.label("reply");
                            ui.add(
                                egui::TextEdit::singleline(&mut tool.reply)
                                    .hint_text("say this afterwards")
                                    .desired_width(360.0),
                            );
                        });
                        hint(ui, "Leave empty for a normal \"done\" line.");
                        ui.horizontal(|ui| {
                            if ui::button(ui, "Try it").clicked() {
                                try_it = Some(i);
                            }
                            if ui::button(ui, "Delete").clicked() {
                                delete = Some(i);
                            }
                        });
                    });
                // Keep whatever the editor did with this function.
                self.cfg.custom_tools[i] = tool;
            }
        });
        if let Some(i) = delete {
            let removed = self.cfg.custom_tools.remove(i);
            // Drafts are keyed by position, and removing one moves every later
            // function up: keeping them would show one card's half-typed text in
            // another's field. They are mirrors of the stored values, so dropping
            // them costs nothing.
            self.drafts.clear();
            self.toast(format!("Removed \"{}\" — save to apply", pretty_name(&removed.label())), true);
        }
        if let Some(i) = try_it {
            self.try_function(ctx, i);
        }
        // ── start from a template ───────────────────────────────────────
        let mut add: Option<CustomTool> = None;
        let mut add_blank = false;
        section(ui, "Add a function", |ui| {
            hint(ui, "Templates are ready to use — add one, then tweak it. They respect the switches in your config, so shutdown and sleep still ask nothing of Needle.");
            ui.horizontal_wrapped(|ui| {
                // The catalogue is built once per frame rather than once per
                // button: `templates()` builds every step of every sequence.
                let templates = tools::templates();
                for t in &templates {
                    if ui::button(ui, &format!("+ {}", pretty_name(&t.label()))).on_hover_text(&t.description).clicked() {
                        add = Some(t.clone());
                    }
                }
                if ui::ghost(ui, "+ Blank function").clicked() {
                    add_blank = true;
                }
            });
        });
        if add_blank {
            add = Some(CustomTool { name: "new_function".into(), description: String::new(), ..Default::default() });
        }
        if let Some(t) = add {
            let label = pretty_name(&t.label());
            self.add_function(t);
            self.toast(format!("Added \"{label}\" — its settings are open above. Save to apply."), true);
        }
    }

    /// Add a function, renaming it if the name is taken.
    fn add_function(&mut self, mut tool: CustomTool) {
        if tool.kind == CustomKind::Python {
            let target = tool.target.clone();
            self.write_starter_script(&target);
        }
        let base = tools::sanitize_name(if tool.name.is_empty() { "new_function" } else { &tool.name });
        let mut name = if base.is_empty() { "new_function".to_string() } else { base };
        let mut n = 2;
        while self.cfg.custom_tools.iter().any(|t| t.name == name) || tools::is_builtin(&name) {
            name = format!("{}{n}", name.trim_end_matches(|c: char| c.is_ascii_digit()));
            n += 1;
        }
        tool.name = name;
        self.cfg.custom_tools.push(tool);
    }

    /// Give a new Python function something that actually runs: a small script
    /// in the scripts folder that shows how parameters arrive.
    fn write_starter_script(&mut self, target: &str) {
        let name = target.trim().trim_matches('"');
        if name.is_empty() || name.contains(['/', '\\']) {
            return;
        }
        let path = self.cfg.scripts_dir().join(if name.ends_with(".py") { name.to_string() } else { format!("{name}.py") });
        if path.exists() {
            return;
        }
        let starter = "# Called by Wisp.\n\
             # Every parameter arrives as an argument and as WISP_PARAM_<NAME>.\n\
             # The last line you print is the answer that gets spoken.\n\
             import os\n\
             import sys\n\
             \n\
             question = sys.argv[1] if len(sys.argv) > 1 else os.environ.get(\"WISP_PARAM_QUESTION\", \"\")\n\
             print(\"You asked:\", question)\n\
             print(\"Change me: this is what gets said.\")\n";
        if let Err(e) = std::fs::write(&path, starter) {
            self.toast(format!("Couldn't write {}: {e}", path.display()), false);
        } else {
            self.toast(format!("Created {} — edit it to do something useful", path.display()), true);
        }
    }

    /// Understand a command the way the microphone would — and, when asked,
    /// actually do it. Shared by the dashboard's command box and the Test page.
    fn run_command(&mut self, ctx: &egui::Context, command: String, execute: bool) {
        if self.test_job.is_some() || command.trim().is_empty() {
            return;
        }
        self.test_command = command.clone();
        let cfg = self.cfg.clone();
        let brain = self.brain.clone();
        self.test_job = Some(Job::spawn(ctx, "Thinking", move |_| {
            let apps = AppIndex::load_cache().unwrap_or_default();
            let mut guard = brain.lock().unwrap();
            let b = guard.get_or_insert_with(|| {
                Brain::new(wisp_core::paths::models_dir().join(wisp_core::NEEDLE_MODEL), cfg.needle_depth)
            });
            let d = b.decide(&command, &cfg, &apps);
            let mut out = format!("Decided via {:?} in {} ms:\n", d.via, d.millis);
            for a in &d.actions {
                out.push_str(&format!("  • {a}\n"));
            }
            if let Some(r) = &d.reasoning {
                out.push_str(&format!("\nIt thought: {r}\n"));
            }
            let reply = wisp_core::personality::dry_reply(&cfg, &d);
            if reply.starts_with('(') {
                out.push_str(&format!("\nIt would say: {reply}\n"));
            } else {
                out.push_str(&format!("\nIt would say: \"{reply}\"\n"));
            }
            if execute {
                let index = AppIndex::load_cache().unwrap_or_default();
                for a in &d.actions {
                    match wisp_core::actions::execute(a, &cfg, &index) {
                        Ok(m) => out.push_str(&format!("  ✔ {m}\n")),
                        Err(e) => out.push_str(&format!("  ✖ {e}\n")),
                    }
                }
            }
            Ok(out)
        }));
    }

    /// Collect what the command runner produced, and remember it for the
    /// history on the Test page.
    fn poll_test(&mut self) {
        let Some(job) = &self.test_job else { return };
        let result = job.0.lock().ok().and_then(|s| s.result.clone());
        if let Some(result) = result {
            let text = result.unwrap_or_else(|e| e);
            self.test_result = Some(text.clone());
            self.test_history.insert(0, (self.test_command.clone(), text));
            self.test_history.truncate(8);
            self.test_job = None;
        }
    }

    /// Run one of the user's functions with sample arguments, so the result
    /// (and any error) can be seen without talking to the microphone.
    fn try_function(&mut self, ctx: &egui::Context, index: usize) {
        let Some(tool) = self.cfg.custom_tools.get(index).cloned() else { return };
        let cfg = self.cfg.clone();
        let params: Vec<(String, String)> = tool.params.iter().map(|p| (p.clone(), format!("<{p}>"))).collect();
        let name = tool.name.clone();
        let label = format!("Running {}", pretty_name(&tool.label()));
        self.start_job(ctx, "try_function", &label, move |_| {
            let apps = AppIndex::load_cache().unwrap_or_default();
            // A function that only speaks has no action to run — its reply is
            // the whole result.
            if tool.kind == CustomKind::SayOnly {
                let reply = wisp_core::personality::reply(
                    &cfg,
                    &[wisp_core::personality::Outcome::Done(
                        wisp_core::brain::Action::Custom { name: name.clone(), params },
                        String::new(),
                    )],
                );
                return Ok(format!("It would say: {reply}"));
            }
            let action = wisp_core::brain::Action::Custom { name: name.clone(), params };
            match wisp_core::actions::execute(&action, &cfg, &apps) {
                Ok(msg) => Ok(format!("✓ {msg}")),
                Err(e) => Ok(format!("✗ {e}")),
            }
        });
    }

    fn tab_test(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let t = ui::theme(ui);
        let mut run = false;
        let mut execute = false;
        ui::card(ui, "Command", "What you would say after the wake word", |ui| {
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.test_input)
                    .hint_text("open chrome, find the file budget, what's my battery")
                    .desired_width(f32::INFINITY)
                    .margin(egui::Margin::symmetric(9, 7)),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                run = ui::primary(ui, "Understand", true).clicked()
                    || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                execute = ui::button(ui, "Understand and do it").clicked();
                if self.test_job.is_some() {
                    ui.spinner();
                    ui::hint(ui, "thinking…");
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui::hint(ui, "Nothing is spoken and the microphone is not used.");
                });
            });
        });
        if run || execute {
            let command = self.test_input.clone();
            self.run_command(ctx, command, execute);
        }

        if self.test_result.is_none() && self.test_history.is_empty() && self.test_job.is_none() {
            ui::card(ui, "", "", |ui| {
                ui::empty(
                    ui,
                    ui::ICON_SEARCH,
                    "Nothing tried yet",
                    "Type a command above and press Understand — it costs nothing and nothing is spoken.",
                );
            });
        }

        if let Some(result) = &self.test_result {
            let lines: Vec<(String, Color32)> = result
                .lines()
                .map(|l| {
                    let trim = l.trim_start();
                    let colour = if trim.starts_with('✔') {
                        t.ok
                    } else if trim.starts_with('✖') {
                        t.danger
                    } else if l.starts_with("It would say") {
                        t.accent
                    } else if l.starts_with("Decided") {
                        t.faint
                    } else {
                        t.text
                    };
                    (l.to_string(), colour)
                })
                .collect();
            ui::card(ui, "Result", "", |ui| {
                ui::code_lines(ui, &lines, 260.0);
            });
        }

        if !self.test_history.is_empty() {
            let mut recall: Option<String> = None;
            ui::card_rows(ui, "Recent commands", "Click one to try it again", |ui| {
                for (command, _result) in self.test_history.iter() {
                    ui.horizontal(|ui| {
                        ui.add_space(14.0);
                        ui.vertical(|ui| {
                            ui.set_width(ui.available_width() - 40.0);
                            if ui
                                .add(egui::Label::new(RichText::new(command).size(12.5).color(t.text)).sense(egui::Sense::click()))
                                .on_hover_text("Run it again")
                                .clicked()
                            {
                                recall = Some(command.clone());
                            }
                        });
                    });
                    ui.add_space(6.0);
                }
            });
            if let Some(command) = recall {
                self.test_input = command;
            }
        }
    }
}

fn phrases_field(ui: &mut egui::Ui, draft: &mut String, stored: &mut Vec<String>) -> egui::Response {
    let response = ui.add(egui::TextEdit::multiline(draft).desired_rows(3).desired_width(400.0));
    if response.changed() {
        *stored = draft.lines().map(|l| l.trim().to_lowercase()).filter(|l| !l.is_empty()).collect();
    }
    response
}

fn accent32((r, g, b): (f32, f32, f32)) -> Color32 {
    Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

fn pretty_name(s: &str) -> String {
    let spaced = s.replace('_', " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => spaced,
    }
}

fn hint(ui: &mut egui::Ui, text: &str) {
    ui::hint(ui, text);
}

fn section(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    ui::card(ui, title, "", body);
}

#[cfg(test)]
mod typing_tests {
    use super::*;

    /// One frame of a UI, with the input events egui would have received.
    fn frame(
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        ui_fn: impl FnOnce(&mut egui::Ui) -> egui::Response,
    ) -> egui::Response {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))),
            events,
            ..Default::default()
        };
        let mut out = None;
        let mut ui_fn = Some(ui_fn);
        let mut frame = ctx.run_ui(input, |ui| {
            if let Some(f) = ui_fn.take() {
                out = Some(f(ui));
            }
        });
        // There is no renderer here to consume the font atlas.
        frame.textures_delta.clear();
        out.expect("a frame")
    }

    fn click(at: egui::Pos2) -> Vec<egui::Event> {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        vec![egui::Event::PointerMoved(at), button(true), button(false)]
    }

    fn press_enter() -> egui::Event {
        egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }
    }

    /// What the user reported: pressing Enter in "say any of these" added
    /// nothing, because the box was redrawn from the parsed list every frame.
    #[test]
    fn enter_starts_a_new_line() {
        let ctx = egui::Context::default();
        // Let egui settle: it needs a frame before widgets have a size.
        let mut stored: Vec<String> = Vec::new();
        let mut draft = String::new();
        let mut rect = egui::Rect::NOTHING;
        for _ in 0..2 {
            rect = frame(&ctx, Vec::new(), |ui| phrases_field(ui, &mut draft, &mut stored)).rect;
        }

        // Click into the box, then type a line, press Enter, type another.
        frame(&ctx, click(rect.center()), |ui| phrases_field(ui, &mut draft, &mut stored));
        frame(&ctx, vec![egui::Event::Text("say something funny".into())], |ui| {
            phrases_field(ui, &mut draft, &mut stored)
        });
        frame(&ctx, vec![press_enter()], |ui| phrases_field(ui, &mut draft, &mut stored));
        frame(&ctx, vec![egui::Event::Text("tell me a joke".into())], |ui| {
            phrases_field(ui, &mut draft, &mut stored)
        });

        assert!(draft.contains('\n'), "the newline must survive into the next frame: {draft:?}");
        assert_eq!(stored, vec!["say something funny".to_string(), "tell me a joke".to_string()]);

        // A blank line is ignored in the list but kept in the box.
        frame(&ctx, vec![press_enter()], |ui| phrases_field(ui, &mut draft, &mut stored));
        assert_eq!(stored.len(), 2);
        assert!(draft.ends_with('\n'), "{draft:?}");
    }

    /// The card around a function's name field, exactly as the Functions page
    /// builds it. `salt_from_text` chooses between the id that follows the name —
    /// what the page used to do — and the fixed one it does now.
    fn card(ui: &mut egui::Ui, text: &mut String, salt_from_text: bool) -> egui::Response {
        let salt = if salt_from_text { format!("fn{text}") } else { "fn0".to_string() };
        let mut field = None;
        egui::CollapsingHeader::new("Card").id_salt(salt).default_open(true).show(ui, |ui| {
            field = Some(ui.add(egui::TextEdit::singleline(text)));
        });
        field.expect("the field")
    }

    /// Does the name box still have focus one keystroke later?
    fn focus_survives_a_keystroke(salt_from_text: bool) -> bool {
        let ctx = egui::Context::default();
        let mut text = String::new();
        for _ in 0..2 {
            frame(&ctx, Vec::new(), |ui| card(ui, &mut text, salt_from_text));
        }
        // Focus the box, type one character, then look at where focus is.
        let first = frame(&ctx, Vec::new(), |ui| card(ui, &mut text, salt_from_text));
        first.request_focus();
        frame(&ctx, vec![egui::Event::Text("h".into())], |ui| card(ui, &mut text, salt_from_text));
        let after = frame(&ctx, Vec::new(), |ui| card(ui, &mut text, salt_from_text));
        assert_eq!(text, "h", "the keystroke must reach the box");
        ctx.memory(|m| m.focused()) == Some(after.id)
    }

    /// What the user reported: "when I type into the new function name box, after
    /// pressing one key it deselects my box."
    ///
    /// The card's id was built from the function's name — the very text being
    /// edited inside it — so the first keystroke gave the whole subtree a new
    /// identity. egui keeps focus by widget id, so the box that had focus no
    /// longer existed on the next frame and focus fell back to nothing. The id is
    /// now the card's position, which no keystroke can change.
    #[test]
    fn a_card_identified_by_the_name_it_edits_loses_focus() {
        assert!(!focus_survives_a_keystroke(true), "this test no longer reproduces the bug");
        assert!(focus_survives_a_keystroke(false), "a fixed id must keep focus");
    }
}

#[cfg(test)]
mod schedule_store_tests {
    use super::*;

    /// A schedule file of our own, so the tests never touch the real list.
    struct TempSchedule {
        path: PathBuf,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    /// The override is an environment variable, so tests using it take turns.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    impl TempSchedule {
        fn new(tag: &str) -> TempSchedule {
            let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let dir = std::env::temp_dir().join("wisp-config-schedule-test");
            let _ = std::fs::create_dir_all(&dir);
            let path = dir.join(format!("{tag}-{}.json", std::process::id()));
            let _ = std::fs::remove_file(&path);
            std::env::set_var("WISP_SCHEDULE_FILE", &path);
            TempSchedule { path, _lock: lock }
        }
    }

    impl Drop for TempSchedule {
        fn drop(&mut self) {
            std::env::remove_var("WISP_SCHEDULE_FILE");
            let _ = std::fs::remove_file(&self.path);
        }
    }

    /// The page is drawn sixty times a second, so the store must hand back the
    /// same list without touching the file — but it must also notice an edit
    /// made by the agent or by hand, and never read back its own half-finished
    /// edit.
    #[test]
    fn the_schedule_is_cached_until_it_changes() {
        let _guard = TempSchedule::new("store");
        let mut store = ScheduleStore::default();

        assert!(store.read().items.is_empty());
        let mut edit = store.edit();
        edit.add(Item { kind: ItemKind::Todo, text: "buy milk".into(), ..Default::default() });
        store.store(edit).unwrap();
        assert_eq!(store.read().items.len(), 1, "the store must keep what it just saved");

        // Another writer (the agent, or a text editor) changes the file.
        let mut other = Schedule::open();
        other.add(Item { kind: ItemKind::Alarm, text: "wake up".into(), ..Default::default() });
        other.save().unwrap();
        assert_eq!(store.read().items.len(), 2, "an outside change must be picked up");

        // An edit that is thrown away must not come back from the cache.
        let abandoned = store.edit();
        drop(abandoned);
        assert_eq!(store.read().items.len(), 2);

        // Nothing is written until `store` is called.
        let mut edit = store.edit();
        edit.clear(None);
        assert_eq!(Schedule::open().items.len(), 2, "an uncommitted edit must stay in memory");
        store.store(edit).unwrap();
        assert!(Schedule::open().items.is_empty());
    }
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    // `--miccheck`: run the microphone check with no window and write the
    // result to %APPDATA%\Wisp\miccheck.txt. Useful on a machine whose
    // audio needs diagnosing, and it exercises the whole wizard — opening the
    // microphones, both steps and the analysis — without a UI to click.
    if args.iter().any(|a| a == "--miccheck") {
        let cfg = Config::load();
        let ctx = egui::Context::default();
        let started = std::time::Instant::now();
        let body = match miccheck::run_to_completion(&cfg, &ctx, Duration::from_secs(45)) {
            Ok(cal) => miccheck::describe(&cal),
            Err(e) => format!("{e}\n"),
        };
        let text = format!(
            "Wisp microphone check — {} (took {:.1}s)\n\n{body}",
            Stamp::now().date() + " " + &Stamp::now().clock(),
            started.elapsed().as_secs_f32()
        );
        let path = wisp_core::paths::data_dir().join("miccheck.txt");
        let _ = std::fs::write(&path, &text);
        log::info!("mic check written to {}", path.display());
        return Ok(());
    }

    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Wisp Settings")
            // The window draws its own title bar, so the frame is ours.
            .with_decorations(false)
            .with_inner_size([1020.0, 740.0])
            .with_min_inner_size([880.0, 580.0]),
        ..Default::default()
    };
    eframe::run_native("Wisp Settings", opts, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}

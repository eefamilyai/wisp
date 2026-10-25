//! What "control my computer" actually means, one operation at a time.
//!
//! Everything here is something a person would otherwise do with the mouse:
//! switch to a window, half-screen it, find a document, put something on the
//! clipboard, type it out, ask how much battery is left, go to sleep. Each
//! operation returns the sentence the assistant says afterwards, written the
//! way a helpful person would say it — "Chrome was in the background; it's in
//! front now" is the whole answer, so these all count as answers rather than
//! as actions that need a personality line afterwards.
//!
//! The Win32 calls live in [`crate::win`]; the tool definitions that expose
//! these live in [`crate::tools`]; the decision that picks one lives in
//! [`crate::brain`]. This module is only the doing.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::apps::AppIndex;
use crate::fuzzy;
use crate::win;

/// One thing the assistant can do to the machine.
///
/// Named after the tool that triggers it (`Op::tool`), because the catalogue in
/// [`crate::tools`] is the list people see, and two lists of names for the same
/// thing drift apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    // ── Windows ────────────────────────────────────────────────────────
    FocusWindow,
    ActiveWindow,
    ListWindows,
    NextWindow,
    PreviousWindow,
    MinimizeWindow,
    MaximizeWindow,
    RestoreWindow,
    SnapWindow,
    MoveToMonitor,
    // ── Files and notes ────────────────────────────────────────────────
    OpenFolder,
    FindFile,
    OpenFile,
    RecycleFile,
    EmptyRecycleBin,
    FolderSize,
    TakeNote,
    ReadNotes,
    SearchNotes,
    // ── Clipboard and typing ───────────────────────────────────────────
    CopyToClipboard,
    ReadClipboard,
    PasteClipboard,
    TypeText,
    // ── The system ─────────────────────────────────────────────────────
    BatteryStatus,
    SystemStatus,
    TopProcesses,
    DiskSpace,
    NetworkStatus,
    MachineInfo,
    IsAppRunning,
    SleepPc,
    ShutdownPc,
    RestartPc,
    SignOut,
    CancelShutdown,
    TurnOffScreen,
    BrightnessSet,
    BrightnessUp,
    BrightnessDown,
    CopyScreenshot,
}

impl Op {
    pub const ALL: [Op; 40] = [
        Op::FocusWindow,
        Op::ActiveWindow,
        Op::ListWindows,
        Op::NextWindow,
        Op::PreviousWindow,
        Op::MinimizeWindow,
        Op::MaximizeWindow,
        Op::RestoreWindow,
        Op::SnapWindow,
        Op::MoveToMonitor,
        Op::OpenFolder,
        Op::FindFile,
        Op::OpenFile,
        Op::RecycleFile,
        Op::EmptyRecycleBin,
        Op::FolderSize,
        Op::TakeNote,
        Op::ReadNotes,
        Op::SearchNotes,
        Op::CopyToClipboard,
        Op::ReadClipboard,
        Op::PasteClipboard,
        Op::TypeText,
        Op::BatteryStatus,
        Op::SystemStatus,
        Op::TopProcesses,
        Op::DiskSpace,
        Op::NetworkStatus,
        Op::MachineInfo,
        Op::IsAppRunning,
        Op::SleepPc,
        Op::ShutdownPc,
        Op::RestartPc,
        Op::SignOut,
        Op::CancelShutdown,
        Op::TurnOffScreen,
        Op::BrightnessSet,
        Op::BrightnessUp,
        Op::BrightnessDown,
        Op::CopyScreenshot,
    ];

    /// The catalogue name, which is also what the brain calls.
    pub fn tool(self) -> &'static str {
        match self {
            Op::FocusWindow => "focus_window",
            Op::ActiveWindow => "active_window",
            Op::ListWindows => "list_windows",
            Op::NextWindow => "next_window",
            Op::PreviousWindow => "previous_window",
            Op::MinimizeWindow => "minimize_window",
            Op::MaximizeWindow => "maximize_window",
            Op::RestoreWindow => "restore_window",
            Op::SnapWindow => "snap_window",
            Op::MoveToMonitor => "move_to_monitor",
            Op::OpenFolder => "open_folder",
            Op::FindFile => "find_file",
            Op::OpenFile => "open_file",
            Op::RecycleFile => "recycle_file",
            Op::EmptyRecycleBin => "empty_recycle_bin",
            Op::FolderSize => "folder_size",
            Op::TakeNote => "take_note",
            Op::ReadNotes => "read_notes",
            Op::SearchNotes => "search_notes",
            Op::CopyToClipboard => "copy_to_clipboard",
            Op::ReadClipboard => "read_clipboard",
            Op::PasteClipboard => "paste_clipboard",
            Op::TypeText => "type_text",
            Op::BatteryStatus => "battery_status",
            Op::SystemStatus => "system_status",
            Op::TopProcesses => "top_processes",
            Op::DiskSpace => "disk_space",
            Op::NetworkStatus => "network_status",
            Op::MachineInfo => "machine_info",
            Op::IsAppRunning => "is_app_running",
            Op::SleepPc => "sleep_pc",
            Op::ShutdownPc => "shutdown_pc",
            Op::RestartPc => "restart_pc",
            Op::SignOut => "sign_out",
            Op::CancelShutdown => "cancel_shutdown",
            Op::TurnOffScreen => "turn_off_screen",
            Op::BrightnessSet => "brightness_set",
            Op::BrightnessUp => "brightness_up",
            Op::BrightnessDown => "brightness_down",
            Op::CopyScreenshot => "copy_screenshot",
        }
    }

    /// The operation behind a tool name, if there is one.
    pub fn from_tool(name: &str) -> Option<Op> {
        Op::ALL.iter().copied().find(|op| op.tool() == name)
    }
}

/// Which required parameter is missing, if any.
///
/// The catalogue marks a parameter optional with a leading `?` in its hint.
/// A call that lacks a required one is a half-heard command — "switch to" with
/// no window named — and the assistant asks again instead of guessing.
pub fn missing_required(op: Op, args: &[(String, String)]) -> Option<&'static str> {
    let builtin = crate::tools::builtin(op.tool())?;
    builtin.params.iter().find_map(|(name, hint)| {
        if hint.starts_with('?') {
            return None;
        }
        let given = args.iter().any(|(k, v)| k.eq_ignore_ascii_case(name) && !v.trim().is_empty());
        (!given).then_some(*name)
    })
}

impl std::fmt::Display for Op {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.tool())
    }
}

/// Where the user's own notes live: `%APPDATA%\Wisp\notes.md`, a plain file they
/// can open, edit or sync without going through this program.
pub fn notes_file() -> PathBuf {
    crate::paths::data_dir().join("notes.md")
}

/// The value of a parameter, or "" when it was not given.
fn arg<'a>(args: &'a [(String, String)], name: &str) -> &'a str {
    args.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.trim())
        .unwrap_or("")
}

/// Run one operation and return what to say about it.
pub fn run(op: Op, args: &[(String, String)], cfg: &crate::Config, apps: &AppIndex) -> Result<String, String> {
    match op {
        // ── Windows ────────────────────────────────────────────────────
        Op::FocusWindow => focus_window(arg(args, "app"), cfg, apps),
        Op::ActiveWindow => {
            let w = win::foreground_window().ok_or("I can't tell what's in front — nothing has focus")?;
            Ok(format!("you're in \"{}\" ({})", w.spoken_title(), w.app))
        }
        Op::ListWindows => list_windows(),
        Op::NextWindow => switch_window(true),
        Op::PreviousWindow => switch_window(false),
        Op::MinimizeWindow => window_state("minimized", win::WindowState::Minimize),
        Op::MaximizeWindow => window_state("maximized", win::WindowState::Maximize),
        Op::RestoreWindow => window_state("back to its normal size", win::WindowState::Restore),
        Op::SnapWindow => snap_window(arg(args, "side")),
        Op::MoveToMonitor => move_to_monitor(arg(args, "which")),
        // ── Files and notes ────────────────────────────────────────────
        Op::OpenFolder => open_folder(arg(args, "folder")),
        Op::FindFile => find_file(arg(args, "name")),
        Op::OpenFile => open_file(arg(args, "name")),
        Op::RecycleFile => recycle_file(arg(args, "name")),
        Op::EmptyRecycleBin => win::empty_recycle_bin().map(|_| "emptied the recycle bin".to_string()),
        Op::FolderSize => folder_size(arg(args, "folder")),
        Op::TakeNote => take_note(arg(args, "text")),
        Op::ReadNotes => read_notes(),
        Op::SearchNotes => search_notes(arg(args, "query")),
        // ── Clipboard and typing ───────────────────────────────────────
        Op::CopyToClipboard => copy_to_clipboard(arg(args, "text")),
        Op::ReadClipboard => read_clipboard(),
        Op::PasteClipboard => win::press_keys("ctrl+v").map(|_| "pasted it".to_string()),
        Op::TypeText => type_text(arg(args, "text")),
        // ── The system ─────────────────────────────────────────────────
        Op::BatteryStatus => battery_status(),
        Op::SystemStatus => system_status(),
        Op::TopProcesses => top_processes(arg(args, "what")),
        Op::DiskSpace => disk_space(),
        Op::NetworkStatus => network_status(),
        Op::MachineInfo => machine_info(),
        Op::IsAppRunning => is_app_running(arg(args, "app"), cfg, apps),
        Op::SleepPc => win::sleep_pc().map(|_| "going to sleep".to_string()),
        Op::ShutdownPc => win::power_off("shutdown", 30)
            .map(|_| "shutting down in 30 seconds — say \"cancel the shutdown\" to stop it".to_string()),
        Op::RestartPc => win::power_off("restart", 30)
            .map(|_| "restarting in 30 seconds — say \"cancel the shutdown\" to stop it".to_string()),
        Op::SignOut => win::power_off("signout", 0).map(|_| "signing you out".to_string()),
        Op::CancelShutdown => win::power_off("cancel", 0).map(|_| "cancelled the shutdown".to_string()),
        Op::TurnOffScreen => win::monitor_off().map(|_| "screen off — any key brings it back".to_string()),
        Op::BrightnessSet => brightness_set(arg(args, "percent")),
        Op::BrightnessUp => brightness_nudge(true),
        Op::BrightnessDown => brightness_nudge(false),
        Op::CopyScreenshot => win::copy_screenshot_to_clipboard()
            .map(|(w, h)| format!("copied the screen ({w}×{h}) to the clipboard")),
    }
}

// ── Windows ──────────────────────────────────────────────────────────────

/// How alike a spoken name and a window have to be before we switch to it.
/// Deliberately high: switching to the wrong window is worse than saying the
/// app is not open, because the user is left typing into something they did
/// not ask for.
const WINDOW_MATCH: f64 = 0.74;

/// The window that best answers to `want`, if any.
fn best_window(want: &str) -> Option<win::WindowRef> {
    let want = fuzzy::strip_filler(want);
    let want = fuzzy::squash(&want);
    let windows = win::switchable_windows();
    if want.is_empty() {
        return None;
    }
    let mut best: Option<(f64, win::WindowRef)> = None;
    for w in windows {
        let app = fuzzy::squash(&w.app);
        let title = fuzzy::squash(&w.title);
        let score: f64 = if app == want {
            1.0
        } else if title == want {
            0.9
        } else if !app.is_empty() && app.contains(&want) {
            0.88
        } else if title.contains(&want) {
            0.82
        } else if !app.is_empty() && want.contains(&app) {
            0.8
        } else {
            fuzzy::similarity(&app, &want).max(fuzzy::similarity(&title, &want) * 0.9)
        };
        if score >= WINDOW_MATCH && best.as_ref().is_none_or(|(b, _)| score > *b) {
            best = Some((score, w));
        }
    }
    best.map(|(_, w)| w)
}

fn focus_window(want: &str, cfg: &crate::Config, apps: &AppIndex) -> Result<String, String> {
    if want.trim().is_empty() {
        return Err("which window?".into());
    }
    if let Some(w) = best_window(want) {
        if !win::focus_window(w.hwnd) {
            // Windows refuses to hand the foreground to a background process
            // sometimes; the window is raised, so say so honestly.
            return Ok(format!("\"{}\" is up, but Windows kept the focus where it was", w.spoken_title()));
        }
        return Ok(format!("switched to \"{}\"", w.spoken_title()));
    }
    // Nothing open under that name. Starting the app is what the person meant
    // nine times out of ten, and it is better than an error for "show me code".
    if let Some((app, _)) = apps.find(want, cfg) {
        app.launch()?;
        return Ok(format!("{} wasn't open, so I started it", app.name));
    }
    Err(format!("nothing called \"{want}\" is open"))
}

fn list_windows() -> Result<String, String> {
    let windows = win::switchable_windows();
    if windows.is_empty() {
        return Ok("there are no windows open".into());
    }
    // Titles are what people recognise, but they are long; five is as many as
    // anyone can hold in their head from one sentence.
    let mut names: Vec<String> = Vec::new();
    for w in windows.iter().take(5) {
        let name = w.spoken_title();
        if !names.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
            names.push(name);
        }
    }
    let more = windows.len().saturating_sub(names.len());
    let mut said = format!("{} window{} open: {}", windows.len(), if windows.len() == 1 { "" } else { "s" }, names.join(", "));
    if more > 0 {
        said.push_str(&format!(", and {more} more"));
    }
    Ok(said)
}

fn switch_window(forward: bool) -> Result<String, String> {
    // The real Alt+Tab, so the switch looks and behaves exactly like the
    // keyboard's own — including the preview when the key is held.
    let combo = if forward { "alt+tab" } else { "alt+shift+tab" };
    win::press_keys(combo)?;
    std::thread::sleep(Duration::from_millis(220));
    match win::foreground_window() {
        Some(w) => Ok(format!("switched to \"{}\"", w.spoken_title())),
        None => Ok("switched windows".into()),
    }
}

/// Run one window state change on whatever is in front.
fn window_state(what: &str, change: win::WindowState) -> Result<String, String> {
    let w = win::foreground_window().ok_or("there's no window in front to work with")?;
    if !win::set_window_state(w.hwnd, change) {
        return Err(format!("Windows wouldn't let me change \"{}\"", w.spoken_title()));
    }
    Ok(format!("\"{}\" is {what}", w.spoken_title()))
}

fn snap_window(side: &str) -> Result<String, String> {
    let w = win::foreground_window().ok_or("there's no window in front to move")?;
    let side = normalize_side(side);
    win::snap_window(w.hwnd, side)?;
    Ok(match side {
        "left" => format!("\"{}\" is on the left half", w.spoken_title()),
        "right" => format!("\"{}\" is on the right half", w.spoken_title()),
        "top" => format!("\"{}\" is across the top", w.spoken_title()),
        "bottom" => format!("\"{}\" is across the bottom", w.spoken_title()),
        _ => format!("\"{}\" fills the screen", w.spoken_title()),
    })
}

/// "to the left" / "the right half" / "full screen" → one of the five sides.
fn normalize_side(side: &str) -> &'static str {
    let s = side.to_lowercase();
    if s.contains("left") {
        "left"
    } else if s.contains("right") {
        "right"
    } else if s.contains("top") || s.contains("upper") {
        "top"
    } else if s.contains("bottom") || s.contains("lower") {
        "bottom"
    } else {
        "full"
    }
}

fn move_to_monitor(which: &str) -> Result<String, String> {
    let w = win::foreground_window().ok_or("there's no window in front to move")?;
    let (where_, _) = win::move_window_to_monitor(w.hwnd, which)?;
    Ok(format!("\"{}\" moved to {where_}", w.spoken_title()))
}

// ── Files and notes ──────────────────────────────────────────────────────

/// The folders people name when they say "my downloads".
fn known_folders() -> Vec<(&'static str, PathBuf, &'static [&'static str])> {
    use windows::Win32::UI::Shell::{
        FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Music, FOLDERID_Pictures,
        FOLDERID_Videos,
    };
    let mut out = Vec::new();
    let mut add = |label: &'static str, id, spoken: &'static [&'static str]| {
        if let Some(p) = win::known_folder(id) {
            out.push((label, p, spoken));
        }
    };
    add("Downloads", &FOLDERID_Downloads, &["downloads", "download", "my downloads"]);
    add("Documents", &FOLDERID_Documents, &["documents", "document", "docs", "my documents"]);
    add("Desktop", &FOLDERID_Desktop, &["desktop", "my desktop"]);
    add("Pictures", &FOLDERID_Pictures, &["pictures", "picture", "photos", "my pictures"]);
    add("Music", &FOLDERID_Music, &["music", "songs", "my music"]);
    add("Videos", &FOLDERID_Videos, &["videos", "video", "movies", "my videos"]);
    out
}

/// Resolve a spoken folder: a known folder, a literal path, or nothing.
fn resolve_folder(want: &str) -> Option<(String, PathBuf)> {
    let raw = want.trim().trim_matches(['"', '\'']);
    if raw.is_empty() {
        return None;
    }
    // A path is a path, however it was said.
    if raw.contains('\\') || raw.contains(":/") || raw.starts_with('/') {
        let p = PathBuf::from(raw);
        return p.is_dir().then(|| (folder_label(&p), p));
    }
    let spoken = fuzzy::squash(&fuzzy::strip_filler(raw));
    for (label, path, names) in known_folders() {
        if names.iter().any(|n| fuzzy::squash(n) == spoken) {
            return Some((label.to_string(), path));
        }
    }
    // A folder by its own name inside the home folder: "the NeedleVoice folder".
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from);
    if let Some(home) = home {
        if let Ok(entries) = std::fs::read_dir(&home) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if entry.path().is_dir() && fuzzy::squash(&name) == spoken {
                    return Some((name, entry.path()));
                }
            }
        }
    }
    None
}

/// Just the last part of a path, for talking about it.
fn folder_label(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| path.display().to_string())
}

fn open_folder(want: &str) -> Result<String, String> {
    let want = if want.trim().is_empty() { "downloads" } else { want };
    let (label, path) = resolve_folder(want)
        .ok_or_else(|| format!("I couldn't find a folder called \"{}\"", want.trim()))?;
    win::shell_open(&path.display().to_string(), None)?;
    Ok(format!("opened {label}"))
}

/// Where a file search looks, best first. Everything else is reachable by
/// giving a path, but these are the places files actually live.
fn search_roots() -> Vec<(String, PathBuf)> {
    let mut roots: Vec<(String, PathBuf)> = known_folders()
        .into_iter()
        .map(|(label, path, _)| (label.to_string(), path))
        .collect();
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        roots.push(("your home folder".into(), PathBuf::from(profile)));
    }
    // A file can be in two of these (a shortcut on the desktop, a copy in
    // Downloads); the same answer twice is noise.
    roots.dedup_by(|a, b| a.1 == b.1);
    roots
}

/// Every match for a spoken file name, best first, as (label, path).
fn find_files(want: &str, limit: usize) -> Vec<(String, PathBuf)> {
    let want = want.trim().trim_matches(['"', '\'']);
    if want.is_empty() {
        return Vec::new();
    }
    let mut found: Vec<(String, PathBuf)> = Vec::new();
    for (label, root) in search_roots() {
        for path in win::find_named_files(&root, want, limit) {
            if !found.iter().any(|(_, p)| p == &path) {
                found.push((label.clone(), path));
            }
        }
        if found.len() >= limit {
            break;
        }
    }
    found.truncate(limit);
    found
}

fn describe_file(label: &str, path: &Path) -> String {
    format!(
        "\"{}\" in {label}",
        path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
    )
}

fn find_file(want: &str) -> Result<String, String> {
    if want.trim().is_empty() {
        return Err("what file am I looking for?".into());
    }
    let found = find_files(want, 4);
    if found.is_empty() {
        return Err(format!(
            "I couldn't find anything called \"{}\" in Downloads, Documents, Desktop, Pictures, Music or Videos",
            want.trim()
        ));
    }
    let mut said = format!("found {}", describe_file(&found[0].0, &found[0].1));
    if found.len() > 1 {
        said.push_str(&format!(", and {} more", found.len() - 1));
    }
    Ok(said)
}

fn open_file(want: &str) -> Result<String, String> {
    if want.trim().is_empty() {
        return Err("which file should I open?".into());
    }
    let found = find_files(want, 2);
    let Some((label, path)) = found.first() else {
        return Err(format!("I couldn't find a file called \"{}\"", want.trim()));
    };
    win::shell_open(&path.display().to_string(), None)?;
    Ok(format!("opened {}", describe_file(label, path)))
}

fn recycle_file(want: &str) -> Result<String, String> {
    if want.trim().is_empty() {
        return Err("which file should I delete?".into());
    }
    let found = find_files(want, 4);
    match found.len() {
        0 => Err(format!("I couldn't find a file called \"{}\"", want.trim())),
        // One match deletes; two do not. Deleting the wrong document is not a
        // mistake that can be talked away afterwards.
        1 => {
            let (label, path) = &found[0];
            win::recycle(std::slice::from_ref(path))?;
            Ok(format!("put {} in the recycle bin", describe_file(label, path)))
        }
        n => {
            let names: Vec<String> = found.iter().map(|(l, p)| describe_file(l, p)).collect();
            Err(format!("there are {n} of them — {}. Say which one, with its folder", names.join("; ")))
        }
    }
}

fn folder_size(want: &str) -> Result<String, String> {
    let want = if want.trim().is_empty() { "downloads" } else { want };
    let (label, path) = resolve_folder(want)
        .ok_or_else(|| format!("I couldn't find a folder called \"{}\"", want.trim()))?;
    let (bytes, files) = directory_size(&path);
    Ok(format!(
        "{label} holds {} in {files} files",
        human_bytes(bytes)
    ))
}

/// Total size of a folder tree, bounded so a chosen drive does not freeze the
/// assistant: 200 000 entries is far more than any personal folder.
fn directory_size(root: &Path) -> (u64, u64) {
    let mut bytes = 0u64;
    let mut files = 0u64;
    let mut seen = 0usize;
    let mut queue = vec![root.to_path_buf()];
    while let Some(dir) = queue.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            seen += 1;
            if seen > 200_000 {
                return (bytes, files);
            }
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                queue.push(entry.path());
            } else {
                bytes += meta.len();
                files += 1;
            }
        }
    }
    (bytes, files)
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["bytes", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} bytes")
    } else if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

// ── Notes ────────────────────────────────────────────────────────────────

/// "25 October, 3:04 pm" — short, because it prefixes every note.
fn note_stamp() -> String {
    let t = win::local_time();
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let month = MONTHS.get(t.month.saturating_sub(1) as usize).copied().unwrap_or("");
    let (hour, suffix) = match t.hour {
        0 => (12, "am"),
        1..=11 => (t.hour, "am"),
        12 => (12, "pm"),
        _ => (t.hour - 12, "pm"),
    };
    format!("{} {month} {}:{:02} {suffix}", t.day, hour, t.minute)
}

fn take_note(text: &str) -> Result<String, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("what should the note say?".into());
    }
    let path = notes_file();
    let mut existing = std::fs::read_to_string(&path).unwrap_or_default();
    if existing.is_empty() {
        existing.push_str("# Notes\n\nTaken by voice. One line each.\n\n");
    }
    if !existing.ends_with('\n') {
        existing.push('\n');
    }
    existing.push_str(&format!("- {} — {text}\n", note_stamp()));
    std::fs::write(&path, existing).map_err(|e| format!("I couldn't write the note file: {e}"))?;
    Ok(format!("noted: {text}"))
}

/// Every note, newest last.
fn read_note_lines() -> Vec<String> {
    std::fs::read_to_string(notes_file())
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.trim().strip_prefix("- ").map(|s| s.trim().to_string()))
        .filter(|l| !l.is_empty())
        .collect()
}

/// Drop the timestamp prefix, which is noise when it is read out loud.
fn without_stamp(note: &str) -> &str {
    match note.split_once(" — ") {
        Some((stamp, rest)) if stamp.len() <= 24 => rest,
        _ => note,
    }
}

fn read_notes() -> Result<String, String> {
    let notes = read_note_lines();
    if notes.is_empty() {
        return Err("there are no notes yet — say \"take a note\" and something".into());
    }
    let start = notes.len().saturating_sub(4);
    let said: Vec<&str> = notes[start..].iter().map(|n| without_stamp(n)).collect();
    let mut out = String::new();
    if start > 0 {
        out.push_str(&format!("your {} most recent notes: ", said.len()));
    } else {
        out.push_str(if said.len() == 1 { "one note: " } else { "your notes: " });
    }
    out.push_str(&said.join("; "));
    Ok(out)
}

fn search_notes(query: &str) -> Result<String, String> {
    let query = query.trim();
    if query.is_empty() {
        return read_notes();
    }
    let needle = fuzzy::squash(query);
    let hits: Vec<String> = read_note_lines()
        .into_iter()
        .filter(|n| fuzzy::squash(n).contains(&needle))
        .map(|n| without_stamp(&n).to_string())
        .collect();
    match hits.len() {
        0 => Err(format!("nothing in your notes mentions \"{query}\"")),
        n => {
            let shown: Vec<&str> = hits.iter().take(3).map(String::as_str).collect();
            let more = if n > shown.len() { format!(", and {} more", n - shown.len()) } else { String::new() };
            Ok(format!("{n} note{} about \"{query}\": {}{more}", if n == 1 { "" } else { "s" }, shown.join("; ")))
        }
    }
}

// ── Clipboard and typing ─────────────────────────────────────────────────

fn copy_to_clipboard(text: &str) -> Result<String, String> {
    let text = text.trim();
    if text.is_empty() {
        // "copy that" with nothing said: copy whatever is selected, which is the
        // same as pressing Ctrl+C.
        win::press_keys("ctrl+c")?;
        return Ok("copied the selection".into());
    }
    win::set_clipboard_text(text)?;
    let shown = if text.chars().count() > 40 {
        format!("{}…", text.chars().take(37).collect::<String>())
    } else {
        text.to_string()
    };
    Ok(format!("copied \"{shown}\""))
}

fn read_clipboard() -> Result<String, String> {
    let text = win::clipboard_text()?;
    let text = text.trim();
    if text.is_empty() {
        return Err("the clipboard is empty".into());
    }
    let words = text.split_whitespace().count();
    let spoken = if text.chars().count() > 300 {
        format!("{}…", text.chars().take(297).collect::<String>())
    } else {
        text.to_string()
    };
    Ok(format!("the clipboard says: {spoken} ({words} words)"))
}

fn type_text(text: &str) -> Result<String, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("what should I type?".into());
    }
    win::type_unicode(text)?;
    Ok(format!("typed {} characters", text.chars().count()))
}

// ── The system ───────────────────────────────────────────────────────────

/// "3 hours 12 minutes" — the way a duration is said.
fn spoken_duration(d: Duration) -> String {
    let secs = d.as_secs();
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3_600;
    let minutes = (secs % 3_600) / 60;
    let mut parts: Vec<String> = Vec::new();
    if days > 0 {
        parts.push(format!("{days} day{}", plural(days)));
    }
    if hours > 0 {
        parts.push(format!("{hours} hour{}", plural(hours)));
    }
    if minutes > 0 {
        parts.push(format!("{minutes} minute{}", plural(minutes)));
    }
    if parts.is_empty() {
        "less than a minute".to_string()
    } else {
        parts.join(" ")
    }
}

fn plural(n: u64) -> &'static str {
    if n == 1 { "" } else { "s" }
}

fn gigabytes(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0 * 1024.0)
}

fn battery_status() -> Result<String, String> {
    let b = win::battery().ok_or("I can't read the battery on this PC")?;
    let Some(percent) = b.percent else {
        return Ok("this PC is on mains power and has no battery".into());
    };
    let state = if b.charging {
        "charging"
    } else if b.on_ac {
        "on mains power"
    } else {
        "on battery"
    };
    match b.seconds_left {
        Some(secs) if !b.charging && secs < 24 * 3600 => Ok(format!(
            "battery at {percent}%, {state} — about {} left",
            spoken_duration(Duration::from_secs(secs as u64))
        )),
        _ => Ok(format!("battery at {percent}%, {state}")),
    }
}

fn system_status() -> Result<String, String> {
    let cpu = win::cpu_usage(Duration::from_millis(280));
    let memory = win::memory();
    let mut parts: Vec<String> = Vec::new();
    if let Some(cpu) = cpu {
        parts.push(format!("processor at {cpu:.0}%"));
    }
    if let Some((total, avail)) = memory {
        parts.push(format!("memory {:.1} of {:.1} GB in use", gigabytes(total - avail), gigabytes(total)));
    }
    let up = win::uptime();
    if up.as_secs() > 60 {
        parts.push(format!("up for {}", spoken_duration(up)));
    }
    if parts.is_empty() {
        return Err("I couldn't read the system's own numbers".into());
    }
    Ok(parts.join(", "))
}

fn top_processes(what: &str) -> Result<String, String> {
    let by_cpu = !matches!(what.trim().to_lowercase().as_str(), "memory" | "ram" | "memory usage" | "the memory");
    let apps = win::top_apps(by_cpu, 3);
    if apps.is_empty() {
        return Ok("nothing is using much of anything".into());
    }
    let said: Vec<String> = apps
        .iter()
        .map(|a| {
            if by_cpu {
                format!("{} at {:.0}%", a.name, a.cpu_percent)
            } else {
                format!("{} using {} MB", a.name, a.megabytes)
            }
        })
        .collect();
    Ok(format!(
        "the {}: {}",
        if by_cpu { "busiest apps" } else { "hungriest apps" },
        said.join(", ")
    ))
}

fn disk_space() -> Result<String, String> {
    let drives = win::drives();
    if drives.is_empty() {
        return Err("I couldn't read any drives".into());
    }
    let said: Vec<String> = drives
        .iter()
        .map(|d| format!("{} has {} free of {}", d.name, human_bytes(d.free), human_bytes(d.total)))
        .collect();
    Ok(said.join("; "))
}

/// The address this machine uses to reach the internet, without sending a thing
/// (a UDP socket's `connect` only fills in the route).
fn local_ip() -> Option<std::net::IpAddr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("1.1.1.1:80").ok()?;
    socket.local_addr().ok().map(|a| a.ip())
}

fn online() -> bool {
    use std::net::ToSocketAddrs;
    let Ok(mut addrs) = "1.1.1.1:53".to_socket_addrs() else { return false };
    let Some(addr) = addrs.next() else { return false };
    std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(700)).is_ok()
}

/// The name of the wireless network, from `netsh`, or "" when on a cable.
fn wifi_name() -> String {
    let Ok(text) = win::run_capture("netsh", &["wlan", "show", "interfaces"], Duration::from_secs(4)) else {
        return String::new();
    };
    for line in text.lines() {
        let trimmed = line.trim_start();
        // "SSID  : name" — but never "BSSID".
        if let Some(rest) = trimmed.strip_prefix("SSID") {
            if let Some((_, value)) = rest.split_once(':') {
                let value = value.trim();
                if !value.is_empty() {
                    return value.to_string();
                }
            }
        }
    }
    String::new()
}

fn network_status() -> Result<String, String> {
    let up = online();
    let ip = local_ip().map(|ip| ip.to_string());
    let wifi = if up { wifi_name() } else { String::new() };
    let mut said = if up { "connected".to_string() } else { "not connected to the internet".to_string() };
    if let Some(ip) = ip {
        said.push_str(&format!(", this PC is {ip}"));
    }
    if !wifi.is_empty() {
        said.push_str(&format!(" on Wi-Fi \"{wifi}\""));
    }
    Ok(said)
}

fn machine_info() -> Result<String, String> {
    let cpu = win::cpu_name();
    let cores = win::logical_processors();
    let mut said: Vec<String> = Vec::new();
    if !cpu.is_empty() {
        said.push(format!("{cpu}, {cores} threads"));
    } else {
        said.push(format!("{cores} threads"));
    }
    if let Some((total, _)) = win::memory() {
        said.push(format!("{:.1} GB of memory", gigabytes(total)));
    }
    let os = win::windows_version();
    if !os.is_empty() {
        said.push(os);
    }
    let name = win::computer_name();
    if !name.is_empty() {
        said.push(format!("this is {name}"));
    }
    Ok(said.join(", "))
}

fn is_app_running(want: &str, cfg: &crate::Config, apps: &AppIndex) -> Result<String, String> {
    let want = want.trim();
    if want.is_empty() {
        return Err("which app?".into());
    }
    let mut exes: Vec<String> = Vec::new();
    if let Some((app, _)) = apps.find(want, cfg) {
        if let Some(exe) = app.exe.clone() {
            exes.push(exe);
        }
    }
    let spoken = fuzzy::squash(&fuzzy::strip_filler(want));
    let running = win::processes();
    let hit = running.iter().find(|p| {
        exes.iter().any(|e| e.eq_ignore_ascii_case(&p.exe)) || {
            let stem = p.exe.split('.').next().unwrap_or(&p.exe);
            let stem = fuzzy::squash(stem);
            !stem.is_empty() && (stem == spoken || fuzzy::similarity(&stem, &spoken) >= 0.85)
        }
    });
    match hit {
        Some(p) => {
            let name = p.exe.split('.').next().unwrap_or(&p.exe).to_string();
            Ok(format!("{name} is running"))
        }
        None => Ok(format!("{want} isn't running")),
    }
}

/// Brightness is the one setting with no plain Win32 call: it lives behind WMI,
/// and only on screens that expose it (laptop panels, mostly). The answers are
/// explicit so an unsupported display says so instead of failing silently.
fn brightness_script(change: &str) -> String {
    let mut s = String::from("$ErrorActionPreference = 'Stop'\n");
    s.push_str("try {\n");
    s.push_str("  $setters = @(Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightnessMethods)\n");
    s.push_str("  if ($setters.Count -eq 0) { Write-Output 'NO_CONTROL'; return }\n");
    s.push_str("  $readers = @(Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightness)\n");
    s.push_str("  $now = if ($readers.Count -gt 0) { [int]$readers[0].CurrentBrightness } else { 50 }\n");
    match change {
        "up" => s.push_str("  $target = [Math]::Min(100, $now + 20)\n"),
        "down" => s.push_str("  $target = [Math]::Max(0, $now - 20)\n"),
        percent => {
            s.push_str("  $target = ");
            s.push_str(percent);
            s.push('\n');
        }
    }
    s.push_str("  foreach ($x in $setters) { [void]$x.WmiSetBrightness(1, $target) }\n");
    s.push_str("  Write-Output ('OK ' + $target)\n");
    s.push_str("} catch { Write-Output ('ERR ' + $_.Exception.Message) }\n");
    s
}

fn apply_brightness(change: &str) -> Result<u8, String> {
    let script = brightness_script(change);
    let out = win::run_capture(
        "powershell.exe",
        &["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &script],
        Duration::from_secs(15),
    )?;
    let line = out.lines().last().unwrap_or("").trim();
    if let Some(value) = line.strip_prefix("OK ") {
        return value.trim().parse::<u8>().map_err(|_| "the screen didn't say what it did".to_string());
    }
    if line.contains("NO_CONTROL") {
        return Err("this screen doesn't let Windows change its brightness".into());
    }
    Err(format!(
        "the screen wouldn't change brightness{}",
        line.strip_prefix("ERR ").map(|e| format!(": {e}")).unwrap_or_default()
    ))
}

fn brightness_set(percent: &str) -> Result<String, String> {
    let digits: String = percent.chars().filter(|c| c.is_ascii_digit()).collect();
    let value = if digits.is_empty() {
        // "dim the screen" with no number: half is the obvious reading.
        50
    } else {
        digits.parse::<u32>().unwrap_or(50).min(100) as u8
    };
    let now = apply_brightness(&value.to_string())?;
    Ok(format!("brightness at {now}%"))
}

fn brightness_nudge(up: bool) -> Result<String, String> {
    let now = apply_brightness(if up { "up" } else { "down" })?;
    Ok(format!("brightness at {now}%"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_op_has_a_tool_name_that_round_trips() {
        for op in Op::ALL {
            let name = op.tool();
            assert!(!name.is_empty(), "{op:?} has no name");
            assert_eq!(Op::from_tool(name), Some(op), "{name} does not map back");
            // The catalogue is the public face of this list; a tool here that is
            // not in it could never be called.
            assert!(crate::tools::builtin(name).is_some(), "{name} is not in the catalogue");
        }
        assert_eq!(Op::from_tool("not_a_tool"), None);
    }

    #[test]
    fn sizes_and_durations_read_like_speech() {
        assert_eq!(human_bytes(512), "512 bytes");
        assert_eq!(human_bytes(2048), "2.0 KB");
        assert_eq!(human_bytes(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(human_bytes(3 * 1024 * 1024 * 1024), "3.0 GB");
        assert_eq!(spoken_duration(Duration::from_secs(45)), "less than a minute");
        assert_eq!(spoken_duration(Duration::from_secs(3600)), "1 hour");
        assert_eq!(spoken_duration(Duration::from_secs(3 * 3600 + 12 * 60)), "3 hours 12 minutes");
        assert_eq!(spoken_duration(Duration::from_secs(86_400 + 120)), "1 day 2 minutes");
    }

    /// The sides people say, in the words they use for them.
    #[test]
    fn sides_are_understood() {
        assert_eq!(normalize_side("left"), "left");
        assert_eq!(normalize_side("to the left"), "left");
        assert_eq!(normalize_side("the right half"), "right");
        assert_eq!(normalize_side("top"), "top");
        assert_eq!(normalize_side("upper"), "top");
        assert_eq!(normalize_side("bottom"), "bottom");
        assert_eq!(normalize_side("full screen"), "full");
        assert_eq!(normalize_side(""), "full");
    }

    /// The note file is a list of lines, and the timestamp is not part of what
    /// gets read back out.
    #[test]
    fn notes_round_trip() {
        let text = "# Notes\n\n- 25 Oct 3:04 pm — buy milk\n- 25 Oct 3:05 pm — call mum\n";
        let lines: Vec<&str> = text
            .lines()
            .filter_map(|l| l.trim().strip_prefix("- "))
            .collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(without_stamp(lines[0]), "buy milk");
        assert_eq!(without_stamp("no timestamp here"), "no timestamp here");
    }

    /// A script that rewrites the brightness has to keep its braces and its
    /// target in one piece — this is the kind of string that quietly turns into
    /// PowerShell injection or a syntax error.
    #[test]
    fn the_brightness_script_is_well_formed() {
        let set = brightness_script("45");
        assert!(set.contains("$target = 45"), "{set}");
        assert!(set.contains("WmiSetBrightness(1, $target)"), "{set}");
        assert_eq!(set.matches('{').count(), set.matches('}').count(), "{set}");
        let up = brightness_script("up");
        assert!(up.contains("[Math]::Min(100, $now + 20)"), "{up}");
        let down = brightness_script("down");
        assert!(down.contains("[Math]::Max(0, $now - 20)"), "{down}");
    }

    /// Finding a file by its spoken name: the stem has to match with or without
    /// spaces, plural or singular, and must not match a different word.
    #[test]
    fn file_names_match_the_way_people_say_them() {
        assert!(win::fuzzy_name_match("payslipjuly", "payslip"));
        assert!(win::fuzzy_name_match("payslip", "payslip"));
        assert!(win::fuzzy_name_match("taxreturns", "taxreturn"));
        assert!(!win::fuzzy_name_match("invoice", "payslip"));
        assert!(!win::fuzzy_name_match("a", "payslip"));
    }
}

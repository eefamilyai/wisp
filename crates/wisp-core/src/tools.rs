//! The functions Needle can call: built-ins plus the user's own functions.
//!
//! Everything the assistant can *do* is described here, once. The same table
//! feeds three things: the tool list handed to Needle, the exact spoken
//! phrases that skip the model entirely, and the settings app's function list.
//! Adding a function therefore means adding one row.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::config::Config;

pub struct Builtin {
    pub name: &'static str,
    /// Heading in the settings app.
    pub group: &'static str,
    pub description: &'static str,
    /// (parameter, hint)
    pub params: &'static [(&'static str, &'static str)],
    /// Shown as an example command in the settings app.
    pub example: &'static str,
    /// Exact phrases that run this without asking Needle. `{name}` slots
    /// capture the rest of what was said.
    pub phrases: &'static [&'static str],
}

const fn b(
    name: &'static str,
    group: &'static str,
    description: &'static str,
    params: &'static [(&'static str, &'static str)],
    example: &'static str,
    phrases: &'static [&'static str],
) -> Builtin {
    Builtin { name, group, description, params, example, phrases }
}

/// The headings the settings app groups functions under, in the order it shows
/// them. Every [`Builtin`] names one of these.
pub const GROUPS: [&str; 10] = [
    "Apps & web",
    "Windows",
    "Files & notes",
    "Clipboard & typing",
    "Music & sound",
    "System",
    "Power & screen",
    "Answers",
    "Schedule",
    "Assistant",
];

/// Descriptions are kept short: every token is processed on every command.
///
/// The phrases are what make this list affordable. Anything with a good phrase
/// never reaches the model at all, and the ones that do are shortlisted by
/// [`tools_json_for`] — which is the only reason a catalogue this size can run
/// on a 121M-parameter model.
pub const BUILTINS: &[Builtin] = &[
    // ── Apps & web ─────────────────────────────────────────────────────
    b("open_app", "Apps & web", "Open an installed app", &[("app", "")], "open chrome",
      &["open {app}", "launch {app}", "start {app}", "open up {app}", "fire up {app}", "bring up {app}"]),
    b("close_app", "Apps & web", "Close a running app", &[("app", "")], "close spotify",
      &["close {app}", "quit {app}", "exit {app}", "kill {app}"]),
    b("web_search", "Apps & web", "Search the web for a question or topic", &[("query", "")], "what is the haber process",
      &["search the web for {query}", "search google for {query}", "search for {query}", "google {query}", "look up {query}"]),
    b("open_website", "Apps & web", "Open a website URL", &[("url", "")], "go to reddit.com",
      &["go to {url}", "visit {url}", "navigate to {url}", "open website {url}"]),
    b("youtube_search", "Apps & web", "Search YouTube videos or music", &[("query", "")], "play lofi on youtube",
      &["play {query} on youtube", "youtube {query}", "search youtube for {query}", "play {query} on yt"]),

    // ── Windows ────────────────────────────────────────────────────────
    b("focus_window", "Windows", "Switch to a window that is already open",
      &[("app", "chrome, code, the report")], "switch to chrome",
      &["switch to {app}", "switch to the {app} window", "focus {app}", "focus on {app}", "show me {app}",
        "bring {app} to the front", "go to the {app} window", "jump to {app}", "back to {app}",
        "bring the {app} window forward"]),
    b("active_window", "Windows", "Say which window is in front", &[], "what am I looking at",
      &["what am i looking at", "what window is this", "what is this window", "which window is this",
        "what is in front", "whats in front", "what am i working on", "what window am i in", "what is this i am looking at", "which app is in front"]),
    b("list_windows", "Windows", "List the windows that are open", &[], "what windows are open",
      &["what windows are open", "list my windows", "list the windows", "which windows are open",
        "what is open", "whats open", "show my open windows", "what have i got open"]),
    b("next_window", "Windows", "Switch to the next window", &[], "next window",
      &["next window", "switch windows", "switch to the next window", "go to the next window", "next app"]),
    b("previous_window", "Windows", "Switch back to the last window", &[], "go back a window",
      &["previous window", "last window", "switch back", "go back a window", "go to the previous window",
        "back one window"]),
    b("minimize_window", "Windows", "Minimise the window in front", &[], "minimize this window",
      &["minimize this", "minimize it", "minimize the window", "minimize this window", "minimise this",
        "minimise it", "minimise the window", "hide this window", "hide the window"]),
    b("maximize_window", "Windows", "Make the window in front fill the screen", &[], "maximize this window",
      &["maximize this", "maximize it", "maximize the window", "maximise this", "maximise it",
        "maximise the window", "make this full screen", "make it full screen", "maximize this window"]),
    b("restore_window", "Windows", "Put the window in front back to its normal size", &[], "restore this window",
      &["restore this window", "restore the window", "unmaximize", "unmaximise", "make it smaller",
        "make the window smaller", "undo maximize", "bring it back to normal"]),
    b("snap_window", "Windows", "Put the window in front on one side of the screen",
      &[("side", "left, right, top, bottom or full")], "snap this to the left",
      &["snap this to the {side}", "snap it to the {side}", "snap the window to the {side}",
        "put this window on the {side}", "put it on the {side} half", "put this on the {side} half",
        "move this window to the {side}", "put this window in the {side} half", "snap it {side}", "snap this {side}", "snap the window {side}", "put it {side}", "put this window {side}", "move it {side}", "put this {side}", "put this one {side}", "put this window onto the {side}"]),
    b("move_to_monitor", "Windows", "Move the window in front to another screen",
      &[("which", "?other, next, previous, left, right, primary"), ], "move this to the other screen",
      &["move this to the {which} monitor", "move this to my {which} screen", "move this to the {which} screen",
        "put this window on the {which} monitor", "move this window to the {which} monitor",
        "move this to the other monitor", "move this to my other screen", "send this to the other screen"]),

    // ── Files & notes ──────────────────────────────────────────────────
    b("open_folder", "Files & notes", "Open one of your folders in Explorer",
      &[("folder", "downloads, documents, desktop, pictures or a path")], "open my downloads",
      &["open my {folder}", "open the {folder} folder", "show me my {folder}", "open the folder {folder}",
        "open my {folder} folder", "open the {folder} directory", "go to my {folder} folder"]),
    b("find_file", "Files & notes", "Find a file by name in your personal folders",
      &[("name", "payslip, tax return")], "find the file budget",
      &["find the file {name}", "find my file {name}", "find the file called {name}", "find a file called {name}",
        "where is the file {name}", "where did i put {name}", "search for the file {name}",
        "find {name} on my computer", "find my {name} file", "locate the file {name}", "where did i save {name}"]),
    b("open_file", "Files & notes", "Find a file by name and open it",
      &[("name", "payslip, tax return")], "open the file payslip",
      &["open the file {name}", "open my {name} file", "open the {name} document", "open the document called {name}"]),
    b("recycle_file", "Files & notes", "Put a file in the recycle bin (recoverable)",
      &[("name", "old draft")], "delete the file old draft",
      &["delete the file {name}", "put {name} in the recycle bin", "put the file {name} in the recycle bin",
        "throw away {name}", "throw out {name}", "trash {name}", "bin {name}", "delete {name}", "send {name} to the recycle bin", "put the file {name} in the bin"]),
    b("empty_recycle_bin", "Files & notes", "Empty the recycle bin", &[], "empty the recycle bin",
      &["empty the recycle bin", "empty my recycle bin", "empty the trash", "clear the recycle bin",
        "empty recycle bin"]),
    b("folder_size", "Files & notes", "Say how much space a folder is using",
      &[("folder", "downloads, documents or a path")], "how big is my downloads folder",
      &["how big is my {folder} folder", "how big is {folder}", "how much space is {folder} using",
        "how much space does {folder} take", "whats the size of my {folder} folder"]),
    b("take_note", "Files & notes", "Write something down in your notes file",
      &[("text", "buy milk")], "take a note buy milk",
      &["take a note {text}", "take a note that {text}", "note that {text}", "make a note that {text}",
        "make a note {text}", "jot down {text}", "remember that {text}", "add a note {text}",
        "add a note that {text}"]),
    b("read_notes", "Files & notes", "Read back your most recent notes", &[], "read my notes",
      &["read my notes", "read my note", "what are my notes", "show my notes", "what did i note",
        "read me my notes", "my notes"]),
    b("search_notes", "Files & notes", "Search your notes for a word",
      &[("query", "the plumber")], "what did I note about the plumber",
      &["what did i note about {query}", "search my notes for {query}", "look in my notes for {query}",
        "find in my notes {query}", "did i note anything about {query}", "what are my notes about {query}"]),

    // ── Clipboard & typing ─────────────────────────────────────────────
    b("copy_to_clipboard", "Clipboard & typing", "Copy what you say, or the selection, to the clipboard",
      &[("text", "?the words to copy; empty copies the selection")], "copy this to the clipboard",
      &["copy {text}", "copy this {text}", "copy this to the clipboard", "put {text} on the clipboard",
        "copy that", "copy it", "copy the selection", "copy this to my clipboard", "put this on my clipboard"]),
    b("read_clipboard", "Clipboard & typing", "Read out what is on the clipboard", &[], "what's on my clipboard",
      &["what is on my clipboard", "whats on my clipboard", "read my clipboard", "read the clipboard",
        "what is on the clipboard", "read out the clipboard", "what is in my clipboard", "what have i got on the clipboard", "what did i copy"]),
    b("paste_clipboard", "Clipboard & typing", "Paste the clipboard into the window in front", &[], "paste it",
      &["paste", "paste it", "paste that", "paste it here", "paste this in", "paste the clipboard"]),
    b("type_text", "Clipboard & typing", "Type the words out into the window in front",
      &[("text", "the words to type")], "type out hello there",
      &["type {text}", "type out {text}", "type this {text}", "write {text}", "write out {text}",
        "type the words {text}"]),

    // ── Music & sound ──────────────────────────────────────────────────
    b("media_pause", "Music & sound", "Pause or stop the music or video playing", &[], "pause the music",
      &["pause", "pause it", "pause the music", "pause the video", "pause playback", "stop the music", "stop the video",
        "stop media", "stop playback", "stop the song", "stop playing music", "hold on"]),
    b("media_resume", "Music & sound", "Resume the music or video that was paused", &[], "resume the music",
      &["resume", "resume it", "resume the music", "resume the video", "resume playback", "continue the music",
        "continue playing", "continue playback", "play the music", "resume media", "unpause", "play it again"]),
    b("media_next", "Music & sound", "Skip to the next song or track", &[], "next song",
      &["next", "next song", "next track", "skip", "skip song", "skip track", "skip this song", "play the next song"]),
    b("media_previous", "Music & sound", "Go back to the previous song or track", &[], "previous track",
      &["previous", "previous song", "previous track", "last song", "go back a song", "play the previous song"]),
    b("media_stop", "Music & sound", "Stop the media completely", &[], "stop the music",
      &["stop", "stop media playback", "stop the player"]),
    b("volume_set", "Music & sound", "Set the volume to a percentage", &[("percent", "0-100")], "set volume to 30",
      &["set volume to {percent}", "volume to {percent}", "set the volume to {percent}", "volume {percent} percent",
        "turn the volume to {percent}", "set volume {percent}"]),
    b("volume_up", "Music & sound", "Turn the volume up", &[], "turn it up",
      &["turn it up", "volume up", "turn up the volume", "louder", "increase the volume", "raise the volume"]),
    b("volume_down", "Music & sound", "Turn the volume down", &[], "turn it down",
      &["turn it down", "volume down", "turn down the volume", "quieter", "lower the volume", "decrease the volume"]),
    b("mute", "Music & sound", "Mute the sound", &[], "mute",
      &["mute", "mute the sound", "mute the volume", "mute it", "silence"]),
    b("unmute", "Music & sound", "Unmute the sound", &[], "unmute",
      &["unmute", "unmute the sound", "unmute the volume", "unmute it", "turn the sound back on"]),

    // ── System ─────────────────────────────────────────────────────────
    b("lock_pc", "System", "Lock the computer", &[], "lock my pc",
      &["lock my pc", "lock the pc", "lock the computer", "lock my computer", "lock the screen", "lock it"]),
    b("show_desktop", "System", "Minimize all windows and show the desktop", &[], "show desktop",
      &["show desktop", "show the desktop", "minimize everything", "minimize all windows", "hide everything"]),
    b("screenshot", "System", "Take a screenshot and save it", &[], "take a screenshot",
      &["take a screenshot", "screenshot", "capture the screen", "take a screen shot", "screen shot"]),
    b("copy_screenshot", "System", "Copy a screenshot to the clipboard, ready to paste", &[], "copy a screenshot",
      &["copy a screenshot", "copy the screen", "screenshot to my clipboard", "copy a screenshot to the clipboard",
        "copy the screen to the clipboard", "screenshot to clipboard"]),
    b("close_window", "System", "Close the window that is in front", &[], "close this window",
      &["close this window", "close the window", "close the current window", "close this tab"]),
    b("open_settings", "System", "Open Windows Settings", &[], "open windows settings",
      &["open settings", "open windows settings", "open the settings app", "open system settings"]),
    b("battery_status", "System", "Say how much battery is left", &[], "how much battery is left",
      &["battery", "battery status", "how much battery", "how much battery is left", "how much charge is left",
        "am i charging", "how long will the battery last", "is it plugged in", "what is my battery at", "how is the battery", "battery left", "what is the battery level", "how much charge do i have", "whats my battery", "what is my battery", "battery level", "how much juice is left", "how much juice do i have", "am i on battery", "is my battery low", "battery percentage", "how full is my battery"]),
    b("system_status", "System", "Say how busy the processor and memory are", &[], "how is my computer doing",
      &["how is my computer doing", "how busy is my computer", "cpu usage", "how is my cpu",
        "how much ram am i using", "memory usage", "system status", "how loaded is my pc",
        "is my computer busy", "how much ram is in use", "is my memory full", "how hard is my cpu working", "how much memory is free", "how loaded is the cpu", "what is my cpu at", "how much ram is being used", "is my pc slow", "how fast is my pc running", "how much memory am i using"]),
    b("top_processes", "System", "Say which apps are using the most processor or memory",
      &[("what", "?cpu or memory")], "what's using my cpu",
      &["what is using my cpu", "whats using my cpu", "what is using my memory", "whats using my memory",
        "what is eating my cpu", "what is eating my memory", "what is slowing down my computer",
        "what is using all the memory", "top processes", "what is using the most memory", "what is using the most cpu", "which app is using my cpu", "what is eating my ram", "what is hogging the cpu", "what is using my ram", "what is taking up my memory", "what is running hot"]),
    b("disk_space", "System", "Say how much space is left on your drives", &[], "how much space is left",
      &["how much space is left", "how much disk space do i have", "how full is my drive",
        "disk space", "how much space do i have", "how much free space is there", "how much disk space do i have left", "how much space is left on my drive", "how much free space do i have", "how much space do i have left", "is my drive full", "how much room is left", "how much room do i have", "storage space", "how full is my disk", "how much storage is left"]),
    b("network_status", "System", "Say whether you are online, and the address of this PC", &[], "am I online",
      &["am i online", "am i connected", "what is my ip", "whats my ip", "what is my ip address",
        "network status", "what wifi am i on", "which wifi am i on", "am i connected to the internet", "do i have internet", "is the internet working", "what network am i on", "what is my ip address", "do i have wifi", "am i on wifi", "what is my wifi", "am i connected to wifi"]),
    b("machine_info", "System", "Say what this computer is: processor, memory and Windows version", &[],
      "what are my specs",
      &["what are my specs", "what computer is this", "how many cores do i have", "what is my cpu",
        "what cpu do i have", "what windows version is this", "how much memory does this pc have",
        "system specs", "tell me about this computer", "what is this computer", "how much ram does this pc have", "what is my computer", "what are the specs", "what are this pcs specs", "how much ram is in this pc"]),
    b("is_app_running", "System", "Say whether an app is running", &[("app", "")], "is spotify running",
      &["is {app} running", "is {app} open", "is {app} still running", "is {app} up", "did {app} start"]),

    // ── Power & screen ─────────────────────────────────────────────────
    b("sleep_pc", "Power & screen", "Put the computer to sleep", &[], "put the computer to sleep",
      &["go to sleep", "sleep the computer", "put the computer to sleep", "put my pc to sleep",
        "put the pc to sleep", "sleep the pc", "put it to sleep"]),
    b("shutdown_pc", "Power & screen", "Shut the computer down, with a 30 second cancel window", &[],
      "shut down the computer",
      &["shut down the computer", "shut down my pc", "shut down the pc", "shut down", "power off",
        "turn off the computer", "turn off my pc", "switch off the computer", "shut the computer down"]),
    b("restart_pc", "Power & screen", "Restart the computer, with a 30 second cancel window", &[],
      "restart the computer",
      &["restart the computer", "restart my pc", "restart the pc", "reboot", "reboot the computer", "restart"]),
    b("sign_out", "Power & screen", "Sign out of Windows", &[], "sign out",
      &["sign out", "sign out of windows", "log me out", "log out", "sign me out"]),
    b("cancel_shutdown", "Power & screen", "Cancel a pending shutdown or restart", &[], "cancel the shutdown",
      &["cancel the shutdown", "cancel shutdown", "abort the shutdown", "cancel the restart",
        "stop the shutdown", "cancel the shutdown timer"]),
    b("turn_off_screen", "Power & screen", "Turn the display off without locking", &[], "turn off the screen",
      &["turn off the screen", "turn off my screen", "turn the screen off", "screen off", "switch off the screen",
        "turn off the display"]),
    b("brightness_set", "Power & screen", "Set the screen brightness",
      &[("percent", "0-100")], "set the brightness to 40",
      &["set the brightness to {percent}", "set brightness to {percent}", "brightness {percent}",
        "screen brightness {percent}", "brightness to {percent}", "set the screen brightness to {percent}"]),
    b("brightness_up", "Power & screen", "Turn the screen brightness up", &[], "brighten the screen",
      &["brighten the screen", "turn the brightness up", "make the screen brighter", "brightness up",
        "turn up the brightness", "make the screen brighter please"]),
    b("brightness_down", "Power & screen", "Turn the screen brightness down", &[], "dim the screen",
      &["dim the screen", "dim my screen", "turn the brightness down", "make the screen dimmer",
        "brightness down", "turn down the brightness", "dim it"]),

    // ── Answers ────────────────────────────────────────────────────────
    b("tell_time", "Answers", "Tell the current time", &[], "what time is it",
      &["what time is it", "what is the time", "whats the time", "tell me the time", "current time", "the time"]),
    b("tell_date", "Answers", "Tell today's date", &[], "what's the date",
      &["what is the date", "whats the date", "what is today's date", "what day is it", "what is today", "todays date"]),

    // ── Schedule ───────────────────────────────────────────────────────
    b("set_alarm", "Schedule", "Set an alarm or timer. \"when\" is spoken, e.g. 7:30 am, in 20 minutes, tomorrow at 9",
      &[("when", "7:30 am"), ("label", "?what it is for")], "set an alarm for 7:30 am",
      &["set an alarm for {when}", "set an alarm at {when}", "set alarm for {when}", "wake me up at {when}",
        "wake me up in {when}", "set a timer for {when}", "start a timer for {when}", "set a timer {when}",
        "alarm in {when}", "wake me at {when}"]),
    b("set_reminder", "Schedule", "Remind me about something, at a spoken time when one is given",
      &[("text", "call mum"), ("when", "?4 pm, or empty")], "remind me to call mum at 4 pm",
      &["remind me to {text} at {when}", "remind me to {text} in {when}", "remind me to {text}",
        "remind me about {text} at {when}", "remind me about {text}", "remind me in {when} to {text}",
        "remind me at {when} to {text}", "set a reminder to {text} at {when}", "set a reminder for {text} at {when}",
        // People drop the "me": "reminder to call mum at four".
        "reminder to {text} at {when}", "reminder to {text}", "reminder for {text} at {when}",
        "reminder {text} at {when}", "add a reminder to {text} at {when}", "add a reminder for {text} at {when}",
        "new reminder to {text} at {when}", "make a reminder to {text} at {when}"]),
    b("add_todo", "Schedule", "Add something to the to-do list", &[("text", "buy milk")], "add buy milk to my list",
      &["add {text} to my list", "add {text} to my to do list", "add a todo {text}", "add to do {text}",
        "put {text} on my list", "put {text} on my to do list", "remember to {text}", "add {text} to the list"]),
    b("add_event", "Schedule", "Put an appointment on the calendar",
      &[("text", "dentist"), ("when", "3 pm")], "put the dentist on my calendar at 3 pm",
      &["add {text} to my calendar at {when}", "put {text} on my calendar at {when}",
        "put {text} in my calendar at {when}", "add {text} to my calendar", "schedule {text} at {when}",
        "add an event {text} at {when}"]),
    b("clear_schedule", "Schedule", "Delete all alarms, reminders, to-dos or calendar events at once",
      &[("what", "?alarms, reminders, to dos, events, or everything")], "delete all my alarms",
      &["delete all {what}", "delete all my {what}", "delete all the {what}", "delete all of my {what}",
        "delete every {what}", "delete all {what} for me", "clear all {what}", "clear all my {what}",
        "clear my {what}", "clear the {what}", "remove all {what}", "remove all my {what}",
        "cancel all {what}", "cancel all my {what}", "delete my {what}", "delete the {what}",
        "clear my schedule", "delete my schedule", "clear the schedule", "clear everything",
        "delete everything", "delete all of it", "clear all of it", "cancel everything",
        "clear my alarms and reminders", "delete all my alarms and reminders"]),
    b("show_schedule", "Schedule", "Read out alarms, reminders, to-dos or today's agenda",
      &[("what", "?alarms, reminders, todos or today")], "what's on my schedule",
      &["what is on my schedule", "whats on my schedule", "what is on my calendar", "whats on my calendar",
        "read my schedule", "read my list", "read my to do list", "read my todos", "what are my alarms",
        "what is on today", "whats on today", "what is next", "whats next", "my to do list", "show my schedule",
        "show my list", "what do i have today", "what is on my list", "whats on my list", "my alarms",
        "my reminders", "my calendar", "show my alarms", "list my alarms"]),
    b("cancel_schedule", "Schedule", "Cancel a alarm, reminder, event or to-do",
      &[("what", "?the dentist, or 7 am")], "cancel my alarm",
      &["cancel my alarm", "cancel the alarm", "cancel my alarms", "delete my alarm", "cancel the {what} alarm",
        "cancel the {what} reminder", "cancel my {what} reminder", "cancel the {what} timer", "cancel my {what} timer",
        "delete the {what} reminder", "remove {what} from my list", "delete {what} from my list",
        "remove {what} from my to do list", "cancel my {what} event", "cancel the {what} meeting"]),
    b("complete_todo", "Schedule", "Tick something off the to-do list", &[("what", "buy milk")], "mark buy milk as done",
      &["mark {what} as done", "mark {what} done", "i finished {what}", "i have finished {what}", "i did {what}",
        "tick off {what}", "cross {what} off my list", "complete {what}", "done with {what}"]),

    // ── Assistant ──────────────────────────────────────────────────────
    b("disengage", "Assistant", "Stop listening and forget what was just said", &[], "never mind",
      &["never mind", "nevermind", "never mind that", "never mind it", "never mind then", "forget it",
        "forget that", "forget about it", "cancel", "cancel that", "cancel it", "scrap that", "disregard",
        "disregard that", "ignore that", "ignore it", "i changed my mind", "changed my mind", "no thanks",
        "no thank you", "thats all", "thats it", "thats everything", "were done", "we are done", "abort",
        "abort that", "as you were", "stand down", "disengage", "stop listening", "stop listening to me",
        "you can stop", "you can stop now", "turn off", "turn yourself off", "go back to sleep",
        "thats enough", "nothing else", "all done", "wait", "wait a second", "wait a moment",
        "wait wait", "hold on", "hang on", "hold that thought", "give me a second", "give me a moment",
        "one moment", "nvm", "never mind me"]),
];

pub fn builtin(name: &str) -> Option<&'static Builtin> {
    BUILTINS.iter().find(|t| t.name == name)
}

pub fn is_builtin(name: &str) -> bool {
    builtin(name).is_some()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CustomKind {
    /// Run a program (or open a file / folder / protocol link).
    Run,
    /// Open a URL in the browser. Parameters are URL-encoded.
    OpenUrl,
    /// Press a key combination, e.g. `ctrl+shift+esc`, `win+d`.
    Keys,
    /// Run a hidden PowerShell script.
    #[serde(rename = "powershell", alias = "power_shell")]
    PowerShell,
    /// Run a Python script. `target` is the file, `args` its arguments;
    /// declared parameters are passed as arguments and as WISP_PARAM_* variables.
    Python,
    /// Do several things in order: the no-code way to build a function.
    Sequence,
    /// Only say the reply (e.g. jokes, reminders).
    SayOnly,
}

impl CustomKind {
    pub const ALL: [CustomKind; 7] = [
        CustomKind::Run,
        CustomKind::OpenUrl,
        CustomKind::Keys,
        CustomKind::PowerShell,
        CustomKind::Python,
        CustomKind::Sequence,
        CustomKind::SayOnly,
    ];

    pub fn label(self) -> &'static str {
        match self {
            CustomKind::Run => "Run a program / open a file or folder",
            CustomKind::OpenUrl => "Open a URL",
            CustomKind::Keys => "Press keys",
            CustomKind::PowerShell => "Run a PowerShell script",
            CustomKind::Python => "Run a Python script",
            CustomKind::Sequence => "Do several things in order",
            CustomKind::SayOnly => "Just say the reply",
        }
    }

    /// One-line explanation for the settings app.
    pub fn hint(self) -> &'static str {
        match self {
            CustomKind::Run => "The target can be an .exe, a folder, a document or a shell: path.",
            CustomKind::OpenUrl => "Opened in the browser you chose on the Browser tab.",
            CustomKind::Keys => "Sent to whatever window is in front.",
            CustomKind::PowerShell => "Runs invisibly in the background. Keep it short — there's no window to show errors in.",
            CustomKind::Python => "The script's output is spoken back, so it can answer questions. Parameters arrive as arguments and as WISP_PARAM_* environment variables.",
            CustomKind::Sequence => "Steps run one after another. A step that produces output can be used by later steps as {output}.",
            CustomKind::SayOnly => "Nothing happens except the reply, e.g. a joke or a reminder.",
        }
    }

    pub fn target_hint(self) -> &'static str {
        match self {
            CustomKind::Run => r"C:\path\to\program.exe   or   C:\Users\me\Documents",
            CustomKind::OpenUrl => "https://open.spotify.com/search/{query}",
            CustomKind::Keys => "ctrl+shift+esc",
            CustomKind::PowerShell => "Stop-Computer -Force",
            CustomKind::Python => "weather.py",
            CustomKind::Sequence => "",
            CustomKind::SayOnly => "",
        }
    }

    /// True when the function's own printed output is the answer worth saying.
    pub fn has_output(self) -> bool {
        matches!(self, CustomKind::Python | CustomKind::PowerShell)
    }

    /// Which of the two string fields the editor should show.
    pub fn uses_target(self) -> bool {
        !matches!(self, CustomKind::Sequence | CustomKind::SayOnly)
    }
}

/// One step of a [`CustomKind::Sequence`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Step {
    pub kind: StepKind,
    /// What to say, where to go, which program, which script, which function…
    pub target: String,
    /// Arguments for programs, PowerShell and Python.
    pub args: String,
}

impl Default for Step {
    fn default() -> Self {
        Step { kind: StepKind::Say, target: String::new(), args: String::new() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    Say,
    Run,
    OpenUrl,
    Keys,
    #[serde(rename = "powershell", alias = "power_shell")]
    PowerShell,
    Python,
    /// Wait this many milliseconds.
    Wait,
    /// Run another of your functions.
    Call,
    /// Turn an alarm or reminder on: "remind me in 10 minutes to stretch".
    Schedule,
}

impl StepKind {
    pub const ALL: [StepKind; 9] = [
        StepKind::Say,
        StepKind::Run,
        StepKind::OpenUrl,
        StepKind::Keys,
        StepKind::PowerShell,
        StepKind::Python,
        StepKind::Wait,
        StepKind::Call,
        StepKind::Schedule,
    ];

    pub fn label(self) -> &'static str {
        match self {
            StepKind::Say => "Say",
            StepKind::Run => "Run a program",
            StepKind::OpenUrl => "Open a URL",
            StepKind::Keys => "Press keys",
            StepKind::PowerShell => "PowerShell",
            StepKind::Python => "Python",
            StepKind::Wait => "Wait (ms)",
            StepKind::Call => "Run another function",
            StepKind::Schedule => "Add to schedule",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            StepKind::Say => "What to say, e.g. \"Right, focus time.\"",
            StepKind::Run => r"Program, folder or document, e.g. C:\Windows\notepad.exe",
            StepKind::OpenUrl => "https://… — opened in your browser",
            StepKind::Keys => "ctrl+shift+esc",
            StepKind::PowerShell => "Script text. Its output becomes {output}.",
            StepKind::Python => "Script file, e.g. weather.py — output becomes {output}",
            StepKind::Wait => "Milliseconds to pause, e.g. 1500",
            StepKind::Call => "The name of one of your other functions",
            StepKind::Schedule => "Text for the reminder, e.g. take a break",
        }
    }

    /// Steps whose output is worth keeping for `{output}`.
    pub fn has_output(self) -> bool {
        matches!(self, StepKind::PowerShell | StepKind::Python | StepKind::Run)
    }

    /// Steps that need a file or script path rather than free text.
    pub fn needs_args(self) -> bool {
        matches!(self, StepKind::Run | StepKind::PowerShell | StepKind::Python)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CustomTool {
    pub enabled: bool,
    /// snake_case name Needle calls, e.g. `spotify_search`.
    pub name: String,
    /// What it does, in plain words — this is what Needle reads.
    pub description: String,
    /// Parameter names, e.g. ["query"]. Use them as {query} below.
    pub params: Vec<String>,
    pub kind: CustomKind,
    /// Program path, URL, key combo or script (with {param} placeholders).
    pub target: String,
    /// Arguments for `Run` (with {param} placeholders).
    pub args: String,
    /// Exact phrases that run this instantly, skipping Needle.
    /// `{param}` captures words, e.g. "search spotify for {query}".
    pub phrases: Vec<String>,
    /// What to say afterwards (with {param} placeholders). Empty = a
    /// personality-style "done" line.
    pub reply: String,
    /// The steps of a [`CustomKind::Sequence`], run in order.
    pub steps: Vec<Step>,
}

impl Default for CustomTool {
    fn default() -> Self {
        Self {
            enabled: true,
            name: "my_function".into(),
            description: "Describe what this does".into(),
            params: Vec::new(),
            kind: CustomKind::Run,
            target: String::new(),
            args: String::new(),
            phrases: Vec::new(),
            reply: String::new(),
            steps: Vec::new(),
        }
    }
}

/// Ready-made functions offered in the settings app's "Add from a template" list.
pub fn templates() -> Vec<CustomTool> {
    let t = |name: &str, desc: &str, params: &[&str], kind, target: &str, args: &str, phrases: &[&str], reply: &str| CustomTool {
        enabled: true,
        name: name.into(),
        description: desc.into(),
        params: params.iter().map(|s| s.to_string()).collect(),
        kind,
        target: target.into(),
        args: args.into(),
        phrases: phrases.iter().map(|s| s.to_string()).collect(),
        reply: reply.into(),
        steps: Vec::new(),
    };
    let mut tools = vec![
        // Shutdown, sleep, the recycle bin and opening your own folders are
        // built-in now, so the templates are the things a built-in cannot do:
        // protocol links, key chords, PowerShell one-liners and sequences.
        t("spotify_search", "Search Spotify for a song, artist or playlist", &["query"], CustomKind::Run, "spotify:search:{query}", "", &["search spotify for {query}", "play {query} on spotify"], "Searching Spotify for {query}."),
        t("maps_directions", "Get directions to a place", &["place"], CustomKind::OpenUrl, "https://www.google.com/maps/dir/?api=1&destination={place}", "", &["directions to {place}", "navigate to {place}"], "Getting directions to {place}."),
        t("task_manager", "Open Task Manager", &[], CustomKind::Keys, "ctrl+shift+esc", "", &[], ""),
        t("task_view", "Show all open windows", &[], CustomKind::Keys, "win+tab", "", &["show all windows", "show task view"], ""),
        t("night_mode", "Turn night light on or off", &[], CustomKind::PowerShell, "$p='HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\CloudStore\\Store\\DefaultAccount\\Current\\default$windows.data.bluelightreduction.bluelightreductionstate\\windows.data.bluelightreduction.bluelightreductionstate'; $d=(Get-ItemProperty -Path $p).Data; if($d[18] -eq 1){$d[18]=0}else{$d[18]=1}; Set-ItemProperty -Path $p -Name Data -Value $d", "", &["turn on night light", "toggle night light", "turn off night light"], ""),
        // A sequence: several steps, no code.
        t("focus_mode", "Close the browser, open my notes and start a timer", &[], CustomKind::Sequence, "", "", &["start focus mode", "focus mode"], "Focus mode on. Distractions closed."),
        CustomTool {
            kind: CustomKind::Sequence,
            ..t("work_setup", "Open everything I need for work", &[], CustomKind::Sequence, "", "", &["set up my work", "work setup"], "Work setup is ready.")
        },
        // Python: the script prints the answer and it gets spoken.
        t("python_example", "Ask a Python script something and speak its answer", &["question"], CustomKind::Python, "ask.py", "{question}", &["ask python {question}"], ""),
        // A sequence that opens everything for the morning and says so.
        t("morning_routine", "Open my calendar and the weather, then say good morning", &[], CustomKind::Sequence, "", "", &["good morning", "start my morning"], "Good morning! Calendar and weather are up."),
    ];
    // Sequences come with their steps ready to run.
    for tool in &mut tools {
        if tool.kind == CustomKind::Sequence && tool.steps.is_empty() {
            tool.steps = template_steps(&tool.name);
        }
    }
    tools
}

/// A sensible starting point for each of the built-in templates, with the
/// steps filled in where a sequence is more useful than an empty shell.
pub fn template_steps(name: &str) -> Vec<Step> {
    let step = |kind, target: &str, args: &str| Step { kind, target: target.into(), args: args.into() };
    match name {
        "focus_mode" => vec![
            step(StepKind::Keys, "ctrl+w", ""),
            step(StepKind::Run, "notepad.exe", ""),
            step(StepKind::Wait, "800", ""),
            step(StepKind::Say, "Timer started — 25 minutes.", ""),
            step(StepKind::Schedule, "focus session over", "in 25 minutes"),
        ],
        "morning_routine" => vec![
            step(StepKind::OpenUrl, "https://calendar.google.com", ""),
            step(StepKind::Wait, "1200", ""),
            step(StepKind::OpenUrl, "https://www.google.com/search?q=weather", ""),
            step(StepKind::Say, "Good morning! Your calendar and the weather are up.", ""),
        ],
        "work_setup" => vec![
            step(StepKind::OpenUrl, "https://mail.google.com", ""),
            step(StepKind::Wait, "1200", ""),
            step(StepKind::Run, "code", ""),
            step(StepKind::Say, "Mail and your editor are up.", ""),
        ],
        _ => Vec::new(),
    }
}

/// Fill `{param}` slots from the captured parameters. `encode` percent-encodes
/// the values, which is what URLs want and plain text does not.
///
/// One pass, with the longest parameter name winning at each position: once
/// `{query}` has been filled in, the text that replaced it is not scanned
/// again, so a value containing braces — or a function with both `q` and
/// `query` — cannot be mangled by a second substitution. `{{ query }}`, a
/// capitalised `{QUERY}` and inner padding are all accepted.
pub fn substitute(text: &str, params: &[(String, String)], encode: bool) -> String {
    if params.is_empty() || !text.contains('{') {
        return text.to_string();
    }
    let mut order: Vec<usize> = (0..params.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(params[i].0.len()));
    let mut out = String::with_capacity(text.len() + 32);
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'{' {
            let next = text[i..].find('{').map(|at| i + at).unwrap_or(text.len());
            out.push_str(&text[i..next]);
            i = next;
            continue;
        }
        // `{{ name }}` or `{name}`.
        let open = if bytes.get(i + 1) == Some(&b'{') { i + 2 } else { i + 1 };
        let Some(close) = text[open..].find('}').map(|at| open + at) else {
            out.push_str(&text[i..]);
            break;
        };
        let key = text[open..close].trim();
        let doubled = bytes.get(close + 1) == Some(&b'}');
        let end = if doubled { close + 2 } else { close + 1 };
        // Longest name first, so "query" is preferred over "q" at this position.
        let hit = order
            .iter()
            .map(|&p| (params[p].0.as_str(), params[p].1.as_str()))
            .find(|(name, _)| name.eq_ignore_ascii_case(key));
        match hit {
            Some((_, value)) => {
                if encode {
                    out.push_str(&crate::fuzzy::url_encode(value));
                } else {
                    out.push_str(value);
                }
            }
            None => out.push_str(&text[i..end]),
        }
        i = end;
    }
    out
}

/// Dotted function names read better in speech: `spotify_search` → "spotify search".
impl CustomTool {
    pub fn label(&self) -> String {
        self.name.replace('_', " ")
    }
}

/// Make a valid function name from free text: "Spotify Search!" → "spotify_search".
pub fn sanitize_name(s: &str) -> String {
    let mut out = String::new();
    for c in s.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    out.trim_end_matches('_').to_string()
}

/// Clean up the saved function list: valid, unique names that don't shadow a
/// built-in; valid parameter names; no stray whitespace.
pub fn sanitize_tools(tools: Vec<CustomTool>, _disabled: &[String]) -> Vec<CustomTool> {
    let mut tools = tools;
    for t in &mut tools {
        sanitize_tool(t);
    }
    dedupe_tools(tools)
}

/// The per-function half of [`sanitize_tools`]: a usable name, parameter names,
/// and trimmed text. Nothing here rejects the function — [`dedupe_tools`] does
/// that — so it is safe to call on one entry of a list, which is what the
/// settings app does for the row being edited.
pub fn sanitize_tool(t: &mut CustomTool) {
    t.name = sanitize_name(&t.name);
    t.description = t.description.trim().to_string();
    if t.description.is_empty() && !t.name.is_empty() {
        t.description = t.name.replace('_', " ");
    }
    let mut params: Vec<String> = Vec::new();
    for p in &t.params {
        let p = sanitize_name(p);
        if !p.is_empty() && !params.contains(&p) {
            params.push(p);
        }
    }
    t.params = params;
    t.phrases = t
        .phrases
        .iter()
        .map(|p| p.trim().to_lowercase())
        .filter(|p| !p.is_empty())
        .collect();
    // A phrase written twice is two chances to match nothing new.
    let mut seen: Vec<String> = Vec::with_capacity(t.phrases.len());
    t.phrases.retain(|p| {
        if seen.iter().any(|s| s == p) {
            return false;
        }
        seen.push(p.clone());
        true
    });
    t.target = t.target.trim().to_string();
    t.args = t.args.trim().to_string();
    t.reply = t.reply.trim().to_string();
    t.steps.retain(|s| !(s.target.trim().is_empty() && s.kind != StepKind::Wait));
    for s in &mut t.steps {
        s.target = s.target.trim().to_string();
        s.args = s.args.trim().to_string();
    }
}

/// Drop the functions that cannot be called: no usable name, one that shadows a
/// built-in, or a duplicate of one already kept.
pub fn dedupe_tools(tools: Vec<CustomTool>) -> Vec<CustomTool> {
    let mut out: Vec<CustomTool> = Vec::with_capacity(tools.len());
    for t in tools {
        if t.name.is_empty() || is_builtin(&t.name) {
            log::warn!("ignoring function with unusable name {:?}", t.name);
            continue;
        }
        if out.iter().any(|o| o.name == t.name) {
            log::warn!("ignoring duplicate function {:?}", t.name);
            continue;
        }
        if t.kind == CustomKind::Sequence && t.steps.is_empty() {
            log::warn!("function {:?} has no steps", t.name);
        }
        out.push(t);
    }
    out
}

pub fn tool_json(name: &str, description: &str, params: &[(String, String)]) -> Value {
    let mut props = serde_json::Map::new();
    for (p, d) in params {
        let mut v = json!({"type": "string"});
        let hint = d.strip_prefix('?').unwrap_or(d);
        if !hint.is_empty() {
            v["description"] = json!(hint);
        }
        props.insert(p.clone(), v);
    }
    // A hint starting with "?" marks a parameter the model may leave out.
    let required: Vec<&String> = params.iter().filter(|(_, d)| !d.starts_with('?')).map(|(p, _)| p).collect();
    json!({
        "name": name,
        "description": description,
        "parameters": {"type": "object", "properties": props, "required": required}
    })
}

/// Built-ins that are switched on, most-used first so the prompt reads naturally.
pub fn enabled_builtins(cfg: &Config) -> impl Iterator<Item = &'static Builtin> + '_ {
    BUILTINS.iter().filter(move |t| !cfg.disabled_tools.iter().any(|d| d == t.name))
}

/// The tool list handed to Needle for this config.
pub fn tools_json(cfg: &Config) -> String {
    let mut tools: Vec<Value> = enabled_builtins(cfg)
        .map(|t| {
            let params: Vec<(String, String)> = t.params.iter().map(|(p, d)| (p.to_string(), d.to_string())).collect();
            tool_json(t.name, t.description, &params)
        })
        .collect();
    for c in cfg.custom_tools.iter().filter(|c| c.enabled && !c.name.is_empty()) {
        let params: Vec<(String, String)> = c.params.iter().map(|p| (p.clone(), String::new())).collect();
        tools.push(tool_json(&c.name, &c.description, &params));
    }
    Value::Array(tools).to_string()
}

/// How many tool definitions a confident first pass carries.
///
/// Measured on this machine: Needle's prompt costs about 13 ms per position, and
/// a tool definition is 40–60 positions, so every function in the list is worth
/// roughly half a second. Seven keeps a confident command under four seconds
/// while still leaving the model a choice.
const SHORTLIST: usize = 7;
/// How many a pass carries when the command did not clearly name anything.
///
/// Bounded on purpose. Needle's prompt is the tool list plus the command, and a
/// truncated prompt is a model that answers nonsense: the whole catalogue is
/// now well past what fits, so nothing may ever send all of it.
const WIDENED: usize = 14;
/// How many the retry after a failed shortlist carries.
const WIDEST: usize = 18;

/// Functions worth having in front of the model whatever was said — the ones
/// people reach for constantly.
const ALWAYS: [&str; 10] = [
    "open_app",
    "close_app",
    "web_search",
    "disengage",
    "tell_time",
    "media_pause",
    "volume_set",
    "lock_pc",
    "screenshot",
    "focus_window",
];

/// The command's words, lowercased, ignoring the short ones that match anything.
fn command_words(command: &str) -> Vec<String> {
    command
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 2)
        .map(|w| w.to_lowercase())
        .collect()
}

/// One function's relevance to a command.
///
/// A phrase hit is the strongest signal there is, because the phrases are
/// written to be what people say. Failing that, the words of the command are
/// compared against the words of the function's own phrases — "what's eating my
/// memory" says nothing that is in `top_processes`'s description, but it says
/// three words that are in its phrases.
fn score_builtin(t: &Builtin, words: &[String], command: &str) -> i32 {
    let mut score = 0i32;
    if t.phrases.iter().any(|p| match_phrase(p, command).is_some()) {
        score += 1000;
    }
    let name = t.name.to_lowercase();
    let name_parts: Vec<&str> = name.split('_').collect();
    for w in words {
        if name_parts.iter().any(|part| part.starts_with(w.as_str()) || w.starts_with(part) && part.len() > 3) {
            score += 60;
        } else if name.contains(w.as_str()) {
            score += 30;
        }
        if t.description.to_lowercase().contains(w.as_str()) {
            score += 8;
        }
        // Any phrase of this function containing the word: 12 points, capped so
        // a function with forty phrases cannot win on volume alone.
        let in_phrases = t
            .phrases
            .iter()
            .take(8)
            .filter(|p| p.split(|c: char| !c.is_alphanumeric()).any(|pw| pw == w.as_str()))
            .count();
        score += (in_phrases.min(3) as i32) * 12;
    }
    score
}

fn score_custom(c: &CustomTool, words: &[String]) -> i32 {
    let mut score = 0i32;
    let name = c.name.to_lowercase();
    let description = c.description.to_lowercase();
    for w in words {
        if name.contains(w.as_str()) {
            score += 60;
        }
        if description.contains(w.as_str()) {
            score += 20;
        }
        if c.phrases.iter().any(|p| p.split(|ch: char| !ch.is_alphanumeric()).any(|pw| pw == w.as_str())) {
            score += 24;
        }
    }
    score
}

/// Every enabled function, scored and sorted best first.
fn scored_tools(cfg: &Config, command: &str) -> Vec<(i32, Value)> {
    let words = command_words(command);
    let mut scored: Vec<(i32, Value)> = Vec::new();
    for t in enabled_builtins(cfg) {
        let score = score_builtin(t, &words, command);
        if score > 0 {
            let params: Vec<(String, String)> = t.params.iter().map(|(p, d)| (p.to_string(), d.to_string())).collect();
            scored.push((score, tool_json(t.name, t.description, &params)));
        }
    }
    for c in cfg.custom_tools.iter().filter(|c| c.enabled && !c.name.is_empty()) {
        let score = score_custom(c, &words);
        if score > 0 {
            let params: Vec<(String, String)> = c.params.iter().map(|p| (p.clone(), String::new())).collect();
            scored.push((score, tool_json(&c.name, &c.description, &params)));
        }
    }
    // Sorted by score; ties keep catalogue order, so the result is stable.
    scored.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    scored
}

/// Add a function by name, if it is enabled and not already there.
fn push_named(cfg: &Config, out: &mut Vec<Value>, name: &str) {
    if out.iter().any(|v| v["name"] == name) {
        return;
    }
    if let Some(t) = enabled_builtins(cfg).find(|t| t.name == name) {
        let params: Vec<(String, String)> = t.params.iter().map(|(p, d)| (p.to_string(), d.to_string())).collect();
        out.push(tool_json(t.name, t.description, &params));
    }
}

/// The tool list handed to Needle, cut down to what could plausibly matter for
/// this command. The second value says whether that was a real shortlist (a
/// function clearly matched) rather than the wide fallback.
///
/// Every command used to be preceded by the same 5 KB of tool definitions, and
/// the model reads prompts at about 15 ms per token on this class of CPU: 1,149
/// positions of tool description cost 17 of the 18.4 seconds a command took,
/// while decoding the answer cost 1.4. So the definitions are the latency, and
/// they are mostly irrelevant to any one command.
///
/// The catalogue has since grown to seventy functions, which is several times
/// what Needle's prompt can hold: sending all of it would truncate the prompt
/// and produce nothing usable. So there are exactly two sizes — a confident
/// shortlist, or a wide one that also carries the everyday functions — and
/// [`crate::brain::Brain::ask_needle`] widens once more before giving up.
pub fn tools_json_for(cfg: &Config, command: &str) -> (String, bool) {
    let scored = scored_tools(cfg, command);
    let decisive = scored.first().is_some_and(|(score, _)| *score >= 40);
    let mut chosen: Vec<Value> =
        scored.into_iter().take(if decisive { SHORTLIST } else { WIDENED }).map(|(_, v)| v).collect();
    // Nothing scored at all: hand over the everyday functions rather than an
    // empty list, so an oddly phrased command still has somewhere to land.
    if chosen.is_empty() {
        for name in ALWAYS {
            push_named(cfg, &mut chosen, name);
        }
    }
    // A question needs a way to be answered. Without this, a general question
    // whose words happen to touch a function gets answered with that function:
    // "what is the haber process" was answered with a processor query.
    if looks_like_a_question(command) {
        push_named(cfg, &mut chosen, "web_search");
    }
    // The model's own way of saying "not that": without it, a command it cannot
    // place becomes a guessed tool call instead of a question.
    push_named(cfg, &mut chosen, "disengage");
    (Value::Array(chosen).to_string(), decisive)
}

/// The list to try when a shortlist led nowhere: the widest that still fits.
pub fn tools_json_widened(cfg: &Config, command: &str) -> String {
    let scored = scored_tools(cfg, command);
    let mut chosen: Vec<Value> = scored.into_iter().take(WIDEST).map(|(_, v)| v).collect();
    for name in ALWAYS {
        push_named(cfg, &mut chosen, name);
    }
    if looks_like_a_question(command) {
        push_named(cfg, &mut chosen, "web_search");
    }
    push_named(cfg, &mut chosen, "disengage");
    Value::Array(chosen).to_string()
}

/// Is this phrased as a question? Cheap and deliberately narrow: it only has to
/// decide whether the search tool travels with the prompt.
fn looks_like_a_question(command: &str) -> bool {
    const ASKS: [&str; 8] = ["what", "who", "whos", "when", "where", "why", "how", "which"];
    let first = command
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    ASKS.contains(&first.as_str())
}

/// Match a spoken command against a phrase pattern with `{param}` slots.
/// Returns the captured values when it matches.
pub fn match_phrase(pattern: &str, text: &str) -> Option<Vec<(String, String)>> {
    let pat: Vec<String> = pattern.split_whitespace().map(|w| w.to_lowercase()).collect();
    let words: Vec<String> = crate::fuzzy::normalize(text).split(' ').filter(|w| !w.is_empty()).map(String::from).collect();
    fn go(pat: &[String], words: &[String], caps: &mut Vec<(String, String)>) -> bool {
        match pat.first() {
            None => words.is_empty(),
            Some(p) if p.starts_with('{') && p.ends_with('}') => {
                let name = p.trim_matches(|c| c == '{' || c == '}').to_string();
                // Capture 1..n words, shortest first, so later literals can match.
                for n in 1..=words.len() {
                    caps.push((name.clone(), words[..n].join(" ")));
                    if go(&pat[1..], &words[n..], caps) {
                        return true;
                    }
                    caps.pop();
                }
                false
            }
            Some(p) => {
                let lit = crate::fuzzy::normalize(p);
                // A pattern word may normalise to several words ("it's" → "its").
                let lit_words: Vec<&str> = lit.split(' ').filter(|w| !w.is_empty()).collect();
                if lit_words.is_empty() {
                    return go(&pat[1..], words, caps);
                }
                words.len() >= lit_words.len()
                    && words[..lit_words.len()].iter().zip(&lit_words).all(|(a, b)| a == b)
                    && go(&pat[1..], &words[lit_words.len()..], caps)
            }
        }
    }
    let mut caps = Vec::new();
    go(&pat, &words, &mut caps).then_some(caps)
}

/// One phrase that runs a function without the model.
pub struct PhraseHit {
    /// Function name (built-in or the user's).
    pub tool: String,
    pub captures: Vec<(String, String)>,
    /// How specific the matched phrase was — used to break ties.
    specificity: usize,
}

/// Match a whole spoken command against every known phrase. The most literal
/// (least guessed) phrase wins, so "stop the music" beats a bare "stop {query}".
/// `keep` filters the candidates by function name.
pub fn match_phrases(cfg: &Config, text: &str, keep: &dyn Fn(&str) -> bool) -> Option<PhraseHit> {
    let mut best: Option<PhraseHit> = None;
    let mut consider = |tool: &str, pattern: &str| {
        if !keep(tool) {
            return;
        }
        // Word for word first; a whole phrase with no blanks is also compared by
        // how alike it sounds, because speech recognition mangles words
        // ("disengage" came back as "this engage"). Long phrases only: "stop" and
        // "step" are one edit apart and mean different things.
        let (captures, exact) = match match_phrase(pattern, text) {
            Some(caps) => (caps, true),
            None if !pattern.contains('{') && crate::fuzzy::squash(pattern).chars().count() >= 8 => {
                let spoken = crate::fuzzy::squash(text);
                let want = crate::fuzzy::squash(pattern);
                if crate::fuzzy::distance(&spoken, &want) > 2 || crate::fuzzy::similarity(&spoken, &want) < 0.78 {
                    return;
                }
                (Vec::new(), false)
            }
            None => return,
        };
        // Exact wording always outranks a fuzzy hit.
        let specificity = if exact { pattern.split_whitespace().filter(|w| !w.starts_with('{')).count() } else { 0 };
        if best.as_ref().is_none_or(|b| specificity > b.specificity) {
            best = Some(PhraseHit { tool: tool.to_string(), captures, specificity });
        }
    };
    // User phrases first: on a tie theirs is the one they meant.
    for c in cfg.custom_tools.iter().filter(|c| c.enabled && !c.name.is_empty()) {
        for p in &c.phrases {
            consider(&c.name, p);
        }
    }
    for t in enabled_builtins(cfg) {
        for p in t.phrases {
            consider(t.name, p);
        }
    }
    best
}

/// [`match_phrases`] over the whole catalogue.
pub fn match_any_phrase(cfg: &Config, text: &str) -> Option<PhraseHit> {
    match_phrases(cfg, text, &|_| true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phrases() {
        assert_eq!(
            match_phrase("search spotify for {query}", "Search Spotify for daft punk."),
            Some(vec![("query".into(), "daft punk".into())])
        );
        assert_eq!(
            match_phrase("play {query} on spotify", "play lofi beats on spotify"),
            Some(vec![("query".into(), "lofi beats".into())])
        );
        assert_eq!(match_phrase("shut down the computer", "shut down the computer"), Some(vec![]));
        assert_eq!(match_phrase("shut down the computer", "shut down the computer now"), None);
    }

    #[test]
    fn a_misheard_phrase_still_lands() {
        let cfg = Config::default();
        // Whisper heard the user say this and the assistant searched the web.
        let hit = match_any_phrase(&cfg, "This engage.").expect("should still be a disengage");
        assert_eq!(hit.tool, "disengage");
        // And the exact wording keeps working.
        assert_eq!(match_any_phrase(&cfg, "never mind").unwrap().tool, "disengage");
        assert_eq!(match_any_phrase(&cfg, "Turn off.").unwrap().tool, "disengage");
        // Short words are never guessed at: "step" must not become "stop".
        assert!(match_any_phrase(&cfg, "step").is_none());
        assert!(match_any_phrase(&cfg, "cancer").is_none());
        // Fuzzy never outranks what was actually said.
        let pause = match_any_phrase(&cfg, "stop the music").unwrap();
        assert_eq!(pause.tool, "media_pause");
    }

    #[test]
    fn deleting_all_of_something_is_understood() {
        let cfg = Config::default();
        for said in [
            "delete all my alarms",
            "delete all alarms",
            "clear my reminders",
            "cancel all my reminders",
            "remove all my to dos",
            "delete everything",
            "clear my schedule",
        ] {
            let hit = match_any_phrase(&cfg, said).unwrap_or_else(|| panic!("no match for {said:?}"));
            assert_eq!(hit.tool, "clear_schedule", "{said:?} went to {}", hit.tool);
        }
        // ...and it says what it is clearing.
        let hit = match_any_phrase(&cfg, "delete all my alarms").unwrap();
        assert_eq!(hit.captures, vec![("what".to_string(), "alarms".to_string())]);
    }

    #[test]
    fn optional_parameters_are_not_required() {
        let cfg = Config::default();
        let tools: Value = serde_json::from_str(&tools_json(&cfg)).unwrap();
        let alarm = tools.as_array().unwrap().iter().find(|t| t["name"] == "set_alarm").unwrap();
        assert_eq!(alarm["parameters"]["required"], json!(["when"]));
    }

    #[test]
    fn json_is_valid() {
        let mut cfg = Config::default();
        cfg.custom_tools = templates();
        let v: Value = serde_json::from_str(&tools_json(&cfg)).unwrap();
        assert_eq!(v.as_array().unwrap().len(), BUILTINS.len() + cfg.custom_tools.len());
        assert_eq!(sanitize_name("Spotify Search!"), "spotify_search");
    }

    #[test]
    fn disabled_builtins_leave_the_prompt() {
        let mut cfg = Config::default();
        cfg.disabled_tools = vec!["media_pause".into()];
        let v: Value = serde_json::from_str(&tools_json(&cfg)).unwrap();
        assert_eq!(v.as_array().unwrap().len(), BUILTINS.len() - 1);
        assert!(match_any_phrase(&cfg, "pause the music").is_none());
    }

    /// Disengaging must not steal the commands that share its words.
    #[test]
    fn disengaging_does_not_steal_real_commands() {
        let cfg = Config::default();
        for command in ["stop the music", "cancel my alarm", "pause the music", "stop media playback"] {
            let hit = match_any_phrase(&cfg, command).expect(command);
            assert_ne!(hit.tool, "disengage", "{command} should not disengage");
        }
        for command in ["never mind", "never mind that", "turn off", "disengage", "you can stop now"] {
            let hit = match_any_phrase(&cfg, command).unwrap_or_else(|| panic!("{command} matched nothing"));
            assert_eq!(hit.tool, "disengage", "{command}");
        }
    }

    #[test]
    fn builtin_phrases_win_on_specificity() {
        let cfg = Config::default();
        let h = match_any_phrase(&cfg, "stop the music").unwrap();
        assert_eq!(h.tool, "media_pause");
        let h = match_any_phrase(&cfg, "set the volume to 30").unwrap();
        assert_eq!(h.tool, "volume_set");
        assert_eq!(h.captures, vec![("percent".into(), "30".into())]);
        assert_eq!(match_any_phrase(&cfg, "what time is it").unwrap().tool, "tell_time");
        assert_eq!(match_any_phrase(&cfg, "pause").unwrap().tool, "media_pause");
    }

    #[test]
    fn user_phrases_win() {
        let mut cfg = Config::default();
        cfg.custom_tools = sanitize_tools(templates(), &[]);
        let h = match_any_phrase(&cfg, "search spotify for daft punk").unwrap();
        assert_eq!(h.tool, "spotify_search");
        assert_eq!(h.captures, vec![("query".into(), "daft punk".into())]);
    }

    /// A hand-written config uses the natural spelling of every kind, so the
    /// names serde writes had better be the natural ones.
    #[test]
    fn config_spellings_are_natural() {
        let mut cfg = Config::default();
        cfg.custom_tools = templates();
        cfg.custom_tools.push(CustomTool {
            name: "coded".into(),
            kind: CustomKind::Python,
            target: "x.py".into(),
            ..Default::default()
        });
        cfg.custom_tools.push(CustomTool {
            name: "chained".into(),
            kind: CustomKind::Sequence,
            steps: StepKind::ALL.iter().map(|k| Step { kind: *k, target: "x".into(), args: String::new() }).collect(),
            ..Default::default()
        });
        let text = toml::to_string_pretty(&cfg).unwrap();
        for wanted in [
            "kind = \"powershell\"",
            "kind = \"python\"",
            "kind = \"sequence\"",
            "kind = \"say\"",
            "kind = \"open_url\"",
            "kind = \"wait\"",
            "kind = \"call\"",
            "kind = \"schedule\"",
        ] {
            assert!(text.contains(wanted), "expected {wanted} in:\n{text}");
        }
        let back: Config = toml::from_str(&text).unwrap();
        let chained = back.custom_tools.iter().find(|t| t.name == "chained").unwrap();
        assert_eq!(chained.steps.len(), StepKind::ALL.len());
    }

    #[test]
    fn sanitizing_functions() {
        let t = CustomTool { name: "My Thing!".into(), params: vec!["Query".into(), "query".into()], ..Default::default() };
        let builtin_clash = CustomTool { name: "open_app".into(), ..Default::default() };
        let out = sanitize_tools(vec![t, builtin_clash], &[]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "my_thing");
        assert_eq!(out[0].params, vec!["query"]);
    }

    /// Placeholders are filled in one pass, so a value is never substituted
    /// into, and a parameter whose name is a prefix of another one cannot eat
    /// part of it.
    #[test]
    fn placeholders_are_filled_exactly_once() {
        let p = |k: &str, v: &str| (k.to_string(), v.to_string());
        // The URL form: + is a space in a query string, so the filled value is
        // encoded while the template's own wording is left as written.
        assert_eq!(substitute("search {query}", &[p("query", "daft punk")], true), "search daft+punk");
        assert_eq!(substitute("https://x/?q={query}", &[p("query", "a b")], true), "https://x/?q=a+b");
        assert_eq!(substitute("search {query}", &[p("query", "daft punk")], false), "search daft punk");
        // `{q}` must not clip the "q" out of `{query}`.
        assert_eq!(
            substitute("{query} then {q}", &[p("q", "x"), p("query", "daft punk")], false),
            "daft punk then x"
        );
        // A value that itself contains braces is not re-substituted.
        assert_eq!(substitute("{a}", &[p("a", "{b}"), p("b", "no")], false), "{b}");
        // Spelling variants people actually type.
        assert_eq!(substitute("{{ query }}", &[p("query", "jazz")], false), "jazz");
        assert_eq!(substitute("{QUERY}", &[p("query", "jazz")], false), "jazz");
        assert_eq!(substitute("{ query }", &[p("query", "jazz")], false), "jazz");
        // An unknown placeholder is left alone rather than dropped.
        assert_eq!(substitute("{nope}", &[p("query", "jazz")], false), "{nope}");
        // No parameters at all is a no-op, and no brace is required.
        assert_eq!(substitute("plain text", &[], false), "plain text");
        assert_eq!(substitute("{a}", &[], false), "{a}");
    }

    #[test]
    fn every_builtin_has_an_example_and_a_home() {
        for t in BUILTINS {
            assert!(!t.example.is_empty(), "{} has no example", t.name);
            assert!(GROUPS.contains(&t.group), "{} has an unknown group {}", t.name, t.group);
            for (p, _) in t.params {
                assert!(t.example.contains(&format!("{{{p}}}")) || !t.phrases.is_empty(), "{}: unusable example", t.name);
            }
        }
    }
}

#[cfg(test)]
mod shortlist_tests {
    use super::*;
    use crate::Config;

    fn names(json: &str) -> Vec<String> {
        serde_json::from_str::<Value>(json)
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t.get("name").and_then(|n| n.as_str()).map(String::from))
            .collect()
    }

    #[test]
    fn a_shortlist_keeps_the_tool_the_command_needs() {
        let cfg = Config::default();
        let (json, short) = tools_json_for(&cfg, "close all the file explorers");
        assert!(short, "an app command should shortlist");
        assert!(names(&json).iter().any(|n| n == "close_app" || n == "open_app"), "{:?}", names(&json));
        assert!(json.len() * 3 < tools_json(&cfg).len(), "the shortlist should be far smaller");
    }

    /// The catalogue is far larger than Needle's prompt. Nothing may ever send
    /// all of it — a truncated prompt is worse than a short list, because the
    /// model answers from half a definition.
    #[test]
    fn no_command_ever_gets_the_whole_catalogue() {
        let cfg = Config::default();
        let full = tools_json(&cfg);
        for command in ["qwertyuiop zxcvbnm", "what is the haber process", "switch to chrome", ""] {
            let (json, _) = tools_json_for(&cfg, command);
            assert!(json.len() * 2 < full.len(), "{command:?} got {} of {} bytes", json.len(), full.len());
            assert!(names(&json).len() <= WIDENED + ALWAYS.len() + 1, "{command:?} got too many");
            let wide = tools_json_widened(&cfg, command);
            assert!(wide.len() * 2 < full.len(), "{command:?} widened to {} bytes", wide.len());
        }
    }

    /// A command that names nothing still gets the everyday functions, so an
    /// odd phrasing has something to land on.
    #[test]
    fn an_unrecognisable_command_gets_the_everyday_tools() {
        let cfg = Config::default();
        let (json, short) = tools_json_for(&cfg, "qwertyuiop zxcvbnm");
        assert!(!short, "nothing matched, so this is the wide fallback");
        let names = names(&json);
        for expected in ["open_app", "web_search", "disengage"] {
            assert!(names.iter().any(|n| n == expected), "{expected} missing from {names:?}");
        }
    }

    /// What each new function is for shows through in the shortlist: a question
    /// about memory has to reach `top_processes`, and a window command has to
    /// reach `focus_window`, even though neither name is in the sentence.
    #[test]
    fn the_shortlist_understands_what_was_asked() {
        let cfg = Config::default();
        let cases: [(&str, &str); 9] = [
            ("what's eating my memory", "top_processes"),
            ("switch to visual studio", "focus_window"),
            ("how much disk space do I have", "disk_space"),
            ("put this on the left half", "snap_window"),
            ("what did I note about the plumber", "search_notes"),
            ("is steam running", "is_app_running"),
            ("what's my battery", "battery_status"),
            ("how much juice is left", "battery_status"),
            ("what's eating my ram", "top_processes"),
        ];
        for (command, wanted) in cases {
            let (json, _) = tools_json_for(&cfg, command);
            assert!(
                names(&json).iter().any(|n| n == wanted),
                "{command:?} did not shortlist {wanted}: {:?}",
                names(&json)
            );
        }
    }

    /// The model's escape hatch travels with every prompt, short or wide.
    #[test]
    fn disengage_is_always_offered() {
        let cfg = Config::default();
        for command in ["qwertyuiop", "open chrome", "what is the haber process"] {
            let (json, _) = tools_json_for(&cfg, command);
            assert!(names(&json).iter().any(|n| n == "disengage"), "{command:?}");
            assert!(names(&tools_json_widened(&cfg, command)).iter().any(|n| n == "disengage"));
        }
    }
}

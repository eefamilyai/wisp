# What Wisp can do to your computer

Wisp has eighty functions. Forty-nine of them touch the machine itself: windows,
files, the clipboard, the power state. This page is the list, what each one actually
does, and the rules that keep it safe.

Everything here runs **locally, on request**. There is no background access, no
watcher process, and nothing is sent anywhere. Each function is the same Win32
call a person would make with the mouse and keyboard — `SetForegroundWindow`,
`SHFileOperationW`, `GetSystemPowerStatus` — not a script, not an API, and not a
screen-scraper.

You can switch any of it off in **Settings → Computer access** (one switch per
group) or **Settings → Functions** (one switch per function). A function that is
off is also removed from the list handed to the brain, so it cannot be called by
accident either.

## Windows

The thing people ask for most: getting to the window they want without hunting
for it among twenty taskbar buttons.

| Function | Say | What it does |
|---|---|---|
| `focus_window` | *"switch to chrome"*, *"show me code"* | Brings the window to the front and gives it the keyboard. Matches the app name or the window title; if nothing matches and the app is installed, it starts it instead |
| `active_window` | *"what am I looking at"* | Says what the window in front is |
| `list_windows` | *"what windows are open"* | Lists what is open, front-most first, skipping tool windows and dialogs the way Alt+Tab does |
| `next_window` | *"next window"* | Alt+Tab, exactly |
| `previous_window` | *"go back a window"* | Alt+Shift+Tab |
| `minimize_window` | *"minimize this"* | Minimises the window in front |
| `maximize_window` | *"maximize this"* | Maximises it |
| `restore_window` | *"restore this window"* | Back to its normal size |
| `snap_window` | *"snap this to the left"*, *"put it on the right half"* | Half or quarter of the screen, inside the work area rather than under the taskbar |
| `move_to_monitor` | *"move this to the other screen"* | Sends it to another display, keeping its size and its place |

Windows only hands the foreground to the process that already has it. Wisp
attaches to the foreground thread's input queue for the duration of the call,
which is the documented way around that. If Windows still refuses, it raises the
window and says so rather than pretending it worked.

## Files and notes

Wisp looks in the folders files actually live in — Downloads, Documents, Desktop,
Pictures, Music, Videos, and your home folder — not across the whole drive. It
never leaves the profile folder unless you give it a path.

| Function | Say | What it does |
|---|---|---|
| `open_folder` | *"open my downloads"* | Opens one of those folders in Explorer. A full path also works |
| `find_file` | *"find the file budget"* | Searches the folders above by name, newest first, and says where the best match is |
| `open_file` | *"open the file payslip"* | Finds it and opens it in whatever handles it |
| `recycle_file` | *"delete the file old draft"* | Moves **one** file to the Recycle Bin. If two files match it names them both and does nothing |
| `empty_recycle_bin` | *"empty the recycle bin"* | Empties it. The only irreversible operation here, and it is one you asked for by name |
| `folder_size` | *"how big is my downloads folder"* | Total size and file count, counted with a hard cap so a huge folder cannot freeze it |
| `count_files` | *"how many files are in downloads"* | Files and folders, counted |
| `where_folder` | *"where is my downloads folder"* | The full path, and what is in it |
| `take_note` | *"take a note ring the plumber"* | Appends a timestamped line to `%APPDATA%\Wisp\notes.md` |
| `read_notes` | *"read my notes"* | Reads back the four most recent |
| `search_notes` | *"what did I note about the plumber"* | Finds notes that mention it |

The notes file is ordinary Markdown in your settings folder: open it, edit it,
sync it, delete it. Wisp does not own it.

## Clipboard and typing

| Function | Say | What it does |
|---|---|---|
| `copy_to_clipboard` | *"copy this to the clipboard"*, *"copy that"* | Puts what you said on the clipboard. With nothing said, it sends Ctrl+C — the same as copying the selection |
| `read_clipboard` | *"what's on my clipboard"* | Reads it out, up to 300 characters, and says how many words there are |
| `paste_clipboard` | *"paste it"* | Ctrl+V into whatever has the focus |
| `type_text` | *"type out hello there"* | Types it as real Unicode keystrokes into the window in front |

Characters are sent as `KEYEVENTF_UNICODE`, so any alphabet works and nothing
fires a keyboard shortcut by accident. Wisp never types unless you said "type".

## System

Read-only questions, mostly. These are the ones a person asks a computer out
loud.

| Function | Say | What it does |
|---|---|---|
| `battery_status` | *"how much battery is left"*, *"am I charging"* | Percentage, whether it is charging, and the time left |
| `system_status` | *"how is my computer doing"*, *"cpu usage"* | Processor load (measured over a short window, not a snapshot), memory in use, uptime |
| `top_processes` | *"what's using my cpu"*, *"what's eating my memory"* | The three hungriest applications, summed per application rather than per process |
| `disk_space` | *"how much space is left"* | Free and total for every connected drive |
| `network_status` | *"am I online"*, *"what's my ip"* | Whether there is a route out, this machine's address, and the Wi-Fi network name |
| `machine_info` | *"what are my specs"* | Processor, threads, memory, Windows version, computer name |
| `is_app_running` | *"is spotify running"* | Yes or no |
| `who_am_i` | *"who am i"*, *"who's logged in"* | The account, the machine and the Windows build |
| `why_slow` | *"why is my computer slow"* | One or two concrete reasons with the numbers behind them — the processor, the memory, a single greedy app, a full drive — and what would give the most back |
| `screenshot` | *"take a screenshot"* | Saves a PNG of every display to `Pictures\Wisp` |
| `copy_screenshot` | *"copy a screenshot"* | Puts the screenshot on the clipboard, ready to paste |
| `lock_pc`, `show_desktop`, `close_window`, `open_settings` | *"lock my pc"*, *"show desktop"*, *"close this window"*, *"open windows settings"* | The everyday ones |

## Power and screen

| Function | Say | What it does |
|---|---|---|
| `sleep_pc` | *"put the computer to sleep"* | Suspends. Asks Windows for the privilege a normal account already has |
| `shutdown_pc` | *"shut down the computer"* | `shutdown /s /t 30`, so there is a 30-second undo |
| `restart_pc` | *"restart the computer"* | The same, with `/r` |
| `sign_out` | *"sign out"* | Signs out of Windows |
| `cancel_shutdown` | *"cancel the shutdown"* | Stops a pending shutdown or restart |
| `turn_off_screen` | *"turn off the screen"* | Powers the display off without locking. Any key brings it back |
| `brightness_set` | *"set the brightness to 40"* | Screen brightness, through WMI — laptop panels and panels that expose it; other screens say so instead of pretending |
| `brightness_up` / `brightness_down` | *"dim the screen"*, *"brighten the screen"* | 20 % at a time |

## Queued actions

Any of these can also be queued: *"shut down in 30 minutes"*, *"lock the pc at
11pm"*, *"close chrome when spotify closes"*, *"dim the screen when I lock it"*.
The queue is one switch-free list in **Settings → Alarms & reminders**, and the
functions that work with it are `list_pending` (*"what's queued"*),
`next_up` (*"what's coming up"*) and `cancel_pending` (*"cancel everything you
queued"*). [docs/when.md](when.md) is the reference.

## What it will not do

* **Delete anything permanently.** Deleting goes to the Recycle Bin. The only
  exception is emptying the Recycle Bin itself, which is what you asked for.
* **Touch a file it cannot identify.** If two files match a name, it asks instead
  of choosing.
* **Act with administrator rights.** Wisp runs as an ordinary user, so anything
  that needs elevation fails with a message rather than prompting for a password.
  That is also why Wi-Fi and Bluetooth toggles are not built in: they need an
  elevated account, and a voice command that quietly fails is worse than one that
  was never offered.
* **Run in the background.** Every action is the direct result of a command you
  spoke, at the moment you spoke it.
* **Send anything anywhere** unless you chose a hosted brain, in which case your
  *command* goes to that service and nothing else does — no files, no clipboard,
  no screenshots.

## Trying it without speaking

**Settings → Test a command** runs a command through the brain without using the
microphone, shows what it decided and what it would say, and can run it. It is
also where to look when a command is understood wrongly: the page shows which
route was taken (instant phrase, Needle, hosted model) and the actions it chose.

The same thing can be done from a command line:

```powershell
Wisp.exe --bench-needle "what's using my cpu"
```

which prints the tool list it sent, the size of the prompt, and where the
milliseconds went.

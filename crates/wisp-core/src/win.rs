//! Thin wrappers over the Win32 calls the three executables share.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use windows::core::{w, Interface, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, IPersistFile, CLSCTX_ALL, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED, STGM_READ,
};
use windows::Win32::System::Shutdown::LockWorkStation;
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    VIRTUAL_KEY, VK_BACK, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_HOME, VK_LEFT,
    VK_LWIN, VK_MEDIA_NEXT_TRACK, VK_MEDIA_PLAY_PAUSE, VK_MEDIA_PREV_TRACK, VK_MEDIA_STOP, VK_MENU,
    VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP, VK_VOLUME_DOWN,
    VK_VOLUME_MUTE, VK_VOLUME_UP,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteKeyW, RegDeleteValueW, RegGetValueW, RegOpenKeyExW,
    RegSetValueExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_SET_VALUE, KEY_WRITE,
    REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
};
use windows::Win32::System::Threading::{
    CreateMutexW, OpenProcess, TerminateProcess, PROCESS_TERMINATE,
};
use windows::Win32::UI::Shell::{IShellLinkW, ShellExecuteW, ShellLink};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowW, GetForegroundWindow, GetShellWindow, GetSystemMetrics, GetWindow, GetWindowTextW,
    GetWindowThreadProcessId, IsWindowVisible, PostMessageW, SendMessageW, GW_OWNER, SM_CXVIRTUALSCREEN,
    SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_SHOWNORMAL, WM_CLOSE, WM_COMMAND,
};

pub fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(std::iter::once(0)).collect()
}

/// Initialise COM for this thread (single-threaded apartment). Safe to call repeatedly.
pub fn com_init() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
}

// ── Launching ────────────────────────────────────────────────────────────

/// `ShellExecute` "open" — works for exes, shortcuts, URLs, `shell:` paths
/// and protocol links like `ms-settings:`.
pub fn shell_open(target: &str, args: Option<&str>) -> Result<(), String> {
    let t = HSTRING::from(target);
    let a = args.map(HSTRING::from);
    let r = unsafe {
        ShellExecuteW(
            None,
            &HSTRING::from("open"),
            &t,
            a.as_ref().map_or(PCWSTR::null(), |a| PCWSTR(a.as_ptr())),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    // ShellExecute returns a value > 32 on success.
    if r.0 as isize > 32 {
        Ok(())
    } else {
        Err(format!("could not open {target} (code {})", r.0 as isize))
    }
}

/// Launch a Start-menu app by its AppUserModelID (works for Store apps too).
pub fn launch_app_id(app_id: &str) -> Result<(), String> {
    shell_open("explorer.exe", Some(&format!("shell:AppsFolder\\{app_id}")))
}

// ── Registry ─────────────────────────────────────────────────────────────

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Wisp";

/// Read a REG_DWORD value, e.g. whether Windows prefers light apps.
pub fn reg_get_dword(root: HKEY, key: &str, value: &str) -> Option<u32> {
    let mut data: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    let r = unsafe {
        RegGetValueW(
            root,
            &HSTRING::from(key),
            PCWSTR(HSTRING::from(value).as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut data as *mut u32).cast()),
            Some(&mut size),
        )
    };
    r.is_ok().then_some(data)
}

pub fn reg_get_string(root: HKEY, key: &str, value: Option<&str>) -> Option<String> {
    let mut buf = vec![0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    let v = value.map(HSTRING::from);
    let r = unsafe {
        RegGetValueW(
            root,
            &HSTRING::from(key),
            v.as_ref().map_or(PCWSTR::null(), |v| PCWSTR(v.as_ptr())),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if r.is_err() {
        return None;
    }
    let len = (size as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..len]))
}

fn reg_set_string(root: HKEY, key: &str, name: &str, value: &str) -> Result<(), String> {
    unsafe {
        let mut hkey = HKEY::default();
        RegCreateKeyExW(
            root,
            &HSTRING::from(key),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut hkey,
            None,
        )
        .ok()
        .map_err(|e| e.to_string())?;
        let data = wide(value);
        let bytes = std::slice::from_raw_parts(data.as_ptr().cast::<u8>(), data.len() * 2);
        let r = RegSetValueExW(hkey, &HSTRING::from(name), None, REG_SZ, Some(bytes));
        let _ = RegCloseKey(hkey);
        r.ok().map_err(|e| e.to_string())
    }
}

fn reg_set_dword(root: HKEY, key: &str, name: &str, value: u32) -> Result<(), String> {
    unsafe {
        let mut hkey = HKEY::default();
        RegCreateKeyExW(
            root,
            &HSTRING::from(key),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut hkey,
            None,
        )
        .ok()
        .map_err(|e| e.to_string())?;
        let r = RegSetValueExW(hkey, &HSTRING::from(name), None, REG_DWORD, Some(&value.to_le_bytes()));
        let _ = RegCloseKey(hkey);
        r.ok().map_err(|e| e.to_string())
    }
}

/// Register (or remove) the agent under HKCU\...\Run so it starts at login.
pub fn set_autostart(enabled: bool, agent_exe: &Path) -> Result<(), String> {
    if enabled {
        let cmd = format!("\"{}\" --background", agent_exe.display());
        reg_set_string(HKEY_CURRENT_USER, RUN_KEY, crate::PRODUCT, &cmd)
    } else {
        unsafe {
            let mut hkey = HKEY::default();
            if RegOpenKeyExW(HKEY_CURRENT_USER, &HSTRING::from(RUN_KEY), None, KEY_SET_VALUE, &mut hkey)
                .is_ok()
            {
                let _ = RegDeleteValueW(hkey, &HSTRING::from(crate::PRODUCT));
                let _ = RegCloseKey(hkey);
            }
        }
        Ok(())
    }
}

pub fn autostart_enabled() -> bool {
    reg_get_string(HKEY_CURRENT_USER, RUN_KEY, Some(crate::PRODUCT)).is_some()
}

/// Add the "Apps & features" entry so Windows can uninstall us.
pub fn register_uninstaller(install_dir: &Path, version: &str, size_kb: u32) -> Result<(), String> {
    let uninst = install_dir.join(crate::UNINSTALL_EXE);
    let icon = install_dir.join(crate::AGENT_EXE);
    let k = UNINSTALL_KEY;
    let r = HKEY_CURRENT_USER;
    reg_set_string(r, k, "DisplayName", "Wisp")?;
    reg_set_string(r, k, "DisplayVersion", version)?;
    reg_set_string(r, k, "Publisher", "Wisp")?;
    reg_set_string(r, k, "DisplayIcon", &icon.display().to_string())?;
    reg_set_string(r, k, "InstallLocation", &install_dir.display().to_string())?;
    reg_set_string(r, k, "UninstallString", &format!("\"{}\" --uninstall", uninst.display()))?;
    reg_set_dword(r, k, "NoModify", 1)?;
    reg_set_dword(r, k, "NoRepair", 1)?;
    reg_set_dword(r, k, "EstimatedSize", size_kb)?;
    Ok(())
}

pub fn unregister_uninstaller() {
    unsafe {
        let _ = RegDeleteKeyW(HKEY_CURRENT_USER, &HSTRING::from(UNINSTALL_KEY));
    }
}

pub fn installed_location() -> Option<PathBuf> {
    reg_get_string(HKEY_CURRENT_USER, UNINSTALL_KEY, Some("InstallLocation")).map(PathBuf::from)
}

/// The Start-menu folder every shortcut for this product lives in. Both the
/// installer and the uninstaller ask for it here, so the two cannot drift
/// apart and leave shortcuts behind.
pub fn start_menu_dir() -> PathBuf {
    use windows::Win32::UI::Shell::FOLDERID_Programs;
    known_folder(&FOLDERID_Programs).unwrap_or_default().join(crate::PRODUCT)
}

/// The desktop shortcut, if the install made one.
pub fn desktop_link() -> PathBuf {
    use windows::Win32::UI::Shell::FOLDERID_Desktop;
    known_folder(&FOLDERID_Desktop).unwrap_or_default().join(format!("{} Settings.lnk", crate::PRODUCT))
}

/// Look up an executable registered under `App Paths` (e.g. `chrome.exe`).
pub fn app_path(exe: &str) -> Option<PathBuf> {
    let key = format!(r"Software\Microsoft\Windows\CurrentVersion\App Paths\{exe}");
    for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        if let Some(p) = reg_get_string(root, &key, None) {
            let p = PathBuf::from(p.trim_matches('"'));
            if p.exists() {
                return Some(p);
            }
        }
    }
    None
}

// ── Shortcuts ────────────────────────────────────────────────────────────

/// Create a `.lnk` shortcut. COM must be initialised on this thread.
pub fn create_shortcut(lnk: &Path, target: &Path, args: &str, description: &str) -> Result<(), String> {
    unsafe {
        let link: IShellLinkW =
            CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(|e| e.to_string())?;
        link.SetPath(&HSTRING::from(target.as_os_str())).map_err(|e| e.to_string())?;
        link.SetArguments(&HSTRING::from(args)).map_err(|e| e.to_string())?;
        link.SetDescription(&HSTRING::from(description)).map_err(|e| e.to_string())?;
        if let Some(dir) = target.parent() {
            link.SetWorkingDirectory(&HSTRING::from(dir.as_os_str())).map_err(|e| e.to_string())?;
        }
        link.SetIconLocation(&HSTRING::from(target.as_os_str()), 0).map_err(|e| e.to_string())?;
        let file: IPersistFile = link.cast().map_err(|e| e.to_string())?;
        if let Some(dir) = lnk.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        file.Save(&HSTRING::from(lnk.as_os_str()), true).map_err(|e| e.to_string())
    }
}

/// Resolve a `.lnk` file to its target path (without touching the network).
pub fn shortcut_target(lnk: &Path) -> Option<PathBuf> {
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let file: IPersistFile = link.cast().ok()?;
        file.Load(&HSTRING::from(lnk.as_os_str()), STGM_READ).ok()?;
        let mut buf = [0u16; 1024];
        link.GetPath(&mut buf, std::ptr::null_mut(), 0).ok()?;
        let len = buf.iter().position(|&c| c == 0).unwrap_or(0);
        if len == 0 {
            return None;
        }
        Some(PathBuf::from(String::from_utf16_lossy(&buf[..len])))
    }
}

/// The user's Documents folder, or None when it can't be resolved.
pub fn documents_folder() -> Option<PathBuf> {
    known_folder(&windows::Win32::UI::Shell::FOLDERID_Documents)
}

/// Known folders like the Start Menu, Desktop.
pub fn known_folder(id: &windows::core::GUID) -> Option<PathBuf> {
    use windows::Win32::UI::Shell::{SHGetKnownFolderPath, KF_FLAG_DEFAULT};
    unsafe {
        let p: PWSTR = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s.map(PathBuf::from)
    }
}

// ── Processes ────────────────────────────────────────────────────────────

pub struct ProcessInfo {
    pub pid: u32,
    pub exe: String,
}

pub fn processes() -> Vec<ProcessInfo> {
    let mut out = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return out;
        };
        let mut entry = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(0);
                out.push(ProcessInfo {
                    pid: entry.th32ProcessID,
                    exe: String::from_utf16_lossy(&entry.szExeFile[..len]),
                });
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    out
}

/// Politely close every top-level window owned by the given processes.
/// Returns how many windows were asked to close.
pub fn close_windows_of(pids: &[u32]) -> usize {
    struct Ctx<'a> {
        pids: &'a [u32],
        count: usize,
    }
    unsafe extern "system" fn cb(hwnd: HWND, lp: LPARAM) -> windows::core::BOOL {
        let ctx = &mut *(lp.0 as *mut Ctx);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let owned = GetWindow(hwnd, GW_OWNER).map(|h| !h.is_invalid()).unwrap_or(false);
        if ctx.pids.contains(&pid) && IsWindowVisible(hwnd).as_bool() && !owned {
            let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            ctx.count += 1;
        }
        true.into()
    }
    let mut ctx = Ctx { pids, count: 0 };
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut ctx as *mut Ctx as isize));
    }
    ctx.count
}

pub fn kill_process(pid: u32) -> bool {
    unsafe {
        match OpenProcess(PROCESS_TERMINATE, false, pid) {
            Ok(h) => {
                let ok = TerminateProcess(h, 0).is_ok();
                let _ = CloseHandle(h);
                ok
            }
            Err(_) => false,
        }
    }
}

/// Kill every process with this exe name except ourselves.
pub fn kill_by_exe(exe: &str) -> usize {
    let me = std::process::id();
    processes()
        .into_iter()
        .filter(|p| p.pid != me && p.exe.eq_ignore_ascii_case(exe))
        .filter(|p| kill_process(p.pid))
        .count()
}

pub fn is_running(exe: &str) -> bool {
    let me = std::process::id();
    processes().iter().any(|p| p.pid != me && p.exe.eq_ignore_ascii_case(exe))
}

/// Ask every process with this exe name to close, and wait for it to go.
///
/// Returns how many were running. `WM_CLOSE` first, and only after `grace` has
/// passed without them leaving does anything get terminated. An installer that
/// reaches straight for `taskkill` looks, from the outside, like something
/// killing processes it has no business touching — and it also loses whatever
/// the agent was doing. Returns `true` when nothing is left running.
pub fn close_processes_named(exe: &str, grace: std::time::Duration) -> bool {
    let me = std::process::id();
    let targets: Vec<(u32, HWND)> = windows_of(exe).into_iter().filter(|(pid, _)| *pid != me).collect();
    if targets.is_empty() {
        // Nothing visible to ask; make sure there is nothing running either.
        return !is_running(exe);
    }
    for (_, hwnd) in &targets {
        unsafe {
            let _ = PostMessageW(Some(*hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
    let deadline = std::time::Instant::now() + grace;
    while std::time::Instant::now() < deadline {
        if !is_running(exe) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    if !is_running(exe) {
        return true;
    }
    // It did not close when asked. Terminate it rather than let the install
    // fight over locked files.
    kill_by_exe(exe);
    for _ in 0..30 {
        if !is_running(exe) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    !is_running(exe)
}

/// Every top-level window belonging to a process with this exe name, with the
/// process id that owns it. The window class is not needed: matching by the
/// owner's image name covers the tray window and the message-only windows alike.
fn windows_of(exe: &str) -> Vec<(u32, HWND)> {
    let wanted: Vec<u32> = processes()
        .into_iter()
        .filter(|p| p.exe.eq_ignore_ascii_case(exe))
        .map(|p| p.pid)
        .collect();
    if wanted.is_empty() {
        return Vec::new();
    }
    let mut found: Vec<(u32, HWND)> = Vec::new();
    let ptr = &mut found as *mut Vec<(u32, HWND)>;
    unsafe {
        let _ = EnumWindows(Some(collect_by_pid), LPARAM(ptr as isize));
    }
    found.retain(|(pid, _)| wanted.contains(pid));
    found
}

unsafe extern "system" fn collect_by_pid(hwnd: HWND, lp: LPARAM) -> windows::core::BOOL {
    let list = &mut *(lp.0 as *mut Vec<(u32, HWND)>);
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid != 0 {
        list.push((pid, hwnd));
    }
    true.into()
}

/// Hold a named mutex for the life of the process. Returns `None` when
/// another process already owns it.
pub struct SingleInstance(HANDLE);

impl SingleInstance {
    pub fn acquire(name: &str) -> Option<Self> {
        unsafe {
            let h = CreateMutexW(None, true, &HSTRING::from(name)).ok()?;
            if windows::Win32::Foundation::GetLastError() == windows::Win32::Foundation::ERROR_ALREADY_EXISTS {
                let _ = CloseHandle(h);
                return None;
            }
            Some(SingleInstance(h))
        }
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Spawn a process without a console window.
pub fn spawn_detached(exe: &Path, args: &[&str]) -> std::io::Result<()> {
    spawn_hidden(exe, args)
}

/// Same, but accepting a bare command name resolved through `PATH`.
pub fn spawn_hidden(exe: impl AsRef<OsStr>, args: &[&str]) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new(exe)
        .args(args)
        .creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW)
        .spawn()
        .map(|_| ())
}

/// Run an already-built [`std::process::Command`] with no console window and no
/// inheritance, so the parent can exit immediately without taking the child
/// with it. Used by the uninstaller for its second half.
pub fn spawn_detached_cmd(cmd: &mut std::process::Command) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // Deliberately not CREATE_BREAKAWAY_FROM_JOB: it is only permitted when the
    // caller is already in a job object that allows breakaway, and asking for it
    // otherwise fails the whole call with ERROR_ACCESS_DENIED. DETACHED_PROCESS
    // is what actually keeps the child alive past the parent's exit.
    cmd.creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW).spawn().map(|_| ())
}

/// Is this process still running? Asking the kernel to wait on the handle with
/// a zero timeout distinguishes "running" from "already gone" — a pid alone
/// cannot, because pids are reused.
pub fn process_alive(pid: u32) -> bool {
    const SYNCHRONIZE: u32 = 0x0010_0000;
    const WAIT_TIMEOUT: u32 = 0x0000_0102;
    unsafe {
        let Ok(h) = OpenProcess(
            windows::Win32::System::Threading::PROCESS_ACCESS_RIGHTS(SYNCHRONIZE),
            false,
            pid,
        ) else {
            return false;
        };
        let r = windows::Win32::System::Threading::WaitForSingleObject(h, 0);
        let _ = CloseHandle(h);
        // WAIT_TIMEOUT means it did not signal: still running.
        r == windows::Win32::Foundation::WAIT_EVENT(WAIT_TIMEOUT)
    }
}

// ── Keyboard, media keys and volume ──────────────────────────────────────

/// Press or release one virtual key. Returns how many events Windows accepted
/// (0 when the input couldn't be delivered — a locked or disconnected session).
fn send_key(vk: VIRTUAL_KEY, up: bool) -> u32 {
    unsafe {
        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        SendInput(&[input], std::mem::size_of::<INPUT>() as i32)
    }
}

/// Tap a key: down then up. Media and volume keys listen for exactly this.
/// Returns true when Windows took both halves of the press.
pub fn tap_key(vk: VIRTUAL_KEY) -> bool {
    send_key(vk, false) == 1 && send_key(vk, true) == 1
}

/// The media-key meaning of each action in [`crate::brain::MediaKey`].
/// Play/pause is one key on Windows, so pause and resume share it — every
/// mainstream player treats it as a toggle.
pub fn media_vk(key: crate::brain::MediaKey) -> VIRTUAL_KEY {
    use crate::brain::MediaKey::*;
    match key {
        Pause | Resume => VK_MEDIA_PLAY_PAUSE,
        Next => VK_MEDIA_NEXT_TRACK,
        Previous => VK_MEDIA_PREV_TRACK,
        Stop => VK_MEDIA_STOP,
    }
}

/// Map one written key name ("ctrl", "esc", "f5", "a") to a virtual key.
fn key_vk(name: &str) -> Option<VIRTUAL_KEY> {
    let n = name.trim().to_lowercase();
    let fixed = match n.as_str() {
        "ctrl" | "control" => Some(VK_CONTROL),
        "shift" => Some(VK_SHIFT),
        "alt" => Some(VK_MENU),
        "win" | "windows" | "meta" | "super" | "cmd" => Some(VK_LWIN),
        "esc" | "escape" => Some(VK_ESCAPE),
        "tab" => Some(VK_TAB),
        "enter" | "return" => Some(VK_RETURN),
        "space" | "spacebar" => Some(VK_SPACE),
        "backspace" | "back" => Some(VK_BACK),
        "delete" | "del" => Some(VK_DELETE),
        "up" => Some(VK_UP),
        "down" => Some(VK_DOWN),
        "left" => Some(VK_LEFT),
        "right" => Some(VK_RIGHT),
        "home" => Some(VK_HOME),
        "end" => Some(VK_END),
        "pageup" | "page up" | "pgup" => Some(VK_PRIOR),
        "pagedown" | "page down" | "pgdn" => Some(VK_NEXT),
        "play" | "playpause" | "play/pause" => Some(VK_MEDIA_PLAY_PAUSE),
        "next" | "nexttrack" | "next track" => Some(VK_MEDIA_NEXT_TRACK),
        "previous" | "prev" | "previoustrack" => Some(VK_MEDIA_PREV_TRACK),
        "stop" | "stopmedia" => Some(VK_MEDIA_STOP),
        "mute" => Some(VK_VOLUME_MUTE),
        "volumeup" | "volume up" => Some(VK_VOLUME_UP),
        "volumedown" | "volume down" => Some(VK_VOLUME_DOWN),
        _ => None,
    };
    if fixed.is_some() {
        return fixed;
    }
    // Function keys.
    if let Some(rest) = n.strip_prefix('f') {
        if let Ok(i) = rest.parse::<u16>() {
            if (1..=24).contains(&i) {
                return Some(VIRTUAL_KEY(0x70 + i - 1));
            }
        }
        return None;
    }
    // A single letter or digit.
    let mut chars = n.chars();
    let first = chars.next()?;
    (chars.next().is_none() && first.is_ascii_alphanumeric())
        .then(|| VIRTUAL_KEY(first.to_ascii_uppercase() as u16))
}

/// Press a key combination like `ctrl+shift+esc` or `win+d`. Modifiers are
/// held while the last key is tapped.
pub fn press_keys(combo: &str) -> Result<String, String> {
    let names: Vec<&str> = combo.split(['+', '-']).map(str::trim).filter(|s| !s.is_empty()).collect();
    if names.is_empty() {
        return Err("no keys to press".into());
    }
    let keys: Vec<VIRTUAL_KEY> = names
        .iter()
        .map(|n| key_vk(n).ok_or_else(|| format!("I don't know the key \"{n}\"")))
        .collect::<Result<_, _>>()?;
    let mut sent = 0;
    for k in &keys {
        sent += send_key(*k, false);
    }
    std::thread::sleep(std::time::Duration::from_millis(30));
    for k in keys.iter().rev() {
        sent += send_key(*k, true);
    }
    if sent < keys.len() as u32 * 2 {
        return Err("Windows wouldn't take the keystrokes (is the screen locked?)".into());
    }
    Ok(format!("pressed {}", names.join(" + ")))
}

/// The audio endpoint whose volume the volume keys change.
fn endpoint_volume() -> Option<IAudioEndpointVolume> {
    unsafe {
        com_init();
        let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia).ok()?;
        device.Activate(CLSCTX_ALL, None).ok()
    }
}

/// Current master volume, 0–100.
pub fn volume_percent() -> Option<u8> {
    let v = endpoint_volume()?;
    let level = unsafe { v.GetMasterVolumeLevelScalar() }.ok()?;
    Some((level * 100.0).round().clamp(0.0, 100.0) as u8)
}

/// Set the master volume, 0–100.
pub fn set_volume_percent(percent: u8) -> Result<u8, String> {
    let percent = percent.min(100);
    match endpoint_volume() {
        Some(v) => {
            unsafe { v.SetMasterVolumeLevelScalar(percent as f32 / 100.0, std::ptr::null()) }
                .map_err(|e| format!("Windows refused the volume change: {e}"))?;
            Ok(percent)
        }
        None => {
            // No audio endpoint: fall back to the volume keys, a small step at
            // a time. Better than doing nothing at all.
            let steps = (percent as i16 - volume_percent().unwrap_or(50) as i16) / 2;
            nudge_volume(steps);
            Ok(percent)
        }
    }
}

/// Move the volume by `steps` × 2% using the volume keys.
pub fn nudge_volume(steps: i16) {
    let key = if steps >= 0 { VK_VOLUME_UP } else { VK_VOLUME_DOWN };
    for _ in 0..steps.unsigned_abs().min(50) {
        tap_key(key);
        std::thread::sleep(std::time::Duration::from_millis(12));
    }
}

pub fn muted() -> Option<bool> {
    let v = endpoint_volume()?;
    unsafe { v.GetMute() }.ok().map(|m| m.as_bool())
}

/// Make sure the endpoint the assistant speaks into can actually be heard.
///
/// A perfect pipeline into a muted device is silence, and nothing in the app
/// could tell the difference between that and a bug: the stream opens, the
/// samples are consumed, the player reports healthy. Windows keeps per-device
/// volume, so this happens more easily than it sounds — a default speaker pair
/// sitting at 0% looked exactly like a working install from inside the process.
///
/// A muted endpoint is un-muted, and a volume under 5% is raised to 50%.
/// Anything already audible is left exactly as it is, so a quiet room stays
/// quiet. Returns a sentence describing what was changed, for the log.
pub fn ensure_audible() -> Option<String> {
    const AUDIBLE_FLOOR: u8 = 5;
    const AUDIBLE_TARGET: u8 = 50;
    let mut changed = Vec::new();
    if muted() == Some(true) {
        match set_muted(false) {
            Ok(()) => changed.push("it was muted".to_string()),
            Err(e) => return Some(format!("the speakers are muted and would not un-mute: {e}")),
        }
    }
    if let Some(percent) = volume_percent() {
        if percent < AUDIBLE_FLOOR {
            match set_volume_percent(AUDIBLE_TARGET) {
                Ok(_) => changed.push(format!("its volume was {percent}%")),
                Err(e) => return Some(format!("the speakers are at {percent}% and would not turn up: {e}")),
            }
        }
    }
    if changed.is_empty() {
        None
    } else {
        Some(format!("{}, raised so a reply can be heard", changed.join(" and ")))
    }
}

pub fn set_muted(on: bool) -> Result<(), String> {
    match endpoint_volume() {
        Some(v) => unsafe { v.SetMute(on, std::ptr::null()) }.map_err(|e| format!("Windows refused the mute: {e}")),
        None => {
            // Only a toggle is available without COM, so check first.
            if muted() != Some(on) {
                tap_key(VK_VOLUME_MUTE);
            }
            Ok(())
        }
    }
}

// ── Windows the user is looking at ───────────────────────────────────────

/// Ask the window in front to close (the X button, politely).
pub fn close_foreground_window() -> Result<String, String> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return Err("there's no window in front to close".into());
        }
        if hwnd == GetShellWindow() {
            return Err("that's the desktop, not a window".into());
        }
        let title = window_title(hwnd);
        PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)).map_err(|e| e.to_string())?;
        Ok(if title.is_empty() { "closed the window".into() } else { format!("closed {title}") })
    }
}

fn window_title(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// Minimize everything — the same as clicking the far edge of the taskbar.
pub fn show_desktop() -> Result<(), String> {
    // 419 is the long-standing "toggle desktop" command of the shell tray.
    const TOGGLE_DESKTOP: usize = 419;
    unsafe {
        let tray = FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()).map_err(|e| e.to_string())?;
        SendMessageW(tray, WM_COMMAND, Some(WPARAM(TOGGLE_DESKTOP)), Some(LPARAM(0)));
    }
    Ok(())
}

pub fn lock_workstation() -> Result<(), String> {
    unsafe { LockWorkStation() }.map_err(|e| e.to_string())
}

// ── Screenshots ──────────────────────────────────────────────────────────

/// One capture of the desktop: RGBA, top-down, every monitor.
pub struct Capture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Capture every monitor into a PNG and return its path.
pub fn screenshot() -> Result<PathBuf, String> {
    let shot = capture_screen()?;
    let path = screenshot_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let file = std::fs::File::create(&path).map_err(|e| format!("couldn't save the screenshot: {e}"))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), shot.width, shot.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(&shot.rgba).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Capture the screen and put it on the clipboard, ready to paste: the form
/// people actually want "take a screenshot" for most of the time.
pub fn copy_screenshot_to_clipboard() -> Result<(u32, u32), String> {
    let shot = capture_screen()?;
    set_clipboard_image(shot.width, shot.height, &shot.rgba)?;
    Ok((shot.width, shot.height))
}

/// Read every monitor's pixels. Split out from [`screenshot`] so the clipboard
/// and the saved file share one capture path — two copies of this much unsafe
/// code is two chances to get the alpha byte wrong.
pub fn capture_screen() -> Result<Capture, String> {
    unsafe {
        let (x, y) = (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN));
        let (w, h) = (GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN));
        if w <= 0 || h <= 0 {
            return Err("couldn't measure the screen".into());
        }
        let screen = GetDC(None);
        if screen.is_invalid() {
            return Err("couldn't get the screen".into());
        }
        // Every GDI object is checked as it is made: creating a device context
        // or a bitmap can fail under memory pressure, and the old code carried
        // on regardless and blitted into a null surface.
        let mem = CreateCompatibleDC(Some(screen));
        let bitmap = if mem.is_invalid() { Default::default() } else { CreateCompatibleBitmap(screen, w, h) };
        if mem.is_invalid() || bitmap.is_invalid() {
            if !mem.is_invalid() {
                let _ = DeleteDC(mem);
            }
            ReleaseDC(None, screen);
            return Err("couldn't create the capture surface".into());
        }
        let old = SelectObject(mem, bitmap.into());
        let mut pixels = vec![0u8; (w as usize) * (h as usize) * 4];
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let blit = BitBlt(mem, 0, 0, w, h, Some(screen), x, y, SRCCOPY);
        // `GetDIBits` refuses to read a bitmap that is still selected into a
        // device context, so it has to come out first — which is also the only
        // order in which the copy can be trusted to have happened.
        SelectObject(mem, old);
        let lines = if blit.is_ok() {
            GetDIBits(mem, bitmap, 0, h as u32, Some(pixels.as_mut_ptr().cast()), &mut info, DIB_RGB_COLORS)
        } else {
            0
        };
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);

        blit.map_err(|e| format!("couldn't copy the screen: {e}"))?;
        if lines == 0 {
            return Err("couldn't read the screen pixels".into());
        }
        // GDI hands back BGRA with the alpha byte left at zero, which as RGBA
        // would be a fully transparent PNG.
        for px in pixels.chunks_exact_mut(4) {
            px.swap(0, 2);
            px[3] = 255;
        }
        Ok(Capture { width: w as u32, height: h as u32, rgba: pixels })
    }
}

/// `Pictures\Wisp\Screenshot 2026-10-02 08.36.24.png`, falling back to
/// the app's data folder when there is no Pictures library.
fn screenshot_path() -> PathBuf {
    use windows::Win32::UI::Shell::FOLDERID_Pictures;
    let dir = known_folder(&FOLDERID_Pictures)
        .unwrap_or_else(crate::paths::data_dir)
        .join(crate::PRODUCT);
    let t = unsafe { GetLocalTime() };
    dir.join(format!(
        "Screenshot {:04}-{:02}-{:02} {:02}.{:02}.{:02}.png",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
    ))
}

// ── Running things and reading what they say ─────────────────────────────

/// Run a program, wait for it (up to `timeout`) and return its output.
///
/// Used by Python and PowerShell functions, where the script's answer is the
/// point. Both streams are captured, so an error message is visible rather
/// than lost in a window nobody sees.
pub fn run_capture(exe: impl AsRef<OsStr>, args: &[&str], timeout: std::time::Duration) -> Result<String, String> {
    run_capture_command(exe, args, &[], timeout)
}

/// [`run_capture`] with environment variables for the child.
pub fn run_capture_command(
    exe: impl AsRef<OsStr>,
    args: &[&str],
    env: &[(String, String)],
    timeout: std::time::Duration,
) -> Result<String, String> {
    use std::io::Read;
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let mut command = std::process::Command::new(exe.as_ref());
    command.args(args);
    for (k, v) in env {
        command.env(k, v);
    }
    let mut child = command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("couldn't start {}: {e}", exe.as_ref().to_string_lossy()))?;

    let collect = |mut pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = String::new();
            if let Some(p) = pipe.as_mut() {
                let mut bytes = Vec::new();
                let _ = p.read_to_end(&mut bytes);
                buf = String::from_utf8_lossy(&bytes).into_owned();
            }
            buf
        })
    };
    let out = collect(child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>));
    let err = collect(child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>));

    let deadline = std::time::Instant::now() + timeout;
    let started = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(poll_gap(started.elapsed())),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Err(e) => return Err(format!("waiting for the program failed: {e}")),
        }
    };
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    let text = if stdout.trim().is_empty() { stderr } else { stdout };
    match status {
        Some(s) if s.success() => Ok(text.trim().to_string()),
        Some(s) => Err(format!(
            "it exited with {}: {}",
            s.code().map(|c| c.to_string()).unwrap_or_else(|| "no code".into()),
            text.trim().lines().last().unwrap_or("no output").trim()
        )),
        None => Err(format!("it took longer than {} seconds", timeout.as_secs())),
    }
}

/// How long to wait before asking again whether a child has finished.
///
/// A Python or PowerShell function is short, and a flat 25 ms poll spent most
/// of its time asleep: a script that took 60 ms was reported as 100. The gap
/// starts short enough to notice an immediate exit and lengthens so a long
/// script still costs almost nothing.
fn poll_gap(elapsed: std::time::Duration) -> std::time::Duration {
    let ms = elapsed.as_millis();
    let gap = if ms < 50 {
        2
    } else if ms < 250 {
        5
    } else if ms < 1000 {
        10
    } else {
        25
    };
    std::time::Duration::from_millis(gap)
}

/// Stop a running process by image name. The agent has no IPC, so this is how
/// the settings app's Stop button and the tray's Exit both end it.
pub fn stop_process(image: &str) -> bool {
    run_capture("taskkill", &["/IM", image, "/F"], std::time::Duration::from_secs(10)).is_ok()
}

/// Where Python is, if it's installed.
///
/// Windows ships a 0-byte "app execution alias" for `python.exe` in
/// `WindowsApps` that just opens the Store, so those are skipped. The `py`
/// launcher is preferred: it always picks the newest real install.
pub fn find_python(configured: &str) -> Option<PathBuf> {
    let configured = configured.trim().trim_matches('"');
    if !configured.is_empty() {
        let p = PathBuf::from(configured);
        return p.exists().then_some(p);
    }
    let usable = |p: &Path| -> bool {
        let Ok(meta) = std::fs::metadata(p) else { return false };
        // Store aliases are 0-byte reparse points.
        meta.len() > 0 && !p.to_string_lossy().to_ascii_lowercase().contains("\\windowsapps\\")
    };
    let named = |dir: &Path| -> Option<PathBuf> {
        ["py.exe", "python.exe"].iter().map(|n| dir.join(n)).find(|p| usable(p))
    };

    // The real installs first: a handful of directory reads, against walking
    // every entry on PATH. The settings app asks for this on every frame, and
    // walking PATH twice a frame on a machine without Python is what made the
    // window stutter.
    let mut roots: Vec<PathBuf> = Vec::new();
    for var in ["LOCALAPPDATA", "ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(root) = std::env::var_os(var) {
            let root = PathBuf::from(root);
            roots.push(root.join("Programs").join("Python"));
            roots.push(root.join("Python"));
        }
    }
    let mut installs: Vec<PathBuf> = Vec::new();
    for root in &roots {
        let Ok(entries) = std::fs::read_dir(root) else { continue };
        for entry in entries.flatten() {
            if let Some(found) = named(&entry.path()) {
                installs.push(found);
            }
        }
        // A bare `Python\py.exe`, with no version folder in between.
        if let Some(found) = named(root) {
            installs.push(found);
        }
    }
    // Newest first by path, which is how these sort: Python313 > Python312.
    installs.sort();
    if let Some(found) = installs.pop() {
        return Some(found);
    }

    // Then whatever PATH points at. `python3.exe` comes last: on Windows the
    // only thing that name usually resolves to is the Store alias.
    if let Some(path) = std::env::var_os("PATH") {
        let dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
        for name in ["py.exe", "python.exe", "python3.exe"] {
            for dir in &dirs {
                let candidate = dir.join(name);
                if usable(&candidate) {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

// ── Clock ────────────────────────────────────────────────────────────────

/// Wall-clock time in the user's timezone.
pub struct LocalTime {
    pub year: u16,
    pub month: u16,
    pub day: u16,
    pub hour: u16,
    pub minute: u16,
    pub second: u16,
    /// 0 = Sunday.
    pub weekday: u16,
}

pub fn local_time() -> LocalTime {
    let t = unsafe { GetLocalTime() };
    LocalTime {
        year: t.wYear,
        month: t.wMonth,
        day: t.wDay,
        hour: t.wHour,
        minute: t.wMinute,
        second: t.wSecond,
        weekday: t.wDayOfWeek,
    }
}

// ══════════════════════════════════════════════════════════════════════════
// The machine itself: windows, monitors, the clipboard, drives and power
// ══════════════════════════════════════════════════════════════════════════
//
// Everything below is the half of "control my computer" that cannot be done by
// launching a program: you cannot switch to a window, read the clipboard or ask
// the battery by starting a process. The wording the user hears lives in
// `crate::machine`; this is the mechanism.

use windows::Win32::Foundation::{FILETIME, HGLOBAL};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, MonitorFromWindow, HDC, HMONITOR, MONITORINFO,
    MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::Security::{
    AdjustTokenPrivileges, LookupPrivilegeValueW, SE_PRIVILEGE_ENABLED,
    TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Power::{GetSystemPowerStatus, SetSuspendState, SYSTEM_POWER_STATUS};
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentProcess, GetCurrentThreadId, GetProcessTimes, GetSystemTimes,
    OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::KEYEVENTF_UNICODE;
use windows::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetLogicalDrives};
use windows::Win32::UI::Shell::{SHFileOperationW, SHEmptyRecycleBinW, SHFILEOPSTRUCTW, FO_DELETE};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, GetWindowLongW, IsIconic, SetForegroundWindow, SetWindowPos, ShowWindow,
    GWL_EXSTYLE, HWND_TOP, SWP_NOACTIVATE, SWP_NOZORDER, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE,
    WM_SYSCOMMAND, WS_EX_TOOLWINDOW,
};

/// `CF_UNICODETEXT`. Spelled out rather than imported: the constant lives in a
/// different module in almost every version of the bindings.
const CF_UNICODETEXT: u32 = 13;
/// `CF_DIB`: the clipboard's own bitmap format, which is what Paste expects.
const CF_DIB: u32 = 8;
/// `SHEmptyRecycleBinW` flags.
const SHERB_NOCONFIRMATION: u32 = 0x1;
const SHERB_NOPROGRESSUI: u32 = 0x2;
const SHERB_NOSOUND: u32 = 0x4;

/// One window a person can see, in front-to-back order.
pub struct WindowRef {
    pub hwnd: HWND,
    pub pid: u32,
    pub title: String,
    /// Image name of the process that owns it, without `.exe`.
    pub app: String,
}

impl WindowRef {
    /// The title with the app's own name removed, for talking about it:
    /// "[chrome.exe] GitHub" reads worse than either half alone.
    pub fn spoken_title(&self) -> String {
        let t = self.title.trim();
        if t.chars().count() <= 70 {
            t.to_string()
        } else {
            format!("{}…", t.chars().take(67).collect::<String>())
        }
    }
}

struct WindowCollect<'a> {
    apps: &'a std::collections::HashMap<u32, String>,
    out: Vec<WindowRef>,
}

unsafe extern "system" fn collect_switchable(hwnd: HWND, lp: LPARAM) -> windows::core::BOOL {
    let ctx = &mut *(lp.0 as *mut WindowCollect);
    // The same filter Alt+Tab uses, for the same reason: a window nobody can
    // switch to is noise in a list that is read out loud.
    if !IsWindowVisible(hwnd).as_bool() {
        return true.into();
    }
    if GetWindow(hwnd, GW_OWNER).map(|h| !h.is_invalid()).unwrap_or(false) {
        return true.into();
    }
    if GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0 {
        return true.into();
    }
    let title = window_title(hwnd);
    let title = title.trim();
    if title.is_empty() || title.eq_ignore_ascii_case("Program Manager") {
        return true.into();
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    let exe = ctx.apps.get(&pid).cloned().unwrap_or_default();
    // Our own bubble, caption and settings window are windows like any other,
    // and listing them would be absurd.
    if exe.eq_ignore_ascii_case(crate::AGENT_EXE) || exe.eq_ignore_ascii_case(crate::CONFIG_EXE) {
        return true.into();
    }
    ctx.out.push(WindowRef { hwnd, pid, title: title.to_string(), app: window_app_name(&exe) });
    true.into()
}

/// Every window the user could switch to, front-most first.
pub fn switchable_windows() -> Vec<WindowRef> {
    let apps: std::collections::HashMap<u32, String> =
        processes().into_iter().map(|p| (p.pid, p.exe)).collect();
    let mut ctx = WindowCollect { apps: &apps, out: Vec::new() };
    unsafe {
        let _ = EnumWindows(Some(collect_switchable), LPARAM(&mut ctx as *mut WindowCollect as isize));
    }
    ctx.out
}

/// The window in front, if it is one a person could be looking at.
pub fn foreground_window() -> Option<WindowRef> {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_invalid() || hwnd == unsafe { GetShellWindow() } {
        return None;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    let exe = processes().into_iter().find(|p| p.pid == pid).map(|p| p.exe).unwrap_or_default();
    let title = window_title(hwnd);
    if title.trim().is_empty() {
        return None;
    }
    Some(WindowRef { hwnd, pid, title, app: window_app_name(&exe) })
}

/// `chrome.exe` → "Chrome" for speaking, leaving an already-pretty name alone.
fn window_app_name(exe: &str) -> String {
    match exe.find('.') {
        Some(at) if at > 0 => {
            let stem = &exe[..at];
            let mut c = stem.chars();
            match c.next() {
                Some(f) => f.to_uppercase().chain(c).collect(),
                None => String::new(),
            }
        }
        _ => exe.to_string(),
    }
}

/// Bring a window to the front and give it the keyboard.
///
/// Windows only honours `SetForegroundWindow` from the process that already has
/// the foreground, which is why a background assistant that calls it and
/// nothing else looks broken. Attaching to the foreground thread's input queue
/// for the duration of the call is the documented way around it.
pub fn focus_window(hwnd: HWND) -> bool {
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let fg = GetForegroundWindow();
        let mut fg_pid = 0u32;
        let fg_thread = if fg.is_invalid() { 0 } else { GetWindowThreadProcessId(fg, Some(&mut fg_pid)) };
        let me = GetCurrentThreadId();
        let attached = fg_thread != 0 && fg_thread != me && AttachThreadInput(me, fg_thread, true).as_bool();
        let mut ok = SetForegroundWindow(hwnd).as_bool();
        if !ok {
            // The other documented route: raising it in the z-order at least
            // makes it the window the next click lands on.
            ok = BringWindowToTop(hwnd).is_ok();
        }
        if attached {
            let _ = AttachThreadInput(me, fg_thread, false);
        }
        ok
    }
}

/// The three things a title bar's buttons do.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WindowState {
    Minimize,
    Maximize,
    Restore,
}

pub fn set_window_state(hwnd: HWND, state: WindowState) -> bool {
    let command = match state {
        WindowState::Minimize => SW_MINIMIZE,
        WindowState::Maximize => SW_MAXIMIZE,
        WindowState::Restore => SW_RESTORE,
    };
    unsafe { ShowWindow(hwnd, command).as_bool() }
}

/// The area of each display, and the part of it not covered by the taskbar.
#[derive(Clone, Copy)]
pub struct Monitor {
    pub rect: Rect,
    pub work: Rect,
    pub primary: bool,
}

/// A plain integer rectangle, so nothing outside this module has to know about
/// Win32's `RECT`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn width(&self) -> i32 {
        self.right - self.left
    }
    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
}

struct MonitorCollect {
    out: Vec<Monitor>,
}

unsafe extern "system" fn collect_monitor(
    hmon: HMONITOR,
    _hdc: HDC,
    _rect: *mut windows::Win32::Foundation::RECT,
    lp: LPARAM,
) -> windows::core::BOOL {
    let ctx = &mut *(lp.0 as *mut MonitorCollect);
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    if GetMonitorInfoW(hmon, &mut info).as_bool() {
        let r = |r: windows::Win32::Foundation::RECT| Rect {
            left: r.left,
            top: r.top,
            right: r.right,
            bottom: r.bottom,
        };
        ctx.out.push(Monitor { rect: r(info.rcMonitor), work: r(info.rcWork), primary: info.dwFlags & 1 != 0 });
    }
    true.into()
}

/// Every display, left to right, which is how people number them.
pub fn monitors() -> Vec<Monitor> {
    let mut ctx = MonitorCollect { out: Vec::new() };
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(collect_monitor), LPARAM(&mut ctx as *mut MonitorCollect as isize));
    }
    ctx.out.sort_by_key(|m| (m.rect.left, m.rect.top));
    ctx.out
}

/// Where a window is, and which display it is mostly on.
fn window_geometry(hwnd: HWND) -> Option<(Rect, usize)> {
    use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;
    let mut r = windows::Win32::Foundation::RECT::default();
    unsafe { GetWindowRect(hwnd, &mut r) }.ok()?;
    let rect = Rect { left: r.left, top: r.top, right: r.right, bottom: r.bottom };
    let hmon = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    let mut here = None;
    if unsafe { GetMonitorInfoW(hmon, &mut info) }.as_bool() {
        let hull = Rect {
            left: info.rcMonitor.left,
            top: info.rcMonitor.top,
            right: info.rcMonitor.right,
            bottom: info.rcMonitor.bottom,
        };
        here = monitors().iter().position(|m| m.rect == hull);
    }
    Some((rect, here.unwrap_or(0)))
}

/// Put a window in half or a quarter of its display, the way dragging it to the
/// edge does.
pub fn snap_window(hwnd: HWND, side: &str) -> Result<Rect, String> {
    let (_, index) = window_geometry(hwnd).ok_or("couldn't read where that window is")?;
    let all = monitors();
    let monitor = all.get(index).or_else(|| all.first()).ok_or("couldn't find the display")?;
    let work = monitor.work;
    let (x, y, w, h) = match side {
        "left" => (work.left, work.top, work.width() / 2, work.height()),
        "right" => (work.left + work.width() / 2, work.top, work.width() / 2, work.height()),
        "top" => (work.left, work.top, work.width(), work.height() / 2),
        "bottom" => (work.left, work.top + work.height() / 2, work.width(), work.height() / 2),
        // "full" is maximise, but without the maximised state sticking around.
        _ => (work.left, work.top, work.width(), work.height()),
    };
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        SetWindowPos(hwnd, Some(HWND_TOP), x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE)
            .map_err(|e| format!("Windows wouldn't move the window: {e}"))?;
    }
    Ok(Rect { left: x, top: y, right: x + w, bottom: y + h })
}

/// Send a window to another display, keeping its size and its relative place.
///
/// `which` accepts "other", "next", "previous", "primary", "left", "right" and
/// 1-based numbers, because all of those are things people say.
pub fn move_window_to_monitor(hwnd: HWND, which: &str) -> Result<(String, Rect), String> {
    let all = monitors();
    if all.len() < 2 {
        return Err("this PC only has one screen attached".into());
    }
    let (rect, current) = window_geometry(hwnd).ok_or("couldn't read where that window is")?;
    let w = which.trim().to_lowercase();
    let primary = all.iter().position(|m| m.primary).unwrap_or(0);
    let target = match w.as_str() {
        "" | "other" | "another" | "the other" => (current + 1) % all.len(),
        "next" => (current + 1) % all.len(),
        "previous" | "prev" | "last" | "back" => (current + all.len() - 1) % all.len(),
        "primary" | "main" | "first" | "one" | "1" => primary,
        "left" => all.iter().enumerate().rev().find(|(_, m)| m.rect.left < all[current].rect.left).map(|(i, _)| i).unwrap_or(current),
        "right" => all.iter().enumerate().find(|(_, m)| m.rect.left > all[current].rect.left).map(|(i, _)| i).unwrap_or(current),
        other => {
            let digits: String = other.chars().filter(|c| c.is_ascii_digit()).collect();
            match digits.parse::<usize>() {
                Ok(n) if (1..=all.len()).contains(&n) => n - 1,
                _ => (current + 1) % all.len(),
            }
        }
    };
    if target == current {
        return Err("it's already on that screen".into());
    }
    let dest = all[target].work;
    // Same size, same relative position, pulled inside the destination.
    let x = dest.left + (rect.left - all[current].work.left).clamp(0, (dest.width() - rect.width()).max(0));
    let y = dest.top + (rect.top - all[current].work.top).clamp(0, (dest.height() - rect.height()).max(0));
    unsafe {
        SetWindowPos(hwnd, Some(HWND_TOP), x, y, rect.width(), rect.height(), SWP_NOZORDER | SWP_NOACTIVATE)
            .map_err(|e| format!("Windows wouldn't move the window: {e}"))?;
    }
    Ok((format!("screen {}", target + 1), Rect { left: x, top: y, right: x + rect.width(), bottom: y + rect.height() }))
}

// ── Clipboard and typing ─────────────────────────────────────────────────

/// Open the clipboard, waiting briefly for whoever has it.
///
/// Another process holding the clipboard is completely normal (a password
/// manager, an Office app), and it is transient — but a single failed attempt
/// reported "couldn't copy" for something that would have worked 20 ms later.
fn clipboard_open() -> Result<(), String> {
    let mut last = String::new();
    for _ in 0..12 {
        match unsafe { OpenClipboard(None) } {
            Ok(()) => return Ok(()),
            Err(e) => {
                last = e.message();
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
        }
    }
    Err(format!("the clipboard is busy — {last}"))
}

pub fn clipboard_text() -> Result<String, String> {
    clipboard_open()?;
    let result = (|| -> Result<String, String> {
        let handle = unsafe { GetClipboardData(CF_UNICODETEXT) }
            .map_err(|_| "there's no text on the clipboard".to_string())?;
        let global = HGLOBAL(handle.0);
        let size = unsafe { GlobalSize(global) };
        let ptr = unsafe { GlobalLock(global) };
        if ptr.is_null() || size < 2 {
            unsafe { let _ = GlobalUnlock(global); };
            return Err("there's no text on the clipboard".into());
        }
        let units = size / 2;
        let words = unsafe { std::slice::from_raw_parts(ptr as *const u16, units) };
        let len = words.iter().position(|&c| c == 0).unwrap_or(units);
        let text = String::from_utf16_lossy(&words[..len]);
        unsafe { let _ = GlobalUnlock(global); };
        Ok(text)
    })();
    unsafe { let _ = CloseClipboard(); };
    result
}

/// Copy a block into the clipboard, which then owns it.
fn put_clipboard(format: u32, bytes: &[u8]) -> Result<(), String> {
    clipboard_open()?;
    let result = (|| -> Result<(), String> {
        unsafe { EmptyClipboard() }.map_err(|e| e.message())?;
        let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len()) }.map_err(|e| e.message())?;
        let ptr = unsafe { GlobalLock(handle) };
        if ptr.is_null() {
            return Err("couldn't get memory for the clipboard".into());
        }
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len());
            let _ = GlobalUnlock(handle);
        }
        unsafe { SetClipboardData(format, Some(HANDLE(handle.0))) }.map_err(|e| e.message())?;
        Ok(())
    })();
    unsafe { let _ = CloseClipboard(); };
    result
}

pub fn set_clipboard_text(text: &str) -> Result<(), String> {
    let mut utf16: Vec<u16> = text.encode_utf16().collect();
    utf16.push(0);
    let bytes: Vec<u8> = utf16.iter().flat_map(|u| u.to_le_bytes()).collect();
    put_clipboard(CF_UNICODETEXT, &bytes)
}

/// Put a bitmap on the clipboard as `CF_DIB`, which is what every app's Paste
/// understands. The rows are bottom-up and the pixels are BGRA — the two things
/// that make a hand-built DIB come out upside down or colour-swapped.
pub fn set_clipboard_image(width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err("there's nothing to copy".into());
    }
    let stride = width as usize * 4;
    let need = stride * height as usize;
    if rgba.len() < need {
        return Err("the image was the wrong size".into());
    }
    let header = 40usize;
    let mut bytes = vec![0u8; header + need];
    fn put_u32(buf: &mut [u8], at: usize, value: u32) {
        buf[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn put_u16(buf: &mut [u8], at: usize, value: u16) {
        buf[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }
    put_u32(&mut bytes, 0, header as u32); // biSize
    put_u32(&mut bytes, 4, width); // biWidth
    put_u32(&mut bytes, 8, height); // biHeight: positive = bottom-up, which is what DIB means
    put_u16(&mut bytes, 12, 1); // biPlanes
    put_u16(&mut bytes, 14, 32); // biBitCount
    put_u32(&mut bytes, 16, 0); // BI_RGB
    put_u32(&mut bytes, 20, need as u32); // biSizeImage
    for row in 0..height as usize {
        let src = (height as usize - 1 - row) * stride;
        let dst = header + row * stride;
        for x in 0..width as usize {
            let s = src + x * 4;
            let d = dst + x * 4;
            bytes[d] = rgba[s + 2]; // B
            bytes[d + 1] = rgba[s + 1]; // G
            bytes[d + 2] = rgba[s]; // R
            bytes[d + 3] = 255;
        }
    }
    put_clipboard(CF_DIB, &bytes)
}

/// Type text into whatever window has the keyboard, as real keystrokes.
///
/// `KEYEVENTF_UNICODE` sends the characters themselves rather than translating
/// virtual keys, so it works for any alphabet and does not fire shortcuts.
pub fn type_unicode(text: &str) -> Result<usize, String> {
    const CHUNK: usize = 32;
    let units: Vec<u16> = text.encode_utf16().collect();
    if units.is_empty() {
        return Ok(0);
    }
    let mut sent = 0usize;
    for chunk in units.chunks(CHUNK) {
        let mut events: Vec<INPUT> = Vec::with_capacity(chunk.len() * 2);
        for unit in chunk {
            for up in [false, true] {
                events.push(INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: VIRTUAL_KEY(0),
                            wScan: *unit,
                            dwFlags: KEYEVENTF_UNICODE
                                | if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                            time: 0,
                            dwExtraInfo: 0,
                        },
                    },
                });
            }
        }
        let accepted = unsafe { SendInput(&events, std::mem::size_of::<INPUT>() as i32) } as usize;
        if accepted == 0 {
            return Err("Windows wouldn't take the keystrokes (is the screen locked?)".into());
        }
        sent += accepted / 2;
        std::thread::sleep(std::time::Duration::from_millis(4));
    }
    Ok(sent)
}

// ── Files, folders and the recycle bin ───────────────────────────────────

/// Move files to the Recycle Bin — recoverable, which is the only kind of
/// delete a voice command should ever do.
pub fn recycle(paths: &[std::path::PathBuf]) -> Result<(), String> {
    if paths.is_empty() {
        return Err("there's nothing to delete".into());
    }
    let mut list: Vec<u16> = Vec::new();
    for p in paths {
        list.extend(p.to_string_lossy().encode_utf16());
        list.push(0);
    }
    list.push(0); // an extra NUL ends the list
    let mut op = SHFILEOPSTRUCTW {
        hwnd: HWND::default(),
        wFunc: FO_DELETE,
        pFrom: PCWSTR(list.as_mut_ptr()),
        pTo: PCWSTR::null(),
        // FOF_ALLOWUNDO (0x40) makes this the recycle bin rather than a real
        // delete; FOF_NOCONFIRMATION (0x10), FOF_SILENT (0x4) and
        // FOF_NOERRORUI (0x400) keep it quiet, so nothing has to be clicked.
        fFlags: 0x40 | 0x10 | 0x4 | 0x400,
        fAnyOperationsAborted: Default::default(),
        hNameMappings: std::ptr::null_mut(),
        lpszProgressTitle: PCWSTR::null(),
    };
    let code = unsafe { SHFileOperationW(&mut op) };
    if code != 0 {
        return Err(match code {
            1223 => "the delete was cancelled".to_string(),
            2 => "I couldn't find it".to_string(),
            5 => "Windows wouldn't let me touch that file".to_string(),
            _ => format!("Windows wouldn't delete it (error {code})"),
        });
    }
    if op.fAnyOperationsAborted.as_bool() {
        return Err("the delete was cancelled".into());
    }
    Ok(())
}

pub fn empty_recycle_bin() -> Result<(), String> {
    unsafe {
        SHEmptyRecycleBinW(
            None,
            PCWSTR::null(),
            SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND,
        )
    }
    .map_err(|e| format!("Windows wouldn't empty the recycle bin: {e}"))
}

/// One drive or volume that is actually connected.
pub struct Drive {
    /// "C:" — what people say.
    pub name: String,
    pub total: u64,
    pub free: u64,
}

/// Fixed, removable and network drives, in letter order. Anything that cannot
/// report its free space (an empty DVD drive, a disconnected share) is left out
/// rather than listed as zero.
pub fn drives() -> Vec<Drive> {
    let mask = unsafe { GetLogicalDrives() };
    let mut out = Vec::new();
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let letter = (b'A' + i as u8) as char;
        let root = format!("{letter}:\\");
        let wide = self::wide(&root);
        let mut free = 0u64;
        let mut total = 0u64;
        let ok = unsafe {
            GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), None, Some(&mut total), Some(&mut free))
        };
        if ok.is_ok() && total > 0 {
            out.push(Drive { name: format!("{letter}:"), total, free });
        }
    }
    out
}

/// Every file under `root` whose name matches, newest first. Bounded in depth
/// and count so an accidental search from the drive root still answers quickly.
pub fn find_named_files(root: &std::path::Path, want: &str, limit: usize) -> Vec<std::path::PathBuf> {
    let mut found: Vec<(std::time::SystemTime, std::path::PathBuf)> = Vec::new();
    let needle = crate::fuzzy::squash(want);
    if needle.is_empty() {
        return Vec::new();
    }
    let mut queue: Vec<(std::path::PathBuf, u32)> = vec![(root.to_path_buf(), 0)];
    let mut visited = 0usize;
    while let Some((dir, depth)) = queue.pop() {
        if depth > 6 || visited > 20_000 || found.len() >= limit * 8 {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            visited += 1;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if !is_dir {
                let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(&name);
                if fuzzy_name_match(&crate::fuzzy::squash(stem), &needle) {
                    let modified = entry
                        .metadata()
                        .and_then(|m| m.modified())
                        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                    found.push((modified, path));
                }
            } else if depth < 6 {
                queue.push((path, depth + 1));
            }
            if visited > 20_000 {
                break;
            }
        }
    }
    found.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    found.into_iter().take(limit).map(|(_, p)| p).collect()
}

/// Does a squashed file stem match what was asked for? "payslip", "pay slip"
/// and "my payslip" all have to find `Payslip-July.pdf`.
pub fn fuzzy_name_match(stem: &str, needle: &str) -> bool {
    if stem == needle {
        return true;
    }
    if stem.contains(needle) || needle.contains(stem) && stem.len() >= 3 {
        return true;
    }
    // One typo or a plural is still a match; a different word is not.
    if needle.len() >= 4 && crate::fuzzy::similarity(stem, needle) >= 0.82 {
        return true;
    }
    false
}

// ── Power and the machine's own numbers ──────────────────────────────────

pub struct Battery {
    /// `None` when Windows will not say (a desktop, usually).
    pub percent: Option<u8>,
    pub charging: bool,
    pub on_ac: bool,
    pub seconds_left: Option<u32>,
}

pub fn battery() -> Option<Battery> {
    let mut status = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut status) }.ok()?;
    // 255 means "unknown", which is what a mains-powered desktop reports.
    let percent = (status.BatteryLifePercent != 255).then_some(status.BatteryLifePercent);
    let seconds_left = if status.BatteryLifeTime != u32::MAX && status.BatteryLifeTime > 0 {
        Some(status.BatteryLifeTime)
    } else {
        None
    };
    Some(Battery {
        percent,
        charging: status.BatteryFlag & 8 != 0,
        on_ac: status.ACLineStatus == 1,
        seconds_left,
    })
}

/// (total, available) physical memory, in bytes.
pub fn memory() -> Option<(u64, u64)> {
    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    unsafe { GlobalMemoryStatusEx(&mut status) }.ok()?;
    Some((status.ullTotalPhys, status.ullAvailPhys))
}

fn filetime_ticks(ft: FILETIME) -> u64 {
    ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64
}

/// Processor load over a short window, 0–100.
///
/// The kernel's own busy counter cannot be sampled instantly — a CPU is either
/// idle or not at any one moment — so this measures across `window` instead,
/// which is why it is only ever asked for by an explicit question.
pub fn cpu_usage(window: std::time::Duration) -> Option<f32> {
    fn sample() -> Option<(u64, u64, u64)> {
        let (mut idle, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
        unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) }.ok()?;
        Some((filetime_ticks(idle), filetime_ticks(kernel), filetime_ticks(user)))
    }
    let (i0, k0, u0) = sample()?;
    std::thread::sleep(window);
    let (i1, k1, u1) = sample()?;
    let idle = i1.saturating_sub(i0) as f64;
    let total = (k1.saturating_sub(k0) + u1.saturating_sub(u0)) as f64;
    if total <= 0.0 {
        return None;
    }
    Some((((total - idle) / total) * 100.0).clamp(0.0, 100.0) as f32)
}

/// How long Windows has been up.
pub fn uptime() -> std::time::Duration {
    use windows::Win32::System::SystemInformation::GetTickCount64;
    std::time::Duration::from_millis(unsafe { GetTickCount64() })
}

/// One process's share of the machine, summed per application.
pub struct AppUsage {
    pub name: String,
    pub megabytes: u64,
    pub cpu_percent: f32,
}

/// The applications using the most memory, or the most processor.
///
/// Per application, not per process: a browser is twenty processes and a
/// list of them tells nobody anything. CPU is measured across a short window,
/// which is the only way to get a number that means something.
pub fn top_apps(by_cpu: bool, count: usize) -> Vec<AppUsage> {
    use std::collections::HashMap;
    let procs = processes();
    let mut memory_by: HashMap<String, u64> = HashMap::new();
    let mut cpu_by: HashMap<String, f32> = HashMap::new();
    let mut first: HashMap<u32, (u64, u64)> = HashMap::new();
    let cores = std::thread::available_parallelism().map(|n| n.get() as f32).unwrap_or(1.0);

    let open = |pid: u32| unsafe {
        OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()
    };
    // First pass: memory, and the starting CPU counters.
    for p in &procs {
        let Some(handle) = open(p.pid) else { continue };
        let mut counters = PROCESS_MEMORY_COUNTERS {
            cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            ..Default::default()
        };
        let used = unsafe { GetProcessMemoryInfo(handle, &mut counters, counters.cb) }
            .map(|_| counters.WorkingSetSize as u64)
            .unwrap_or(0);
        *memory_by.entry(p.exe.clone()).or_default() += used;
        if by_cpu {
            let (mut c, mut e, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
            if unsafe { GetProcessTimes(handle, &mut c, &mut e, &mut k, &mut u) }.is_ok() {
                first.insert(p.pid, (filetime_ticks(k), filetime_ticks(u)));
            }
        }
        unsafe { let _ = CloseHandle(handle); };
    }

    if by_cpu {
        std::thread::sleep(std::time::Duration::from_millis(320));
        for p in &procs {
            let Some(&(k0, u0)) = first.get(&p.pid) else { continue };
            let Some(handle) = open(p.pid) else { continue };
            let (mut c, mut e, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
            if unsafe { GetProcessTimes(handle, &mut c, &mut e, &mut k, &mut u) }.is_ok() {
                let used = filetime_ticks(k).saturating_sub(k0) + filetime_ticks(u).saturating_sub(u0);
                // 100 ns units over the window, as a share of all the cores.
                let share = used as f32 / 10_000.0 / 320.0 / cores * 100.0;
                *cpu_by.entry(p.exe.clone()).or_default() += share;
            }
            unsafe { let _ = CloseHandle(handle); };
        }
    }

    let mut out: Vec<AppUsage> = memory_by
        .iter()
        .map(|(name, &bytes)| AppUsage {
            name: window_app_name(name),
            megabytes: bytes / (1024 * 1024),
            cpu_percent: cpu_by.get(name).copied().unwrap_or(0.0),
        })
        .filter(|a| a.megabytes >= 8 || a.cpu_percent >= 0.4)
        .collect();
    if by_cpu {
        out.sort_by(|a, b| b.cpu_percent.partial_cmp(&a.cpu_percent).unwrap_or(std::cmp::Ordering::Equal));
    } else {
        out.sort_by_key(|a| std::cmp::Reverse(a.megabytes));
    }
    out.truncate(count);
    out
}

/// The processor's marketing name, from the registry.
pub fn cpu_name() -> String {
    reg_get_string(
        windows::Win32::System::Registry::HKEY_LOCAL_MACHINE,
        r"HARDWARE\DESCRIPTION\System\CentralProcessor\0",
        Some("ProcessorNameString"),
    )
    .map(|s| s.trim().to_string())
    .unwrap_or_default()
}

pub fn logical_processors() -> u32 {
    std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(1)
}

/// "Windows 11 24H2 (build 26100)" — the product name in the registry still says
/// "Windows 10" on Windows 11, so the build number decides.
pub fn windows_version() -> String {
    use windows::Win32::System::Registry::HKEY_LOCAL_MACHINE;
    const KEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
    let product = reg_get_string(HKEY_LOCAL_MACHINE, KEY, Some("ProductName")).unwrap_or_default();
    let build = reg_get_string(HKEY_LOCAL_MACHINE, KEY, Some("CurrentBuildNumber")).unwrap_or_default();
    let edition = reg_get_string(HKEY_LOCAL_MACHINE, KEY, Some("DisplayVersion")).unwrap_or_default();
    let name = match build.parse::<u32>() {
        Ok(n) if n >= 22000 => "Windows 11".to_string(),
        _ => product,
    };
    let mut out = name;
    if !edition.is_empty() {
        out.push(' ');
        out.push_str(&edition);
    }
    if !build.is_empty() {
        out.push_str(&format!(" (build {build})"));
    }
    out.trim().to_string()
}

pub fn computer_name() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_default()
}

pub fn user_name() -> String {
    std::env::var("USERNAME").unwrap_or_default()
}

/// Turn on a privilege this process does not hold by default.
fn grant_privilege(name: PCWSTR) -> bool {
    use windows::Win32::Security::LUID_AND_ATTRIBUTES;
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut luid = windows::Win32::Foundation::LUID::default();
        let looked_up = LookupPrivilegeValueW(None, name, &mut luid).is_ok();
        let change = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES { Luid: luid, Attributes: SE_PRIVILEGE_ENABLED }],
        };
        let adjusted = looked_up && AdjustTokenPrivileges(token, false, Some(&change), 0, None, None).is_ok();
        // AdjustTokenPrivileges succeeds even when it assigned nothing; the
        // real answer is the last error, which is ERROR_NOT_ALL_ASSIGNED (1300)
        // when the account is not allowed the privilege at all.
        let granted = adjusted && windows::Win32::Foundation::GetLastError().0 != 1300;
        let _ = CloseHandle(token);
        granted
    }
}

/// Is the workstation locked right now?
///
/// The documented way to ask: the *input* desktop can only be opened while the
/// user is at it, so failing to open it means the lock screen (or a UAC prompt's
/// desktop) has it instead. No polling of windows, no title matching.
pub fn screen_locked() -> bool {
    use windows::Win32::System::StationsAndDesktops::{CloseDesktop, OpenInputDesktop, DESKTOP_CONTROL_FLAGS, DESKTOP_READOBJECTS};
    unsafe {
        match OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_READOBJECTS) {
            Ok(desktop) => {
                let _ = CloseDesktop(desktop);
                false
            }
            Err(_) => true,
        }
    }
}

/// How long since the last keyboard or mouse input.
pub fn idle_time() -> std::time::Duration {
    use windows::Win32::System::SystemInformation::GetTickCount64;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    let mut info = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    if unsafe { GetLastInputInfo(&mut info) }.as_bool() {
        let now = unsafe { GetTickCount64() } as u32;
        let idle_ms = now.wrapping_sub(info.dwTime);
        std::time::Duration::from_millis(idle_ms as u64)
    } else {
        std::time::Duration::ZERO
    }
}

/// Put the machine to sleep. Windows requires a privilege for this, which a
/// normal user account holds but has to ask for.
pub fn sleep_pc() -> Result<(), String> {
    grant_privilege(w!("SeShutdownPrivilege"));
    let ok = unsafe { SetSuspendState(false, false, false) };
    if ok {
        Ok(())
    } else {
        Err("Windows wouldn't go to sleep — hibernation may be switched off".into())
    }
}

/// Power the display off without locking or sleeping: back on at any keypress.
pub fn monitor_off() -> Result<(), String> {
    const SC_MONITORPOWER: usize = 0xF170;
    unsafe {
        let hwnd = GetForegroundWindow();
        let target = if hwnd.is_invalid() { GetShellWindow() } else { hwnd };
        PostMessageW(Some(target), WM_SYSCOMMAND, WPARAM(SC_MONITORPOWER), LPARAM(2)).map_err(|e| e.to_string())
    }
}

/// Shut down, restart, sign out or cancel a pending one, with the same 30-second
/// delay and cancel window people get from the Start menu.
pub fn power_off(kind: &str, seconds: u32) -> Result<(), String> {
    let delay = seconds.to_string();
    match kind {
        "shutdown" => run_capture("shutdown.exe", &["/s", "/t", &delay], std::time::Duration::from_secs(20)),
        "restart" => run_capture("shutdown.exe", &["/r", "/t", &delay], std::time::Duration::from_secs(20)),
        "signout" => run_capture("shutdown.exe", &["/l"], std::time::Duration::from_secs(20)),
        "cancel" => run_capture("shutdown.exe", &["/a"], std::time::Duration::from_secs(20)),
        other => Err(format!("I don't know how to \"{other}\"")),
    }
    .map(|_| ())
    .map_err(|e| format!("Windows refused: {e}"))
}



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_names_map() {
        assert_eq!(key_vk("ctrl"), Some(VK_CONTROL));
        assert_eq!(key_vk("WIN"), Some(VK_LWIN));
        assert_eq!(key_vk("esc"), Some(VK_ESCAPE));
        assert_eq!(key_vk("f5"), Some(VIRTUAL_KEY(0x74)));
        assert_eq!(key_vk("f24"), Some(VIRTUAL_KEY(0x87)));
        assert_eq!(key_vk("d"), Some(VIRTUAL_KEY(0x44)));
        assert_eq!(key_vk("play/pause"), Some(VK_MEDIA_PLAY_PAUSE));
        assert_eq!(key_vk("volume up"), Some(VK_VOLUME_UP));
        assert_eq!(key_vk("banana"), None);
        assert_eq!(key_vk("f25"), None);
        assert_eq!(key_vk("ab"), None);
    }

    #[test]
    fn bad_combos_are_refused() {
        assert!(press_keys("").is_err());
        assert!(press_keys("ctrl+nonsense").is_err());
    }

    /// The poll gap only ever grows, and starts short enough to notice a
    /// process that exits at once.
    #[test]
    fn child_polling_starts_quick_and_backs_off() {
        use std::time::Duration;
        assert!(poll_gap(Duration::ZERO) <= Duration::from_millis(2));
        let mut last = Duration::ZERO;
        for ms in [0u64, 10, 60, 300, 2_000, 30_000] {
            let gap = poll_gap(Duration::from_millis(ms));
            assert!(gap >= last, "{ms} ms went backwards");
            assert!(gap <= Duration::from_millis(25), "{ms} ms polled slower than the old flat 25");
            last = gap;
        }
        assert_eq!(poll_gap(Duration::from_secs(60)), Duration::from_millis(25));
    }

    #[test]
    fn media_keys_are_mapped() {
        use crate::brain::MediaKey::*;
        assert_eq!(media_vk(Pause), VK_MEDIA_PLAY_PAUSE);
        assert_eq!(media_vk(Resume), VK_MEDIA_PLAY_PAUSE);
        assert_eq!(media_vk(Next), VK_MEDIA_NEXT_TRACK);
        assert_eq!(media_vk(Previous), VK_MEDIA_PREV_TRACK);
        assert_eq!(media_vk(Stop), VK_MEDIA_STOP);
    }

    /// F24 is the polite test key: nothing on a normal desktop reacts to it.
    #[test]
    #[ignore]
    fn send_input_works() {
        assert!(tap_key(VIRTUAL_KEY(0x87)), "SendInput refused the key press");
        assert!(press_keys("ctrl+f24").is_ok());
    }

    /// Reads the master volume and puts it straight back, so nothing changes.
    #[test]
    #[ignore]
    fn volume_com_round_trips() {
        let before = volume_percent().expect("no default audio endpoint");
        set_volume_percent(before).expect("could not set the volume");
        let after = volume_percent().unwrap();
        assert!(after.abs_diff(before) <= 1, "{before} -> {after}");
        let _ = muted();
        println!("master volume {before}% (unchanged)");
    }

    /// Captures the screen, then reads the PNG back to prove it is a real image:
    /// the right size (which catches a capture surface that never got created),
    /// fully opaque (which catches the alpha byte GDI leaves at zero) and not
    /// one flat colour (which catches a blit that did not happen).
    #[test]
    #[ignore]
    fn screenshot_is_a_png() {
        let path = screenshot().expect("screenshot failed");
        let bytes = std::fs::read(&path).unwrap();
        let decoder = png::Decoder::new(std::io::Cursor::new(&bytes));
        let mut reader = decoder.read_info().unwrap();
        let info = reader.info();
        let (w, h) = (info.width, info.height);
        assert_eq!(info.color_type, png::ColorType::Rgba, "a screenshot must be RGBA");
        let expected = unsafe { (GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN)) };
        assert_eq!((w, h), (expected.0 as u32, expected.1 as u32), "size mismatch");
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut buf).unwrap();

        let mut opaque = 0usize;
        let mut colours = std::collections::HashSet::new();
        for px in buf.chunks_exact(4).step_by(4096) {
            if px[3] == 255 {
                opaque += 1;
            }
            colours.insert([px[0], px[1], px[2]]);
        }
        let sampled = buf.len() / 4 / 4096 + 1;
        assert_eq!(opaque, sampled, "the capture came back transparent");
        assert!(colours.len() > 1, "the capture is a single flat colour — the blit did nothing");
        println!("{}: {w}x{h}, {} bytes, {} distinct colours", path.display(), bytes.len(), colours.len());
        let _ = std::fs::remove_file(&path);
    }


#[test]
#[ignore]
fn the_numbers_are_real() {
    println!("memory   {:?}", crate::win::memory());
    println!("drives   {:?}", crate::win::drives().iter().map(|d| (&d.name, d.free, d.total)).collect::<Vec<_>>());
    println!("battery  {:?}", crate::win::battery().map(|b| (b.percent, b.charging, b.on_ac)));
    println!("uptime   {:?}", crate::win::uptime());
    println!("cpu      {:?}", crate::win::cpu_usage(std::time::Duration::from_millis(200)));
    println!("windows  {:?}", crate::win::windows_version());
    println!("cpu name {:?}", crate::win::cpu_name());
    println!("windows! {:?}", crate::win::switchable_windows().iter().take(3).map(|w| (&w.app, &w.title)).collect::<Vec<_>>());
    println!("clipboard {:?}", crate::win::clipboard_text().map(|t| t.chars().take(40).collect::<String>()));
}

    #[test]
    #[ignore]
    fn clock_is_sane() {
        let t = local_time();
        assert!((1900..2200).contains(&t.year), "{t:?}", t = t.year);
        assert!((1..=12).contains(&t.month));
        assert!(t.hour < 24 && t.minute < 60);
    }
}

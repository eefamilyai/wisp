use std::path::PathBuf;

/// `%APPDATA%\Wisp` — config, app cache and logs. Survives reinstalls.
pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join(crate::PRODUCT);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Where the settings used to live, before the product was renamed.
pub fn legacy_data_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(crate::LEGACY_PRODUCT)
}

/// Bring a pre-rename setup across, once.
///
/// The product was called NeedleVoice until 0.3.0, and its settings, custom
/// functions, scripts and schedule all lived in `%APPDATA%\NeedleVoice`. A
/// rename that silently starts from defaults would lose every function someone
/// wrote and every alarm they set, which is not a cosmetic change. Nothing is
/// moved or deleted: the old folder is only read, and only when the new folder
/// has no settings of its own yet.
///
/// Returns the files that were copied, for the log.
pub fn adopt_legacy() -> Vec<PathBuf> {
    let new = data_dir();
    let old = legacy_data_dir();
    if new.join("config.toml").exists() || !old.join("config.toml").exists() {
        return Vec::new();
    }
    // Everything the assistant stores, except the log (a fresh log is more
    // useful than a stale one) and the microphone clips.
    const FILES: [&str; 4] = ["config.toml", "schedule.json", "apps.json", "notes.md"];
    const DIRS: [&str; 1] = ["scripts"];
    let mut copied = Vec::new();
    for name in FILES {
        let from = old.join(name);
        if from.is_file() {
            let to = new.join(name);
            if std::fs::copy(&from, &to).is_ok() {
                copied.push(to);
            }
        }
    }
    for name in DIRS {
        let from = old.join(name);
        if from.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&from) {
                for entry in entries.flatten() {
                    let to = new.join(name).join(entry.file_name());
                    if entry.path().is_file() && std::fs::copy(entry.path(), &to).is_ok() {
                        copied.push(to);
                    }
                }
            }
        }
    }
    if !copied.is_empty() {
        log::info!(
            "adopted {} file(s) from the pre-rename settings folder {}",
            copied.len(),
            old.display()
        );
    }
    copied
}

pub fn config_file() -> PathBuf {
    data_dir().join("config.toml")
}

pub fn apps_cache_file() -> PathBuf {
    data_dir().join("apps.json")
}

pub fn log_file() -> PathBuf {
    data_dir().join("agent.log")
}

/// Directory holding the running executable.
pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Default install location: `%LOCALAPPDATA%\Programs\Wisp` (no admin needed).
pub fn default_install_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("Programs").join(crate::PRODUCT)
}

/// Where model files live. Prefers `<exe dir>\models`; during development
/// also walks up from the exe looking for a `models` folder.
///
/// Resolved once and remembered: the answer cannot change while the process
/// runs, and the settings app asks for it from several pages on every frame,
/// each time walking the directory tree and probing the disk.
pub fn models_dir() -> PathBuf {
    static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(resolve_models_dir).clone()
}

fn resolve_models_dir() -> PathBuf {
    let mut dir = Some(exe_dir());
    while let Some(d) = dir {
        let candidate = d.join("models");
        if candidate.join(crate::NEEDLE_MODEL).exists() {
            return candidate;
        }
        dir = d.parent().map(|p| p.to_path_buf());
    }
    exe_dir().join("models")
}

/// Path of a sibling executable (e.g. the config app next to the agent).
pub fn sibling_exe(name: &str) -> PathBuf {
    exe_dir().join(name)
}

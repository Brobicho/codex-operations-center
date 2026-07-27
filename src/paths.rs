use std::path::PathBuf;

use anyhow::{Context, Result};
use directories::BaseDirs;

pub fn data_dir() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("CODEX_OPS_HOME") {
        return Ok(PathBuf::from(path));
    }
    let base = BaseDirs::new().context("unable to locate the user data directory")?;
    #[cfg(target_os = "linux")]
    let root = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| base.home_dir().to_path_buf())
        .join(".local/share");
    #[cfg(not(target_os = "linux"))]
    let root = base.data_local_dir().to_path_buf();
    Ok(root.join("codex-ops"))
}

pub fn events_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("events.jsonl"))
}

pub fn config_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("config.json"))
}

pub fn hd_terminal_dir() -> Result<PathBuf> {
    Ok(data_dir()?.join("kitty.app"))
}

pub fn hd_terminal_path() -> Result<PathBuf> {
    let root = hd_terminal_dir()?;
    #[cfg(target_os = "macos")]
    return Ok(root.join("Contents/MacOS/kitty"));
    #[cfg(not(target_os = "macos"))]
    Ok(root.join("bin/kitty"))
}

pub fn manifest_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("install-manifest.json"))
}

pub fn launcher_path() -> Result<PathBuf> {
    let base = BaseDirs::new().context("unable to locate the user home directory")?;
    #[cfg(windows)]
    let name = "codex-ops.exe";
    #[cfg(not(windows))]
    let name = "codex-ops";
    Ok(base.home_dir().join(".local/bin").join(name))
}

pub fn codex_home() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("CODEX_HOME") {
        return Ok(PathBuf::from(path));
    }
    let base = BaseDirs::new().context("unable to locate the user home directory")?;
    Ok(base.home_dir().join(".codex"))
}

pub fn hooks_path() -> Result<PathBuf> {
    Ok(codex_home()?.join("hooks.json"))
}

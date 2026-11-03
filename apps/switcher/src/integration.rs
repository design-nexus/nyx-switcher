//! The shortcuts in Hyprland: the block in `~/.config/hypr/hyprland.lua` (written by
//! `nyx_switcher_core::binds`). Only the exact block written last is ever replaced or
//! removed. Hyprland reloads, and a configuration it rejects is rolled back.

use nyx_switcher_core::binds::{self, Conflict};
use nyx_switcher_core::config::Keys;
use std::path::{Path, PathBuf};
use std::process::Command;

fn config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"))
}

fn state_dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"))
        .join("nyx-switcher")
}

fn hypr_file() -> PathBuf {
    config_home().join("hypr/hyprland.lua")
}

fn owned_file() -> PathBuf {
    state_dir().join("hypr-block.lua")
}

fn hyprctl(args: &[&str]) -> Result<String, String> {
    let out = Command::new("hyprctl").args(args).output().map_err(|e| format!("hyprctl: {e}"))?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn reload_ok() -> Result<(), String> {
    hyprctl(&["reload"])?;
    let s = hyprctl(&["configerrors"])?;
    if !s.trim().is_empty() && !s.to_lowercase().contains("no errors") {
        return Err(format!("Hyprland reports: {}", s.trim()));
    }
    Ok(())
}

fn write_atomic(file: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mode = std::fs::metadata(file).ok().map(|m| m.permissions());
    let tmp = file.with_extension(format!("nyx-switcher-{}", std::process::id()));
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    if let Some(mode) = mode {
        std::fs::set_permissions(&tmp, mode).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp, file).map_err(|e| e.to_string())
}

/// Other bindings on `keys` right now (Omarchy's that are replaced on purpose are
/// marked `expected`).
pub fn check(keys: &Keys) -> Result<Vec<Conflict>, String> {
    binds::conflicts(&hyprctl(&["-j", "binds"])?, keys)
}

/// Bind `keys` (None removes the block, giving the keys back to Omarchy). A key that
/// another binding (not one of Omarchy's known ones) uses is refused unless `force`.
pub fn apply(keys: Option<&Keys>, force: bool) -> Result<String, String> {
    if let Some(keys) = keys {
        if let Some(bad) = binds::binds(keys).iter().find(|b| !binds::valid_key(&b.key)) {
            return Err(format!("“{}” isn't a key Hyprland takes (like ALT + TAB)", bad.key));
        }
        let others: Vec<Conflict> = check(keys).unwrap_or_default().into_iter().filter(|c| !c.expected).collect();
        if !others.is_empty() && !force {
            let list: Vec<String> = others.iter().map(|c| format!("{} ({})", c.key, c.other)).collect();
            return Err(format!("already bound: {}. Change the keys in switcher.toml, or run --integrate --force", list.join(", ")));
        }
    }
    let file = hypr_file();
    let original = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let owned = owned_file();
    let old = std::fs::read_to_string(&owned).ok();
    let new = keys.map(binds::block);
    let changed = binds::edit(&original, old.as_deref(), new.as_deref())?;
    if changed == original {
        return Ok(String::new());
    }
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    write_atomic(&state_dir().join(format!("hyprland-{stamp}.lua")), &original)?;
    write_atomic(&file, &changed)?;
    if let Err(e) = reload_ok() {
        write_atomic(&file, &original)?;
        let _ = reload_ok();
        return Err(format!("{e} (the change was undone)"));
    }
    match new {
        Some(n) => write_atomic(&owned, &n)?,
        None => {
            let _ = std::fs::remove_file(owned);
        }
    }
    Ok(match keys {
        Some(k) => binds::binds(k).iter().map(|b| format!("{}: {}", b.key, b.description)).collect::<Vec<_>>().join("\n"),
        None => "The shortcuts are Omarchy's again.".into(),
    })
}

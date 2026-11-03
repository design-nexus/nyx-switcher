//! `~/.config/nyx/switcher.toml`. Every key is optional; a missing or broken file means
//! the defaults (a broken one is reported, never overwritten).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Which windows the switcher lists.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// Every workspace on every monitor.
    #[default]
    All,
    /// The monitor with focus (all its workspaces).
    Monitor,
    /// The workspace in front of the user on the focused monitor.
    Workspace,
}

impl Scope {
    pub fn parse(s: &str) -> Option<Scope> {
        match s {
            "all" => Some(Scope::All),
            "monitor" => Some(Scope::Monitor),
            "workspace" => Some(Scope::Workspace),
            _ => None,
        }
    }
}

/// The shortcuts, as Omarchy writes keys (`ALT + TAB`). Empty leaves one unbound.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Keys {
    pub windows: String,
    pub windows_reverse: String,
    pub apps: String,
    pub apps_reverse: String,
    pub same_app: String,
    pub same_app_reverse: String,
    pub overview: String,
}

impl Default for Keys {
    fn default() -> Keys {
        Keys {
            windows: "ALT + TAB".into(),
            windows_reverse: "ALT + SHIFT + TAB".into(),
            apps: "SUPER + TAB".into(),
            apps_reverse: "SUPER + SHIFT + TAB".into(),
            same_app: "ALT + grave".into(),
            same_app_reverse: "ALT + SHIFT + grave".into(),
            overview: "SUPER + ALT + W".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub scope: Scope,
    /// List windows on agent workspaces (`nyx-agent…`) too.
    pub show_agent_workspaces: bool,
    /// Window thumbnails in the window switcher.
    pub thumbnails: bool,
    /// How long the shortcut has to be held before the switcher appears; a quicker tap
    /// switches to the previous window without showing anything.
    pub show_delay_ms: u32,
    /// Fade the switcher in and out.
    pub animation: bool,
    pub keys: Keys,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            scope: Scope::All,
            show_agent_workspaces: false,
            thumbnails: true,
            show_delay_ms: 80,
            animation: true,
            keys: Keys::default(),
        }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"));
        base.join("nyx/switcher.toml")
    }

    pub fn parse(text: &str) -> Result<Config, String> {
        let mut c: Config = toml::from_str(text).map_err(|e| e.to_string())?;
        c.show_delay_ms = c.show_delay_ms.min(1000);
        Ok(c)
    }

    /// The settings on disk, or the defaults (a broken file is reported on stderr).
    pub fn load() -> Config {
        let path = Config::path();
        match std::fs::read_to_string(&path) {
            Ok(text) => Config::parse(&text).unwrap_or_else(|e| {
                eprintln!("nyx-switcher: {}: {e}; using the defaults", path.display());
                Config::default()
            }),
            Err(_) => Config::default(),
        }
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_is_the_defaults() {
        assert_eq!(Config::parse("").unwrap(), Config::default());
    }

    #[test]
    fn partial_file_keeps_other_defaults() {
        let c = Config::parse("scope = \"workspace\"\n[keys]\napps = \"\"\n").unwrap();
        assert_eq!(c.scope, Scope::Workspace);
        assert_eq!(c.keys.apps, "");
        assert_eq!(c.keys.windows, "ALT + TAB");
        assert!(c.thumbnails);
    }

    #[test]
    fn round_trips_and_clamps() {
        let c = Config { scope: Scope::Monitor, show_delay_ms: 70, ..Default::default() };
        assert_eq!(Config::parse(&c.to_toml()).unwrap(), c);
        assert_eq!(Config::parse("show_delay_ms = 99999").unwrap().show_delay_ms, 1000);
        assert!(Config::parse("scope = \"galaxy\"").is_err());
    }
}

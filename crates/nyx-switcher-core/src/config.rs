//! The `switcher` domain of NyxOS's configuration layers: NyxOS's defaults
//! (`/etc/nyxos/defaults/switcher.toml`), an administrator's, then the user's
//! `~/.config/nyx/switcher.toml` (Settings writes it). Every key is optional; a missing
//! layer adds nothing, a key of the wrong type is left at its default, and a user file
//! that isn't TOML is kept aside as `switcher.toml.broken-<time>` and reported.

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

/// How large window thumbnails are.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Size {
    Small,
    #[default]
    Medium,
    Large,
}

impl Size {
    /// Scale for the medium size.
    pub fn factor(self) -> f64 {
        match self {
            Size::Small => 0.78,
            Size::Medium => 1.0,
            Size::Large => 1.3,
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
    /// Show each Hyprland group (tabs) once in the window switcher; ↓ lists its windows.
    pub collapse_groups: bool,
    pub thumbnail_size: Size,
    /// A touchpad swipe that opens the overview: `"4 up"`, `"3 down"`; empty for none.
    pub gesture: String,
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
            collapse_groups: true,
            thumbnail_size: Size::Medium,
            gesture: String::new(),
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

    /// The settings through every layer (problems are reported on stderr).
    pub fn load() -> Config {
        Config::load_from(&store())
    }

    pub fn load_from(store: &nyx_config::Store) -> Config {
        let loaded = store.load();
        let (mut c, problems) = nyx_config::parse::<Config>(&loaded);
        for p in problems.iter().chain(loaded.problems.iter().map(|p| &p.message)) {
            eprintln!("nyx-switcher: {p}");
        }
        c.show_delay_ms = c.show_delay_ms.min(1000);
        c
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }
}

/// The switcher's domain. It never writes its file, so there's nothing to migrate.
pub const DOMAIN: nyx_config::Domain = nyx_config::Domain { name: "switcher", version: 0, migrations: &[] };

pub fn store() -> nyx_config::Store {
    nyx_config::Store::new(DOMAIN, nyx_config::table_of(&Config::default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nyxos_defaults_come_under_the_users_file() {
        let d = tempfile::tempdir().unwrap();
        let layers = nyx_config::Layers { defaults: d.path().join("d"), system: d.path().join("s"), user: d.path().join("u") };
        std::fs::create_dir_all(&layers.defaults).unwrap();
        std::fs::create_dir_all(&layers.user).unwrap();
        std::fs::write(layers.defaults.join("switcher.toml"), "scope = \"monitor\"\nshow_delay_ms = 5000\n").unwrap();
        let store = nyx_config::Store::with_layers(layers.clone(), DOMAIN, nyx_config::table_of(&Config::default()));
        let c = Config::load_from(&store);
        assert_eq!(c.scope, Scope::Monitor);
        assert_eq!(c.show_delay_ms, 1000, "clamped as before");
        std::fs::write(layers.user.join("switcher.toml"), "scope = \"workspace\"\n").unwrap();
        assert_eq!(Config::load_from(&store).scope, Scope::Workspace);
    }

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
        assert!(c.collapse_groups && c.gesture.is_empty());
        assert_eq!(Config::parse("thumbnail_size = \"large\"").unwrap().thumbnail_size, Size::Large);
    }

    #[test]
    fn round_trips_and_clamps() {
        let c = Config { scope: Scope::Monitor, show_delay_ms: 70, ..Default::default() };
        assert_eq!(Config::parse(&c.to_toml()).unwrap(), c);
        assert_eq!(Config::parse("show_delay_ms = 99999").unwrap().show_delay_ms, 1000);
        assert!(Config::parse("scope = \"galaxy\"").is_err());
    }
}

//! What the switcher lists: the windows (or apps) in most-recently-used order, filtered
//! by scope.
//!
//! Windows come from `nyx-desktop`, whose list is already most recent first. Special
//! workspaces (scratchpad, Quake console) are left out except Omarchy's minimized one;
//! agent workspaces (a name starting with `nyx-agent`, with or without `special:`) are
//! left out unless asked for.

use crate::config::Scope;
use nyx_desktop::Desktop;
use nyx_desktop::actions::MINIMIZED;
use nyx_desktop::apps::{Index, key_for};

/// Workspace names starting with this belong to agents, not the user.
pub const AGENT_PREFIX: &str = "nyx-agent";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// One entry per window (Alt+Tab).
    Windows,
    /// One entry per app (Super+Tab).
    Apps,
    /// The windows of the app in front (Alt+`).
    SameApp,
}

impl Mode {
    pub fn parse(s: &str) -> Option<Mode> {
        match s {
            "windows" => Some(Mode::Windows),
            "apps" => Some(Mode::Apps),
            "same-app" => Some(Mode::SameApp),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Mode::Windows => "windows",
            Mode::Apps => "apps",
            Mode::SameApp => "same-app",
        }
    }
}

/// One entry. In app mode it stands for the app's most recent window and lists the
/// others in `windows`.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// Desktop id, or `class:<class>` for a window no installed app claims.
    pub app: String,
    pub address: String,
    pub stable_id: String,
    pub class: String,
    pub title: String,
    pub workspace: i64,
    /// The workspace's name as Hyprland shows it (`3`, `special:minimized`, `web`).
    pub workspace_name: String,
    /// Connector name of the window's monitor.
    pub monitor: String,
    pub minimized: bool,
    pub urgent: bool,
    pub floating: bool,
    /// Every window of the entry, most recent first (one in window modes).
    pub windows: Vec<String>,
}

impl Item {
    /// What identifies the entry across refreshes.
    pub fn id(&self, mode: Mode) -> &str {
        if mode == Mode::Apps { &self.app } else { &self.address }
    }
}

/// Which windows to list.
#[derive(Debug, Clone, Default)]
pub struct Filter {
    pub scope: Scope,
    pub show_agent_workspaces: bool,
    /// Only this app's windows (an app expanded in the app switcher).
    pub app: Option<String>,
}

fn is_agent(name: &str) -> bool {
    name.trim_start_matches("special:").starts_with(AGENT_PREFIX)
}

/// The windows the switcher can go to, most recent first.
pub fn windows(desktop: &Desktop, index: &Index, filter: &Filter) -> Vec<Item> {
    let focused = desktop.focused_monitor();
    let mut out = Vec::new();
    for w in &desktop.windows {
        let ws = desktop.workspaces.iter().find(|ws| ws.id == w.workspace);
        let ws_name = ws.map(|ws| ws.name.clone()).unwrap_or_else(|| w.workspace.to_string());
        let minimized = ws_name == MINIMIZED;
        if is_agent(&ws_name) && !filter.show_agent_workspaces {
            continue;
        }
        if ws_name.starts_with("special:") && !minimized && !is_agent(&ws_name) {
            continue;
        }
        let monitor = desktop.monitors.iter().find(|m| m.id == w.monitor);
        if !minimized {
            match (filter.scope, focused) {
                (Scope::Monitor, Some(f)) if f.id != w.monitor => continue,
                (Scope::Workspace, Some(f)) if f.active_workspace != w.workspace => continue,
                _ => {}
            }
        }
        let app = key_for(index, &w.class, &w.initial_class);
        if filter.app.as_ref().is_some_and(|a| a != &app) {
            continue;
        }
        out.push(Item {
            app,
            address: w.address.clone(),
            stable_id: w.stable_id.clone(),
            class: if w.class.is_empty() { w.initial_class.clone() } else { w.class.clone() },
            title: w.title.clone(),
            workspace: w.workspace,
            workspace_name: ws_name,
            monitor: monitor.map(|m| m.name.clone()).unwrap_or_default(),
            minimized,
            urgent: w.urgent,
            floating: w.floating,
            windows: vec![w.address.clone()],
        });
    }
    out
}

/// One entry per app, in the order of each app's most recent window.
pub fn apps(windows: Vec<Item>) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::new();
    for w in windows {
        match out.iter_mut().find(|a| a.app == w.app) {
            Some(a) => {
                a.windows.push(w.address);
                a.urgent |= w.urgent;
            }
            None => out.push(w),
        }
    }
    out
}

/// The entries for `mode`. `SameApp` lists the windows of the focused window's app
/// (empty when no window has focus).
pub fn items(desktop: &Desktop, index: &Index, mode: Mode, filter: &Filter) -> Vec<Item> {
    match mode {
        Mode::Windows => windows(desktop, index, filter),
        Mode::Apps => apps(windows(desktop, index, filter)),
        Mode::SameApp => {
            let Some(active) = desktop.active_window.as_ref().and_then(|a| desktop.window(a)) else {
                return Vec::new();
            };
            let app = key_for(index, &active.class, &active.initial_class);
            windows(desktop, index, &Filter { app: Some(app), ..filter.clone() })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::desktop;
    use nyx_desktop::apps::Entry;

    fn index() -> Index {
        Index::new(&[
            Entry { id: "org.mozilla.firefox".into(), exec: "firefox %u".into(), ..Default::default() },
            Entry { id: "Alacritty".into(), exec: "alacritty".into(), ..Default::default() },
        ])
    }

    fn titles(items: &[Item]) -> Vec<&str> {
        items.iter().map(|i| i.title.as_str()).collect()
    }

    #[test]
    fn most_recent_first_without_special_or_agent_workspaces() {
        let d = desktop();
        let all = windows(&d, &index(), &Filter::default());
        assert_eq!(titles(&all), ["term 1", "web 1", "term 2", "web 2", "minimized notes"]);
        assert!(all[4].minimized);
        let with_agents = windows(&d, &index(), &Filter { show_agent_workspaces: true, ..Default::default() });
        assert!(titles(&with_agents).contains(&"agent browser"));
        assert!(!titles(&with_agents).contains(&"scratch"));
    }

    #[test]
    fn scopes() {
        let d = desktop();
        let ws = windows(&d, &index(), &Filter { scope: Scope::Workspace, ..Default::default() });
        assert_eq!(titles(&ws), ["term 1", "web 1", "minimized notes"], "workspace 1, plus minimized windows");
        let mon = windows(&d, &index(), &Filter { scope: Scope::Monitor, ..Default::default() });
        assert_eq!(titles(&mon), ["term 1", "web 1", "term 2", "minimized notes"], "eDP-1 only");
    }

    #[test]
    fn apps_group_in_order_of_their_latest_window() {
        let d = desktop();
        let a = items(&d, &index(), Mode::Apps, &Filter::default());
        let names: Vec<&str> = a.iter().map(|i| i.app.as_str()).collect();
        assert_eq!(names, ["Alacritty", "org.mozilla.firefox", "class:notes"]);
        assert_eq!(a[0].windows, ["a1", "a3"]);
        assert_eq!(a[0].address, "a1");
    }

    #[test]
    fn same_app_lists_the_focused_apps_windows() {
        let mut d = desktop();
        let s = items(&d, &index(), Mode::SameApp, &Filter::default());
        assert_eq!(titles(&s), ["term 1", "term 2"]);
        d.active_window = None;
        assert!(items(&d, &index(), Mode::SameApp, &Filter::default()).is_empty());
    }
}

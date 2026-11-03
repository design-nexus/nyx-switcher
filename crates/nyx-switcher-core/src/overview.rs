//! The overview: every window, grouped by workspace, and the search that narrows the
//! switcher and the overview.

use crate::model::Item;
use nyx_desktop::Desktop;

/// One workspace's windows.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub workspace: i64,
    /// As Hyprland names it: `3`, `web`, `special:minimized`.
    pub name: String,
    pub monitor: String,
    /// Shown on its monitor right now.
    pub visible: bool,
    pub minimized: bool,
    pub items: Vec<Item>,
}

impl Group {
    /// The heading: `Workspace 3`, `web`, `Minimized`.
    pub fn title(&self) -> String {
        if self.minimized {
            "Minimized".into()
        } else if self.name.parse::<i64>().is_ok() {
            format!("Workspace {}", self.name)
        } else {
            self.name.trim_start_matches("special:").to_string()
        }
    }
}

/// `items` grouped by workspace: the focused monitor's workspaces first, then the other
/// monitors' (each in workspace order), minimized windows last. Within a group the
/// windows keep their order (most recent first).
pub fn groups(desktop: &Desktop, items: Vec<Item>) -> Vec<Group> {
    let focused = desktop.focused_monitor().map(|m| m.name.clone()).unwrap_or_default();
    let mut out: Vec<Group> = Vec::new();
    for item in items {
        match out.iter_mut().find(|g| g.workspace == item.workspace) {
            Some(g) => g.items.push(item),
            None => {
                let visible = desktop.monitors.iter().any(|m| m.active_workspace == item.workspace);
                out.push(Group {
                    workspace: item.workspace,
                    name: item.workspace_name.clone(),
                    monitor: item.monitor.clone(),
                    visible,
                    minimized: item.minimized,
                    items: vec![item],
                });
            }
        }
    }
    let monitor_rank = |g: &Group| -> usize {
        if g.monitor == focused {
            0
        } else {
            1 + desktop.monitors.iter().position(|m| m.name == g.monitor).unwrap_or(usize::MAX - 1)
        }
    };
    out.sort_by_key(|g| (g.minimized, g.workspace < 0, monitor_rank(g), g.workspace));
    for g in &mut out {
        g.items = tabs_together(std::mem::take(&mut g.items));
    }
    out
}

/// The tabs of a group side by side, in group order, where its most recent tab was.
fn tabs_together(items: Vec<Item>) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::with_capacity(items.len());
    let mut rest = items;
    while !rest.is_empty() {
        let first = rest.remove(0);
        if first.group.len() < 2 {
            out.push(first);
            continue;
        }
        let group = first.group.clone();
        let mut members = vec![first];
        rest.retain(|i| {
            if i.group.first() == group.first() {
                members.push(i.clone());
                false
            } else {
                true
            }
        });
        members.sort_by_key(|m| group.iter().position(|a| a == &m.address).unwrap_or(usize::MAX));
        out.extend(members);
    }
    out
}

/// Whether `item` (whose app is called `app_name`) matches every word of `query`, in
/// its title, app name or class, ignoring case. An empty query matches everything.
pub fn matches(item: &Item, app_name: &str, query: &str) -> bool {
    let hay = format!("{} {} {}", item.title, app_name, item.class).to_lowercase();
    query.split_whitespace().all(|w| hay.contains(&w.to_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Filter, Mode, items};
    use crate::testing::desktop;
    use nyx_desktop::apps::Index;

    #[test]
    fn grouped_by_workspace_focused_monitor_first() {
        let d = desktop();
        let all = items(&d, &Index::default(), Mode::Windows, &Filter::default());
        let g = groups(&d, all);
        let names: Vec<String> = g.iter().map(Group::title).collect();
        assert_eq!(names, ["Workspace 1", "Workspace 2", "Workspace 11", "Minimized"]);
        assert_eq!(g[0].items.len(), 2);
        assert!(g[0].visible && !g[1].visible && g[2].visible, "11 is shown on DP-1");
        assert_eq!(g[2].monitor, "DP-1");
    }

    #[test]
    fn tabs_of_a_group_stay_together() {
        let mut d = desktop();
        // Group term 2 (workspace 2) with a new tab that was used least recently.
        let mut extra = crate::testing::window(9, "Alacritty", "term 9", 2, 0);
        extra.focus_order = 99;
        d.windows.push(extra);
        d.windows[2].grouped = vec!["a9".into(), "a3".into()];
        d.windows.last_mut().unwrap().grouped = vec!["a9".into(), "a3".into()];
        let all = items(&d, &Index::default(), Mode::Windows, &Filter::default());
        let g = groups(&d, all);
        let ws2: Vec<&str> = g[1].items.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(ws2, ["term 9", "term 2"], "group order");
    }

    #[test]
    fn search_needs_every_word() {
        let d = desktop();
        let all = items(&d, &Index::default(), Mode::Windows, &Filter::default());
        let web1 = &all[1];
        assert!(matches(web1, "Firefox", ""));
        assert!(matches(web1, "Firefox", "fire WEB"));
        assert!(matches(web1, "Firefox", "1"));
        assert!(!matches(web1, "Firefox", "web term"));
    }
}

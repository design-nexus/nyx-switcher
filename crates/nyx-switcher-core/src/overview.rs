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

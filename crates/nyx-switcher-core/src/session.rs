//! One switching session: from the first press of the shortcut to the release of its
//! modifier (switch), Esc (cancel) or a click.
//!
//! The first press already chooses: the previous window (or app), so a quick tap goes
//! back and forth between two. Pressing again moves on, Shift goes back, and both wrap.

use crate::model::{Item, Mode};

#[derive(Debug, Clone)]
pub struct Session {
    pub mode: Mode,
    pub items: Vec<Item>,
    pub selected: usize,
    /// The app whose windows are shown, when one was expanded from the app switcher.
    pub expanded: Option<String>,
}

impl Session {
    /// Start on `items` (most recent first). `active` is the focused window; when the
    /// first entry is (or holds) it, going forward starts on the second.
    pub fn start(mode: Mode, items: Vec<Item>, active: Option<&str>, forward: bool) -> Option<Session> {
        if items.is_empty() {
            return None;
        }
        let current = active.is_some_and(|a| items[0].windows.iter().any(|w| w == a));
        let n = items.len();
        let selected = match (forward, current) {
            (true, true) => 1 % n,
            (true, false) => 0,
            (false, _) => n - 1,
        };
        Some(Session { mode, items, selected, expanded: None })
    }

    pub fn selected(&self) -> Option<&Item> {
        self.items.get(self.selected)
    }

    /// The next entry (or the one before), wrapping around.
    pub fn step(&mut self, forward: bool) {
        self.move_by(if forward { 1 } else { -1 });
    }

    /// Move by `delta` entries, wrapping around.
    pub fn move_by(&mut self, delta: isize) {
        let n = self.items.len() as isize;
        if n > 0 {
            self.selected = (self.selected as isize + delta).rem_euclid(n) as usize;
        }
    }

    pub fn select(&mut self, index: usize) {
        if index < self.items.len() {
            self.selected = index;
        }
    }

    /// New entries after windows opened, closed or changed. The selection stays on the
    /// same entry if it's still there, else on the one that took its place. Returns
    /// false when nothing is left to switch to.
    pub fn refresh(&mut self, items: Vec<Item>) -> bool {
        let mode = self.mode;
        let keep = self.selected().map(|i| i.id(mode).to_string());
        self.items = items;
        if self.items.is_empty() {
            return false;
        }
        self.selected = match keep.and_then(|k| self.items.iter().position(|i| i.id(mode) == k)) {
            Some(i) => i,
            None => self.selected.min(self.items.len() - 1),
        };
        true
    }

    /// Show the windows of the selected app instead (`windows`: its windows, most recent
    /// first). Only from the app switcher.
    pub fn expand(&mut self, windows: Vec<Item>) -> bool {
        if self.mode != Mode::Apps || windows.is_empty() {
            return false;
        }
        self.expanded = self.selected().map(|i| i.app.clone());
        self.mode = Mode::Windows;
        self.items = windows;
        self.selected = 0;
        true
    }

    /// Back from an expanded app to all apps (`apps`), with that app selected.
    pub fn collapse(&mut self, apps: Vec<Item>) -> bool {
        let Some(app) = self.expanded.take() else { return false };
        if apps.is_empty() {
            return false;
        }
        self.mode = Mode::Apps;
        self.selected = apps.iter().position(|a| a.app == app).unwrap_or(0);
        self.items = apps;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Scope;
    use crate::model::{Filter, items};
    use crate::testing::desktop;
    use nyx_desktop::apps::Index;

    fn windows() -> Vec<Item> {
        items(&desktop(), &Index::default(), Mode::Windows, &Filter { scope: Scope::All, ..Default::default() })
    }

    fn title(s: &Session) -> &str {
        &s.selected().unwrap().title
    }

    #[test]
    fn a_tap_goes_to_the_previous_window() {
        let s = Session::start(Mode::Windows, windows(), Some("a1"), true).unwrap();
        assert_eq!(title(&s), "web 1");
        let back = Session::start(Mode::Windows, windows(), Some("a1"), false).unwrap();
        assert_eq!(title(&back), "minimized notes", "Shift starts from the end");
        let unfocused = Session::start(Mode::Windows, windows(), None, true).unwrap();
        assert_eq!(title(&unfocused), "term 1", "nothing focused: the most recent window");
        assert!(Session::start(Mode::Windows, Vec::new(), None, true).is_none());
    }

    #[test]
    fn steps_wrap_both_ways() {
        let mut s = Session::start(Mode::Windows, windows(), Some("a1"), true).unwrap();
        let n = s.items.len();
        for _ in 0..n {
            s.step(true);
        }
        assert_eq!(title(&s), "web 1", "a full round comes back");
        s.step(false);
        s.step(false);
        assert_eq!(title(&s), "minimized notes");
        s.move_by(-(n as isize) * 3);
        assert_eq!(title(&s), "minimized notes");
    }

    #[test]
    fn a_closed_window_moves_the_selection_along() {
        let mut s = Session::start(Mode::Windows, windows(), Some("a1"), true).unwrap();
        s.step(true); // term 2
        let mut fewer = windows();
        fewer.retain(|i| i.title != "web 1");
        assert!(s.refresh(fewer.clone()));
        assert_eq!(title(&s), "term 2", "stays on its window");
        fewer.retain(|i| i.title != "term 2");
        assert!(s.refresh(fewer));
        assert_eq!(title(&s), "web 2", "the next one takes its place");
        assert!(!s.refresh(Vec::new()));
    }

    #[test]
    fn expand_and_collapse_an_app() {
        let d = desktop();
        let idx = Index::default();
        let apps = items(&d, &idx, Mode::Apps, &Filter::default());
        let mut s = Session::start(Mode::Apps, apps.clone(), Some("a1"), true).unwrap();
        assert_eq!(s.selected().unwrap().app, "class:firefox");
        let ff = items(&d, &idx, Mode::Windows, &Filter { app: Some("class:firefox".into()), ..Default::default() });
        assert!(s.expand(ff));
        assert_eq!(s.mode, Mode::Windows);
        assert_eq!(s.items.len(), 2);
        assert!(!s.expand(windows()), "only apps expand");
        assert!(s.collapse(apps));
        assert_eq!(s.selected().unwrap().app, "class:firefox");
        assert_eq!(s.mode, Mode::Apps);
    }
}

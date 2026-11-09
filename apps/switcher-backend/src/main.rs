//! `nyx-switcher-backend`: the switcher without GTK, for its Quickshell interface
//! (`qml/Switcher` in this repository).
//!
//! The shortcuts' fast path stays as it is: `nyx-switcher-send next windows` writes one
//! line to `$XDG_RUNTIME_DIR/nyx-switcher.sock`, read here (`ipc.rs`, shared with the
//! GTK switcher), and the session is `nyx-switcher-core`'s. What to draw goes to the
//! interface over `$XDG_RUNTIME_DIR/nyx-switcher/ui.sock` (nyx-ui-link):
//!
//! - topic `switcher`: the session (`open`, `mode`, `items`, `selected`, the caption,
//!   the modifiers that keep it open, `pending` while the card waits to show);
//! - topic `overview`: the overview's workspaces and windows, when it's open;
//! - topic `thumbs`: window thumbnails, `{stable id: path?stamp}` (PPM files under
//!   `$XDG_RUNTIME_DIR/nyx-switcher/shots`).
//!
//! The interface sends keys (`Key`), the modifier going up (`Release`), clicks and the
//! overview's actions. It owns `io.github.design_nexus.nyx.Switcher` and serves
//! `Panel1` (`switcher`) as the GTK switcher did.

#[path = "../../switcher/src/ipc.rs"]
#[allow(dead_code)]
mod ipc;
mod shots;

use gio::prelude::*;
use ipc::{Kind, Request};
use nyx_desktop::apps::Index;
use nyx_desktop::{Desktop, actions, watch};
use nyx_switcher_core::binds;
use nyx_switcher_core::config::{Config, Scope};
use nyx_switcher_core::model::{self, Filter, Item, Mode};
use nyx_switcher_core::session::{Expanded, Session};
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

pub const APP_ID: &str = "io.github.design_nexus.nyx.Switcher";

const APP_TILE: i32 = 112;
const WINDOW_TILE: i32 = 184;
const THUMB_TILE: i32 = 256;
const THUMB_W: i32 = 232;
const THUMB_H: i32 = 146;
/// Overview tile width with its margins.
const OV_TILE: i32 = 272;

fn scaled(px: i32, config: &Config) -> i32 {
    (px as f64 * config.thumbnail_size.factor()).round() as i32
}

fn now_us() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_micros()).unwrap_or(0)
}

#[derive(Default)]
struct Overview {
    open: bool,
    serial: u64,
    app: Option<String>,
    query: String,
    screen: String,
}

pub struct App {
    link: nyx_ui_link::Link,
    desktop: RefCell<Arc<Desktop>>,
    index: RefCell<Index>,
    config: RefCell<Config>,
    session: RefCell<Option<Session>>,
    query: RefCell<String>,
    /// Hyprland modifier bits that keep the session open (empty: until Enter or Esc).
    held: Cell<u32>,
    open: Cell<bool>,
    serial: Cell<u64>,
    screen: RefCell<String>,
    pending: Cell<bool>,
    reveal: RefCell<Option<glib::SourceId>>,
    /// Switched: where to, once the interface has let go of the keyboard.
    focus_next: RefCell<Option<(String, bool, i64)>>,
    opened: Cell<Option<u128>>,
    overview: RefCell<Overview>,
    pub shots: shots::Shots,
    last_active: RefCell<Option<String>>,
    panel1: RefCell<Option<nyx_desktop::panel::Panel>>,
    only: Option<String>,
    debug: bool,
}

pub fn app_info(app: &str) -> Option<gio_unix::DesktopAppInfo> {
    if app.starts_with("class:") { None } else { gio_unix::DesktopAppInfo::new(&format!("{app}.desktop")) }
}

pub fn app_name(item: &Item) -> String {
    app_info(&item.app).map(|i| i.display_name().to_string()).unwrap_or_else(|| item.class.clone())
}

/// Icon names to try, best first: the app's own, then guesses from its class.
fn icons(item: &Item) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(icon) = app_info(&item.app).and_then(|i| i.icon())
        && let Some(s) = IconExt::to_string(&icon)
    {
        out.push(s.to_string());
    }
    let class = item.class.to_lowercase();
    out.push(class.clone());
    if let Some(last) = class.rsplit('.').next() {
        out.push(last.to_string());
    }
    out
}

/// Where a window is, in words: `Workspace 3`, `Minimized`, `Workspace 12 · DP-1`.
fn place(item: &Item, desktop: &Desktop) -> String {
    if item.minimized {
        return "Minimized".into();
    }
    let ws = if item.workspace_name.starts_with("special:") {
        item.workspace_name.trim_start_matches("special:").to_string()
    } else if item.workspace_name.parse::<i64>().is_ok() {
        format!("Workspace {}", item.workspace_name)
    } else {
        item.workspace_name.clone()
    };
    if several_monitors(desktop) { format!("{ws} · {}", item.monitor) } else { ws }
}

fn several_monitors(desktop: &Desktop) -> bool {
    desktop.monitors.iter().filter(|m| !m.disabled && !m.is_virtual()).count() > 1
}

fn item_json(item: &Item, mode: Mode, desktop: &Desktop) -> Value {
    let tabs = if item.windows.len() > 1 && item.tab().is_some() {
        Some(format!("{} tabs", item.windows.len()))
    } else {
        item.tab().map(|(i, n)| format!("Tab {i} of {n}"))
    };
    json!({
        "id": item.id(mode),
        "address": item.address,
        "stable_id": item.stable_id,
        "title": if item.title.is_empty() { &item.class } else { &item.title },
        "class": item.class,
        "app": item.app,
        "name": app_name(item),
        "icons": icons(item),
        "place": place(item, desktop),
        "urgent": item.urgent,
        "minimized": item.minimized,
        "floating": item.floating,
        "grouped": item.group.len() > 1,
        "windows": item.windows.len(),
        "tabs": tabs,
        "workspace": item.workspace,
        "monitor": item.monitor,
    })
}

impl App {
    fn log(&self, what: &str) {
        if self.debug {
            eprintln!("nyx-switcher-backend: {what}");
        }
    }

    fn desktop(&self) -> Arc<Desktop> {
        self.desktop.borrow().clone()
    }

    fn filter(&self, app: Option<String>, sticky_all: bool) -> Filter {
        let c = self.config.borrow();
        Filter {
            scope: if sticky_all { Scope::All } else { c.scope },
            show_agent_workspaces: c.show_agent_workspaces,
            app,
            only: None,
            collapse_groups: c.collapse_groups && !sticky_all,
        }
    }

    /// The entries for `mode`, or for an expanded app or group, narrowed by the search.
    fn items(&self, mode: Mode, expanded: Option<&Expanded>) -> Vec<Item> {
        let desktop = self.desktop();
        let mut filter = self.filter(None, false);
        match expanded {
            Some(e) if e.from == Mode::Apps => filter.app = Some(e.id.clone()),
            Some(e) => filter.only = Some(e.windows.clone()),
            None => {}
        }
        let mut items = model::items(&desktop, &self.index.borrow(), mode, &filter);
        let query = self.query.borrow();
        if !query.is_empty() {
            items.retain(|i| nyx_switcher_core::overview::matches(i, &app_name(i), &query));
        }
        items
    }

    fn screen_now(&self) -> String {
        match &self.only {
            Some(o) => o.clone(),
            None => self.desktop().focused_monitor().map(|m| m.name.clone()).unwrap_or_default(),
        }
    }

    fn monitor_width(&self) -> i32 {
        let desktop = self.desktop();
        let name = self.screen.borrow().clone();
        desktop.monitors.iter().find(|m| m.name == name).map(|m| m.logical_size().0).unwrap_or(1280)
    }

    fn columns(&self) -> usize {
        let s = self.session.borrow();
        let Some(s) = s.as_ref() else { return 1 };
        let tile = if s.mode == Mode::Apps {
            APP_TILE
        } else if self.config.borrow().thumbnails {
            scaled(THUMB_TILE, &self.config.borrow())
        } else {
            WINDOW_TILE
        };
        let n = s.items.len().max(1) as i32;
        ((self.monitor_width() as f64 * 0.8) as i32 / (tile + 8)).clamp(1, 12).min(n) as usize
    }

    // ---------- Publishing ----------

    fn publish(&self) {
        let desktop = self.desktop();
        let c = self.config.borrow().clone();
        let v = match self.session.borrow().as_ref() {
            Some(s) => {
                let (caption, detail) = match s.selected() {
                    None => ("No matching windows".to_string(), "Backspace to widen the search".to_string()),
                    Some(item) if s.mode == Mode::Apps => {
                        let n = item.windows.len();
                        (app_name(item), if n == 1 { "1 window".into() } else { format!("{n} windows · ↓ to choose") })
                    }
                    Some(item) => {
                        let group = if item.windows.len() > 1 {
                            format!(" · {} tabs · ↓ to choose", item.windows.len())
                        } else {
                            item.tab().map(|(i, n)| format!(" · tab {i} of {n}")).unwrap_or_default()
                        };
                        let title = if item.title.is_empty() { item.class.clone() } else { item.title.clone() };
                        (title, format!("{} · {}{group}", app_name(item), place(item, &desktop)))
                    }
                };
                let held = self.held.get();
                json!({
                    "open": self.open.get(),
                    "serial": self.serial.get(),
                    "mode": s.mode.name(),
                    "apps": s.mode == Mode::Apps,
                    "items": s.items.iter().map(|i| item_json(i, s.mode, &desktop)).collect::<Vec<_>>(),
                    "selected": s.selected,
                    "caption": caption,
                    "detail": detail,
                    "query": *self.query.borrow(),
                    "held": {"alt": held & binds::ALT != 0, "super": held & binds::SUPER != 0, "ctrl": held & binds::CTRL != 0},
                    "pending": self.pending.get(),
                    "columns": self.columns(),
                    "screen": *self.screen.borrow(),
                    "thumbnails": c.thumbnails,
                    "animation": c.animation,
                    "tile": {"tile": scaled(THUMB_TILE, &c), "w": scaled(THUMB_W, &c), "h": scaled(THUMB_H, &c), "app": APP_TILE, "window": WINDOW_TILE},
                })
            }
            None => json!({"open": false, "serial": self.serial.get(), "items": [], "selected": 0}),
        };
        self.link.publish("switcher", v);
    }

    fn publish_overview(&self) {
        let o = self.overview.borrow();
        if !o.open {
            self.link.publish("overview", json!({"open": false, "serial": o.serial}));
            return;
        }
        let desktop = self.desktop();
        let mut items = model::items(&desktop, &self.index.borrow(), Mode::Windows, &self.filter(o.app.clone(), true));
        items.retain(|i| nyx_switcher_core::overview::matches(i, &app_name(i), &o.query));
        let groups = nyx_switcher_core::overview::groups(&desktop, items);
        let width = desktop.monitors.iter().find(|m| m.name == o.screen).map(|m| m.logical_size().0).unwrap_or(1280);
        let columns = ((width - 96) / OV_TILE).clamp(1, 8) as usize;
        let several = several_monitors(&desktop);
        let groups: Vec<Value> = groups
            .iter()
            .map(|g| {
                let mut meta = Vec::new();
                if several && !g.minimized {
                    meta.push(g.monitor.clone());
                }
                if g.visible {
                    meta.push("on screen".to_string());
                }
                let per_row = match g.items.len() {
                    0..=3 => g.items.len().max(1),
                    4 => 2,
                    _ => 3,
                }
                .min(columns);
                json!({
                    "title": g.title(),
                    "meta": meta.join(" · "),
                    "workspace": g.workspace,
                    "minimized": g.minimized,
                    "per_row": per_row,
                    "items": g.items.iter().map(|i| item_json(i, Mode::Windows, &desktop)).collect::<Vec<_>>(),
                })
            })
            .collect();
        let mut ids: Vec<i64> = desktop.workspaces.iter().map(|w| w.id).filter(|&id| id > 0).collect();
        ids.sort_unstable();
        let free = (1..).find(|n| !ids.contains(n)).unwrap_or(1);
        let monitors: Vec<String> =
            desktop.monitors.iter().filter(|m| !m.disabled && !m.is_virtual()).map(|m| m.name.clone()).collect();
        let store = nyx_desktop::profiles::Store::user();
        let list = store.list();
        let active = store.active();
        let profiles = json!({
            "shown": list.len() >= 2,
            "active": active,
            "name": list.iter().find(|p| active.as_deref() == Some(p.id.as_str())).map(|p| p.name.clone()).unwrap_or_default(),
            "list": list.iter().map(|p| json!({"id": p.id, "name": p.name})).collect::<Vec<_>>(),
        });
        self.link.publish(
            "overview",
            json!({
                "open": true,
                "serial": o.serial,
                "screen": o.screen,
                "app": o.app,
                "query": o.query,
                "columns": columns,
                "groups": groups,
                "workspaces": ids,
                "free": free,
                "monitors": monitors,
                "profiles": profiles,
                "thumbnails": self.config.borrow().thumbnails,
                "tile": {"tile": scaled(THUMB_TILE, &self.config.borrow()), "w": scaled(THUMB_W, &self.config.borrow()), "h": scaled(THUMB_H, &self.config.borrow())},
            }),
        );
    }

    // ---------- Requests ----------

    fn request(self: &Rc<Self>, req: Request) {
        if req.sent_us > 0 {
            self.log(&format!("{:?} arrived {:.2} ms after the key", req.kind, (now_us().saturating_sub(req.sent_us)) as f64 / 1000.0));
        }
        match req.kind {
            Kind::Hide => {
                self.cancel();
                self.close_overview();
            }
            Kind::Overview { app } => {
                self.cancel();
                if self.overview.borrow().open && app.is_none() {
                    self.close_overview();
                } else {
                    self.open_overview(app);
                }
            }
            Kind::Step { mode, forward } => {
                let same = self
                    .session
                    .borrow()
                    .as_ref()
                    .map(|s| s.mode == mode || s.expanded.as_ref().is_some_and(|e| e.from == mode));
                match same {
                    Some(true) => {
                        if let Some(s) = self.session.borrow_mut().as_mut() {
                            s.step(forward);
                        }
                        self.publish();
                    }
                    _ => self.open_session(mode, forward, req.sent_us),
                }
            }
        }
    }

    fn open_session(self: &Rc<Self>, mode: Mode, forward: bool, sent_us: u128) {
        *self.config.borrow_mut() = Config::load();
        self.close_overview();
        self.query.borrow_mut().clear();
        let desktop = self.desktop();
        let items = self.items(mode, None);
        let Some(session) = Session::start(mode, items, desktop.active_window.as_deref(), forward) else {
            self.log("nothing to switch to");
            self.cancel();
            return;
        };
        let held = {
            let keys = &self.config.borrow().keys;
            let key = match mode {
                Mode::Windows => &keys.windows,
                Mode::Apps => &keys.apps,
                Mode::SameApp => &keys.same_app,
            };
            binds::held(key)
        };
        self.held.set(held);
        *self.session.borrow_mut() = Some(session);
        *self.screen.borrow_mut() = self.screen_now();
        if let Some(t) = self.reveal.borrow_mut().take() {
            t.remove();
        }
        let delay = if held == 0 { 0 } else { self.config.borrow().show_delay_ms };
        let was_open = self.open.get();
        self.pending.set(delay > 0 && !was_open);
        if self.pending.get() {
            let w = Rc::downgrade(self);
            *self.reveal.borrow_mut() = Some(glib::timeout_add_local_once(Duration::from_millis(delay as u64), move || {
                if let Some(app) = w.upgrade() {
                    app.reveal.borrow_mut().take();
                    app.pending.set(false);
                    app.publish();
                }
            }));
        }
        self.open.set(true);
        self.serial.set(self.serial.get() + 1);
        self.opened.set(Some(sent_us));
        self.focus_next.borrow_mut().take();
        self.publish();
        self.capture_listed();
        if let Some(p) = self.panel1.borrow().as_ref() {
            p.announce();
        }
    }

    /// Switch to the selected window and hide; the focus moves once the interface has
    /// let go of the keyboard (`Closed`), or after a moment.
    fn commit(self: &Rc<Self>) {
        let Some(session) = self.session.borrow_mut().take() else { return };
        let item = session.selected().cloned();
        let current = self.desktop().focused_monitor().map(|m| m.active_workspace).unwrap_or(1);
        if let Some(item) = item {
            self.log(&format!("switch to {} ({})", item.title, item.address));
            *self.focus_next.borrow_mut() = Some((item.address.clone(), item.minimized, current));
            let w = Rc::downgrade(self);
            glib::timeout_add_local_once(Duration::from_millis(150), move || {
                if let Some(app) = w.upgrade() {
                    app.focus_now();
                }
            });
        }
        self.hide();
    }

    fn focus_now(&self) {
        if let Some((address, minimized, current)) = self.focus_next.borrow_mut().take() {
            std::thread::spawn(move || actions::focus(&address, minimized, current));
        }
    }

    fn cancel(&self) {
        if self.session.borrow_mut().take().is_some() {
            self.log("cancelled");
        }
        self.hide();
    }

    fn hide(&self) {
        if let Some(t) = self.reveal.borrow_mut().take() {
            t.remove();
        }
        if self.open.replace(false) {
            self.publish();
        }
    }

    fn open_overview(self: &Rc<Self>, app: Option<String>) {
        *self.config.borrow_mut() = Config::load();
        {
            let mut o = self.overview.borrow_mut();
            o.open = true;
            o.serial += 1;
            o.app = app;
            o.query.clear();
            o.screen = self.screen_now();
        }
        self.publish_overview();
        let desktop = self.desktop();
        let app = self.overview.borrow().app.clone();
        let ids: Vec<String> = model::items(&desktop, &self.index.borrow(), Mode::Windows, &self.filter(app, true))
            .iter()
            .map(|i| i.stable_id.clone())
            .filter(|id| !id.is_empty())
            .take(40)
            .collect();
        self.capture(ids);
        if let Some(p) = self.panel1.borrow().as_ref() {
            p.announce();
        }
    }

    fn close_overview(&self) {
        let was = std::mem::replace(&mut self.overview.borrow_mut().open, false);
        if was {
            self.publish_overview();
        }
    }

    // ---------- Keys (the interface forwards them) ----------

    fn key(self: &Rc<Self>, key: &str, shift: bool, ctrl: bool, text: &str) {
        if self.session.borrow().is_none() {
            return;
        }
        let cols = self.columns() as isize;
        let expanded = self.session.borrow().as_ref().is_some_and(|s| s.expanded.is_some());
        let can_expand = self.session.borrow().as_ref().is_some_and(|s| s.can_expand());
        let selected = self.session.borrow().as_ref().map(|s| s.selected).unwrap_or(0);
        match key {
            "Escape" => self.cancel(),
            "Return" | "Enter" | "Space" => self.commit(),
            "Tab" => self.move_by(if shift { -1 } else { 1 }),
            "Backtab" => self.move_by(-1),
            "Grave" => self.move_by(if shift { -1 } else { 1 }),
            "Left" => self.move_by(-1),
            "Right" => self.move_by(1),
            "Down" if can_expand => self.expand(),
            "Up" if expanded && (selected as isize) < cols => self.collapse(),
            "Down" => self.move_row(cols),
            "Up" => self.move_row(-cols),
            "Home" => self.select(0),
            "End" => self.select(usize::MAX),
            "Delete" => {
                let address = self.session.borrow().as_ref().and_then(|s| s.selected().map(|i| i.address.clone()));
                if let Some(a) = address {
                    std::thread::spawn(move || actions::close(&a));
                }
            }
            "Backspace" => {
                if self.query.borrow_mut().pop().is_some() {
                    self.search_changed();
                }
            }
            _ => {
                let typed: String = text.chars().filter(|c| !c.is_control()).collect();
                if !typed.is_empty() && !ctrl {
                    self.query.borrow_mut().push_str(&typed);
                    self.search_changed();
                }
            }
        }
    }

    fn search_changed(self: &Rc<Self>) {
        let (mode, expanded) = match self.session.borrow().as_ref() {
            Some(s) => (s.mode, s.expanded.clone()),
            None => return,
        };
        let items = self.items(mode, expanded.as_ref());
        if let Some(s) = self.session.borrow_mut().as_mut() {
            if !s.refresh(items) {
                s.items.clear();
            }
            s.selected = 0;
        }
        self.publish();
        self.capture_listed();
    }

    fn move_by(&self, delta: isize) {
        if let Some(s) = self.session.borrow_mut().as_mut() {
            s.move_by(delta);
        }
        self.publish();
    }

    fn move_row(&self, delta: isize) {
        if let Some(s) = self.session.borrow_mut().as_mut() {
            let to = s.selected as isize + delta;
            if to >= 0 && (to as usize) < s.items.len() {
                s.selected = to as usize;
            }
        }
        self.publish();
    }

    fn select(&self, index: usize) {
        if let Some(s) = self.session.borrow_mut().as_mut() {
            s.select(index.min(s.items.len().saturating_sub(1)));
        }
        self.publish();
    }

    fn expand(self: &Rc<Self>) {
        let target = self.session.borrow().as_ref().and_then(|s| {
            let i = s.selected()?;
            Some(Expanded { from: s.mode, id: i.id(s.mode).to_string(), windows: i.windows.clone() })
        });
        let Some(target) = target else { return };
        let windows = self.items(Mode::Windows, Some(&target));
        if self.session.borrow_mut().as_mut().is_some_and(|s| s.expand(windows)) {
            self.publish();
            self.capture_listed();
        }
    }

    fn collapse(&self) {
        let Some(from) = self.session.borrow().as_ref().and_then(|s| s.expanded.as_ref().map(|e| e.from)) else { return };
        let items = self.items(from, None);
        if self.session.borrow_mut().as_mut().is_some_and(|s| s.collapse(items)) {
            self.publish();
        }
    }

    /// Windows opened, closed or were renamed during a session.
    fn desktop_changed(&self) {
        let (mode, expanded) = match self.session.borrow().as_ref() {
            Some(s) => (s.mode, s.expanded.clone()),
            None => return,
        };
        let items = self.items(mode, expanded.as_ref());
        let alive = self.session.borrow_mut().as_mut().is_some_and(|s| s.refresh(items));
        if alive {
            self.publish();
        } else if !self.query.borrow().is_empty() {
            if let Some(s) = self.session.borrow_mut().as_mut() {
                s.items.clear();
            }
            self.publish();
        } else {
            self.cancel();
        }
    }

    // ---------- Thumbnails ----------

    fn capture(self: &Rc<Self>, ids: Vec<String>) {
        if ids.is_empty() || !self.config.borrow().thumbnails {
            return;
        }
        let desktop = self.desktop();
        let screen = self.screen.borrow().clone();
        let scale = desktop.monitors.iter().find(|m| m.name == screen).map(|m| m.scale.ceil() as u32).unwrap_or(2).max(1);
        let (w, h) = {
            let c = self.config.borrow();
            (scaled(THUMB_W, &c) as u32 * scale, scaled(THUMB_H, &c) as u32 * scale)
        };
        let link = self.link.clone();
        self.shots.capture(ids, w, h, move |all| link.publish("thumbs", all));
    }

    fn capture_listed(self: &Rc<Self>) {
        let ids: Vec<String> = match self.session.borrow().as_ref() {
            Some(s) if s.mode != Mode::Apps => {
                s.items.iter().take(30).map(|i| i.stable_id.clone()).filter(|id| !id.is_empty()).collect()
            }
            _ => return,
        };
        self.capture(ids);
    }

    /// The window that just lost focus is captured as it was left, so the next switch
    /// shows it at once.
    fn focus_moved(self: &Rc<Self>) {
        let desktop = self.desktop();
        let active = desktop.active_window.clone();
        let previous = self.last_active.replace(active.clone());
        if self.session.borrow().is_some() {
            return;
        }
        let Some(prev) = previous.filter(|p| Some(p) != active.as_ref()) else { return };
        let Some(id) = desktop.window(&prev).map(|w| w.stable_id.clone()).filter(|id| !id.is_empty()) else { return };
        if !self.shots.recent(&id) {
            self.capture(vec![id]);
        }
    }

    // ---------- Operations from the interface ----------

    fn handle(self: &Rc<Self>, req: nyx_ui_link::Request) {
        let b = req.body.clone();
        let s = |k: &str| b.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
        let flag = |k: &str| b.get(k).and_then(Value::as_bool).unwrap_or(false);
        match req.op.as_str() {
            "Key" => self.key(&s("key"), flag("shift"), flag("ctrl"), &s("text")),
            "Release" => {
                self.log("modifier up");
                self.commit();
            }
            "Select" => self.select(b.get("index").and_then(Value::as_u64).unwrap_or(0) as usize),
            "Activate" => {
                self.select(b.get("index").and_then(Value::as_u64).unwrap_or(0) as usize);
                self.commit();
            }
            "Cancel" => self.cancel(),
            "Closed" => self.focus_now(),
            "Shown" => {
                if let Some(sent) = self.opened.take().filter(|s| *s > 0) {
                    self.log(&format!("shown {:.2} ms after the key", (now_us().saturating_sub(sent)) as f64 / 1000.0));
                }
            }
            "Open" => {
                let mode = Mode::parse(&s("mode")).unwrap_or(Mode::Windows);
                self.request(Request { kind: Kind::Step { mode, forward: !flag("reverse") }, sent_us: now_us() });
            }
            "OverviewOpen" => self.open_overview(b.get("app").and_then(Value::as_str).map(String::from)),
            "OverviewQuery" => {
                self.overview.borrow_mut().query = s("text");
                self.publish_overview();
            }
            "OverviewClose" => self.close_overview(),
            "OverviewActivate" => {
                let current = self.desktop().focused_monitor().map(|m| m.active_workspace).unwrap_or(1);
                let (address, minimized) = (s("address"), flag("minimized"));
                self.close_overview();
                std::thread::spawn(move || actions::focus(&address, minimized, current));
            }
            "WindowAction" => {
                let address = s("address");
                let d = match s("action").as_str() {
                    "close" => Some(actions::close_window(&address)),
                    "float" => Some(actions::toggle_floating(&address)),
                    "ungroup" => Some(actions::leave_group(&address)),
                    "workspace" => Some(actions::move_to_workspace(&address, &s("to"), false)),
                    "monitor" => Some(actions::move_to_monitor(&address, &s("to"))),
                    _ => None,
                };
                if let Some(d) = d {
                    std::thread::spawn(move || {
                        d.run();
                    });
                }
            }
            "OwnTitlebar" => {
                let app = s("app");
                std::thread::spawn(move || {
                    let _ = std::process::Command::new("nyx-settings").args(["--own-titlebar", &app]).output();
                });
            }
            "GoWorkspace" => {
                let id = b.get("workspace").and_then(Value::as_i64).unwrap_or(1);
                self.close_overview();
                std::thread::spawn(move || {
                    actions::Dispatch { lua: format!("hl.dsp.focus({{ workspace = \"{id}\" }})"), classic: format!("workspace {id}") }
                        .run();
                });
            }
            "Profile" => {
                let id = s("id");
                std::thread::spawn(move || {
                    let _ = std::process::Command::new("nyx-settings").args(["--workspace-profile", &id]).output();
                });
            }
            _ => {
                let op = req.op.clone();
                return req.err("unknown_op", op);
            }
        }
        req.ok(json!({}));
    }
}

/// Keep the theme snapshot current for Quickshell parts where no theme service runs.
fn follow_theme() {
    nyx_theme_core::notify_changed("system", "shell");
    let pending = Rc::new(Cell::new(false));
    let mut monitors = Vec::new();
    for dir in nyx_theme_core::paths::watched_dirs() {
        let Ok(m) = gio::File::for_path(&dir).monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE) else {
            continue;
        };
        let pending = pending.clone();
        m.connect_changed(move |_, file, _, _| {
            if file.basename().is_some_and(|b| b.to_string_lossy().ends_with(".tmp")) || pending.replace(true) {
                return;
            }
            let pending = pending.clone();
            glib::timeout_add_local_once(Duration::from_millis(120), move || {
                pending.set(false);
                nyx_theme_core::notify_changed("system", "shell");
            });
        });
        monitors.push(m);
    }
    std::mem::forget(monitors);
}

fn main() -> std::process::ExitCode {
    if std::env::args().any(|a| a == "-h" || a == "--help") {
        println!(
            "Usage: nyx-switcher-backend\n\nThe switcher's services for its Quickshell interface (nyx-switcher --ui qml). NYX_SWITCHER_ONLY=OUTPUT keeps it to one output; NYX_SWITCHER_SOCKET and NYX_SWITCHER_UI_SOCKET make a test copy."
        );
        return std::process::ExitCode::SUCCESS;
    }
    let only = std::env::var("NYX_SWITCHER_ONLY").ok().filter(|s| !s.is_empty());
    let test = std::env::var_os("NYX_SWITCHER_UI_SOCKET").is_some();
    let ctx = glib::MainContext::default();
    let _guard = ctx.acquire().expect("main context");
    let (req_tx, req_rx) = async_channel::unbounded::<nyx_ui_link::Request>();
    let link = match nyx_ui_link::serve(nyx_ui_link::Options::new("nyx-switcher"), move |r| {
        let _ = req_tx.send_blocking(r);
    }) {
        Ok(l) => l,
        Err(nyx_ui_link::Error::AlreadyRunning(p)) => {
            eprintln!("nyx-switcher-backend: already running at {}", p.display());
            return std::process::ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("nyx-switcher-backend: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let app = Rc::new(App {
        link,
        desktop: RefCell::default(),
        index: RefCell::new(nyx_desktop::apps::from_gio()),
        config: RefCell::new(Config::load()),
        session: RefCell::default(),
        query: RefCell::default(),
        held: Cell::new(0),
        open: Cell::new(false),
        serial: Cell::new(0),
        screen: RefCell::default(),
        pending: Cell::new(false),
        reveal: RefCell::default(),
        focus_next: RefCell::default(),
        opened: Cell::new(None),
        overview: RefCell::default(),
        shots: shots::Shots::default(),
        last_active: RefCell::default(),
        panel1: RefCell::default(),
        only,
        debug: std::env::var_os("NYX_SWITCHER_DEBUG").is_some(),
    });
    app.publish();
    app.publish_overview();
    app.link.publish("thumbs", json!({}));
    follow_theme();

    let w = Rc::downgrade(&app);
    if let Err(e) = ipc::listen(move |req| {
        if let Some(app) = w.upgrade() {
            app.request(req);
        }
    }) {
        eprintln!("nyx-switcher-backend: {e}");
        return std::process::ExitCode::FAILURE;
    }

    let w = Rc::downgrade(&app);
    std::mem::forget(watch::watch_local(move |u| {
        let Some(app) = w.upgrade() else { return };
        *app.desktop.borrow_mut() = u.desktop.clone();
        if u.changes.focus {
            app.focus_moved();
        }
        if u.changes.windows {
            let alive: std::collections::HashSet<String> = u.desktop.windows.iter().map(|w| w.stable_id.clone()).collect();
            if app.shots.forget(&alive) {
                app.link.publish("thumbs", app.shots.all());
            }
        }
        if u.changes.windows || u.changes.titles {
            app.desktop_changed();
        }
        if app.overview.borrow().open && (u.changes.windows || u.changes.titles || u.changes.active || u.changes.workspaces) {
            app.publish_overview();
        }
    }));
    let w = Rc::downgrade(&app);
    gio::AppInfoMonitor::get().connect_changed(move |_| {
        if let Some(app) = w.upgrade() {
            *app.index.borrow_mut() = nyx_desktop::apps::from_gio();
        }
    });

    if !test {
        match gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) {
            Ok(conn) => {
                let (w1, w2, w3) = (Rc::downgrade(&app), Rc::downgrade(&app), Rc::downgrade(&app));
                let handlers = nyx_desktop::panel::Handlers {
                    show: Box::new(move || {
                        if let Some(app) = w1.upgrade() {
                            app.request(Request { kind: Kind::Overview { app: None }, sent_us: 0 });
                        }
                    }),
                    hide: Box::new(move || {
                        if let Some(app) = w2.upgrade() {
                            app.cancel();
                            app.close_overview();
                        }
                    }),
                    is_open: Box::new(move || w3.upgrade().is_some_and(|app| app.open.get() || app.overview.borrow().open)),
                };
                match nyx_desktop::panel::Panel::serve(&conn, "switcher", handlers) {
                    Ok(p) => *app.panel1.borrow_mut() = Some(p),
                    Err(e) => eprintln!("nyx-switcher-backend: Panel1: {e}"),
                }
                gio::bus_own_name_on_connection(
                    &conn,
                    APP_ID,
                    gio::BusNameOwnerFlags::REPLACE | gio::BusNameOwnerFlags::ALLOW_REPLACEMENT,
                    |_, _| {},
                    |_, name| eprintln!("nyx-switcher-backend: {name} is taken (is the GTK switcher running?)"),
                );
                std::mem::forget(conn);
            }
            Err(e) => eprintln!("nyx-switcher-backend: no session bus: {e}"),
        }
    }

    let a = app.clone();
    ctx.spawn_local(async move {
        while let Ok(req) = req_rx.recv().await {
            a.handle(req);
        }
    });
    glib::MainLoop::new(Some(&ctx), false).run();
    std::process::ExitCode::SUCCESS
}

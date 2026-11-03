//! The switcher on screen: one layer-shell surface, built once and kept hidden, shown on
//! the focused monitor for each session.
//!
//! While a shortcut is held the surface takes the keyboard (exclusively), so it sees the
//! modifier go up: that switches. Hyprland keeps the bound chords (Alt+Tab, …) and sends
//! them here as further requests, which move the selection. The card stays transparent
//! for `show_delay_ms`, so a quick tap switches without anything flashing up.

use crate::ipc::{Kind, Request};
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use gtk4_layer_shell::{KeyboardMode, Layer, LayerShell};
use nyx_desktop::apps::Index;
use nyx_desktop::{Desktop, watch};
use nyx_switcher_core::binds;
use nyx_switcher_core::config::{Config, Scope};
use nyx_switcher_core::model::{self, Filter, Item, Mode};
use nyx_switcher_core::session::Session;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::{Duration, Instant};

const APP_TILE: i32 = 112;
const WINDOW_TILE: i32 = 184;

pub struct Switcher {
    window: gtk::Window,
    card: gtk::Box,
    grid: gtk::FlowBox,
    caption: gtk::Label,
    detail: gtk::Label,
    tiles: RefCell<Vec<gtk::FlowBoxChild>>,
    desktop: RefCell<Arc<Desktop>>,
    index: RefCell<Index>,
    config: RefCell<Config>,
    session: RefCell<Option<Session>>,
    /// Modifiers (GDK) that keep the session open; empty for a session that stays open
    /// until Enter, a click or Esc.
    held: Cell<gdk::ModifierType>,
    reveal: RefCell<Option<glib::SourceId>>,
    guard: RefCell<Option<glib::SourceId>>,
    /// The pointer moved since the session started (hover only selects after that).
    pointer: Cell<Option<(f64, f64)>>,
    pointer_moved: Cell<bool>,
    only: Option<String>,
    keyboard: bool,
    debug: bool,
    opened: Cell<Option<(Instant, u128)>>,
    panel: RefCell<Option<nyx_desktop::panel::Panel>>,
    me: RefCell<Weak<Switcher>>,
}

fn now_us() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_micros()).unwrap_or(0)
}

/// Hyprland modifier bits as GDK's.
fn gdk_mods(mask: u32) -> gdk::ModifierType {
    let mut m = gdk::ModifierType::empty();
    if mask & binds::ALT != 0 {
        m |= gdk::ModifierType::ALT_MASK;
    }
    if mask & binds::SUPER != 0 {
        m |= gdk::ModifierType::SUPER_MASK | gdk::ModifierType::META_MASK;
    }
    if mask & binds::CTRL != 0 {
        m |= gdk::ModifierType::CONTROL_MASK;
    }
    m
}

fn monitor_named(name: &str) -> Option<gdk::Monitor> {
    let display = gdk::Display::default()?;
    display.monitors().iter::<gdk::Monitor>().flatten().find(|m| m.connector().as_deref() == Some(name))
}

fn app_info(app: &str) -> Option<gio_unix::DesktopAppInfo> {
    if app.starts_with("class:") { None } else { gio_unix::DesktopAppInfo::new(&format!("{app}.desktop")) }
}

fn app_name(item: &Item) -> String {
    app_info(&item.app).map(|i| i.display_name().to_string()).unwrap_or_else(|| item.class.clone())
}

fn app_icon(item: &Item) -> gio::Icon {
    if let Some(icon) = app_info(&item.app).and_then(|i| i.icon()) {
        return icon;
    }
    let theme = gdk::Display::default().map(|d| gtk::IconTheme::for_display(&d));
    let class = item.class.to_lowercase();
    let name = [class.as_str(), class.rsplit('.').next().unwrap_or(&class)]
        .into_iter()
        .find(|n| theme.as_ref().is_some_and(|t| t.has_icon(n)))
        .unwrap_or("application-x-executable");
    gio::ThemedIcon::new(name).upcast()
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
    if desktop.monitors.iter().filter(|m| !m.disabled && !m.is_virtual()).count() > 1 { format!("{ws} · {}", item.monitor) } else { ws }
}

impl Switcher {
    pub fn build(app: &gtk::Application, only: Option<String>) -> Rc<Switcher> {
        let keyboard = std::env::var_os("NYX_SWITCHER_NO_KEYBOARD").is_none();
        let window = gtk::Window::new();
        window.set_application(Some(app));
        window.add_css_class("nyx-switcher");
        window.set_decorated(false);
        window.set_resizable(false);
        window.init_layer_shell();
        window.set_namespace(Some("nyx-switcher"));
        window.set_layer(Layer::Overlay);
        window.set_exclusive_zone(-1);
        window.set_keyboard_mode(if keyboard { KeyboardMode::Exclusive } else { KeyboardMode::None });

        let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
        card.add_css_class("sw-card");
        let grid = gtk::FlowBox::new();
        grid.add_css_class("sw-grid");
        grid.set_selection_mode(gtk::SelectionMode::None);
        grid.set_homogeneous(true);
        grid.set_activate_on_single_click(false);
        grid.set_halign(gtk::Align::Center);
        grid.set_can_focus(false);
        card.append(&grid);
        let caption = gtk::Label::new(None);
        caption.add_css_class("sw-caption");
        caption.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        caption.set_max_width_chars(60);
        let detail = gtk::Label::new(None);
        detail.add_css_class("sw-detail");
        card.append(&caption);
        card.append(&detail);
        window.set_child(Some(&card));

        let sw = Rc::new(Switcher {
            window,
            card,
            grid,
            caption,
            detail,
            tiles: RefCell::default(),
            desktop: RefCell::default(),
            index: RefCell::new(nyx_desktop::apps::from_gio()),
            config: RefCell::new(Config::load()),
            session: RefCell::default(),
            held: Cell::new(gdk::ModifierType::empty()),
            reveal: RefCell::default(),
            guard: RefCell::default(),
            pointer: Cell::new(None),
            pointer_moved: Cell::new(false),
            only,
            keyboard,
            debug: std::env::var_os("NYX_SWITCHER_DEBUG").is_some(),
            opened: Cell::new(None),
            panel: RefCell::default(),
            me: RefCell::default(),
        });
        *sw.me.borrow_mut() = Rc::downgrade(&sw);
        sw.connect_input();

        let w = Rc::downgrade(&sw);
        std::mem::forget(watch::watch_local(move |u| {
            if let Some(sw) = w.upgrade() {
                *sw.desktop.borrow_mut() = u.desktop.clone();
                if u.changes.windows || u.changes.titles {
                    sw.desktop_changed();
                }
            }
        }));
        let w = Rc::downgrade(&sw);
        gio::AppInfoMonitor::get().connect_changed(move |_| {
            if let Some(sw) = w.upgrade() {
                *sw.index.borrow_mut() = nyx_desktop::apps::from_gio();
            }
        });
        // Never open together with the Control Center or Notification Center.
        if let Some(conn) = app.dbus_connection() {
            let (w1, w2, w3) = (Rc::downgrade(&sw), Rc::downgrade(&sw), Rc::downgrade(&sw));
            let handlers = nyx_desktop::panel::Handlers {
                show: Box::new(move || {
                    if let Some(sw) = w1.upgrade() {
                        sw.request(Request { kind: Kind::Overview, sent_us: 0 });
                    }
                }),
                hide: Box::new(move || {
                    if let Some(sw) = w2.upgrade() {
                        sw.cancel();
                    }
                }),
                is_open: Box::new(move || w3.upgrade().is_some_and(|sw| sw.session.borrow().is_some())),
            };
            match nyx_desktop::panel::Panel::serve(&conn, "switcher", handlers) {
                Ok(p) => *sw.panel.borrow_mut() = Some(p),
                Err(e) => eprintln!("nyx-switcher: panel coordination: {e}"),
            }
        }
        sw
    }

    fn weak(&self) -> Weak<Switcher> {
        self.me.borrow().clone()
    }

    fn log(&self, what: &str) {
        if self.debug {
            eprintln!("nyx-switcher: {what}");
        }
    }

    // ---------- Requests ----------

    pub fn request(&self, req: Request) {
        if self.debug && req.sent_us > 0 {
            self.log(&format!("{:?} arrived {:.2} ms after the key", req.kind, (now_us().saturating_sub(req.sent_us)) as f64 / 1000.0));
        }
        match req.kind {
            Kind::Hide => self.cancel(),
            Kind::Overview => {
                if self.session.borrow().is_some() {
                    self.cancel();
                } else {
                    self.open(Mode::Windows, true, true, req.sent_us);
                }
            }
            Kind::Step { mode, forward } => {
                let same = self.session.borrow().as_ref().map(|s| s.mode == mode || (s.expanded.is_some() && mode == Mode::Apps));
                match same {
                    Some(true) => {
                        if let Some(s) = self.session.borrow_mut().as_mut() {
                            s.step(forward);
                        }
                        self.show_selection();
                    }
                    _ => self.open(mode, forward, false, req.sent_us),
                }
            }
        }
    }

    fn filter(&self, app: Option<String>, sticky_all: bool) -> Filter {
        let c = self.config.borrow();
        Filter {
            scope: if sticky_all { Scope::All } else { c.scope },
            show_agent_workspaces: c.show_agent_workspaces,
            app,
        }
    }

    fn items(&self, mode: Mode, app: Option<String>) -> Vec<Item> {
        let desktop = self.desktop.borrow().clone();
        model::items(&desktop, &self.index.borrow(), mode, &self.filter(app, false))
    }

    fn open(&self, mode: Mode, forward: bool, sticky: bool, sent_us: u128) {
        *self.config.borrow_mut() = Config::load();
        let desktop = self.desktop.borrow().clone();
        let items = if sticky {
            model::items(&desktop, &self.index.borrow(), mode, &self.filter(None, true))
        } else {
            self.items(mode, None)
        };
        let Some(session) = Session::start(mode, items, desktop.active_window.as_deref(), forward) else {
            self.log("nothing to switch to");
            self.cancel();
            return;
        };
        let held = if sticky || !self.keyboard {
            gdk::ModifierType::empty()
        } else {
            let keys = &self.config.borrow().keys;
            let key = match mode {
                Mode::Windows => &keys.windows,
                Mode::Apps => &keys.apps,
                Mode::SameApp => &keys.same_app,
            };
            gdk_mods(binds::held(key))
        };
        self.held.set(held);
        *self.session.borrow_mut() = Some(session);
        self.pointer.set(None);
        self.pointer_moved.set(false);
        self.render();

        let monitor = match &self.only {
            Some(name) => monitor_named(name),
            None => desktop.focused_monitor().and_then(|m| monitor_named(&m.name)),
        };
        self.window.set_monitor(monitor.as_ref());
        self.set_columns(monitor.as_ref());
        let delay = if held.is_empty() { 0 } else { self.config.borrow().show_delay_ms };
        if let Some(t) = self.reveal.borrow_mut().take() {
            t.remove();
        }
        if delay > 0 && !self.window.is_visible() {
            self.card.add_css_class("pending");
            let w = self.weak();
            *self.reveal.borrow_mut() = Some(glib::timeout_add_local_once(Duration::from_millis(delay as u64), move || {
                if let Some(sw) = w.upgrade() {
                    sw.reveal.borrow_mut().take();
                    sw.card.remove_css_class("pending");
                }
            }));
        } else {
            self.card.remove_css_class("pending");
        }
        self.card.set_css_classes(&self.card_classes());
        self.opened.set(Some((Instant::now(), sent_us)));
        if self.debug {
            let w = self.weak();
            if let Some(clock) = self.window.frame_clock() {
                let id: Rc<RefCell<Option<glib::SignalHandlerId>>> = Rc::default();
                let id2 = id.clone();
                *id.borrow_mut() = Some(clock.connect_after_paint(move |clock| {
                    if let Some(sw) = w.upgrade()
                        && let Some((_, sent)) = sw.opened.get()
                        && sent > 0
                    {
                        sw.log(&format!("first frame {:.2} ms after the key", (now_us().saturating_sub(sent)) as f64 / 1000.0));
                    }
                    if let Some(h) = id2.borrow_mut().take() {
                        clock.disconnect(h);
                    }
                }));
            }
        }
        self.window.present();
        if let Some(p) = self.panel.borrow().as_ref() {
            p.announce();
        }
        self.start_guard();
    }

    fn card_classes(&self) -> Vec<&'static str> {
        let mut c = vec!["sw-card"];
        if self.card.has_css_class("pending") {
            c.push("pending");
        }
        if self.session.borrow().as_ref().is_some_and(|s| s.mode == Mode::Apps) {
            c.push("apps");
        }
        if self.config.borrow().animation {
            c.push("animated");
        }
        c
    }

    /// Tiles per row: as many as fit in most of the monitor's width.
    fn columns(&self, monitor: Option<&gdk::Monitor>) -> u32 {
        let width = monitor.map(|m| m.geometry().width()).unwrap_or(1280);
        let tile = if self.session.borrow().as_ref().is_some_and(|s| s.mode == Mode::Apps) { APP_TILE } else { WINDOW_TILE };
        let n = self.session.borrow().as_ref().map(|s| s.items.len()).unwrap_or(1) as i32;
        ((width as f64 * 0.8) as i32 / (tile + 8)).clamp(1, 12).min(n.max(1)) as u32
    }

    fn set_columns(&self, monitor: Option<&gdk::Monitor>) {
        let cols = self.columns(monitor);
        // Both bounds, so the card is as wide as its tiles rather than the most that fit.
        self.grid.set_min_children_per_line(cols);
        self.grid.set_max_children_per_line(cols);
    }

    // ---------- Ending ----------

    /// Switch to the selected window and hide.
    pub fn commit(&self) {
        let Some(session) = self.session.borrow_mut().take() else { return };
        self.hide();
        let Some(item) = session.selected().cloned() else { return };
        let current = self.desktop.borrow().focused_monitor().map(|m| m.active_workspace).unwrap_or(1);
        self.log(&format!("switch to {} ({})", item.title, item.address));
        // After the surface lets go of the keyboard, off the main thread.
        std::thread::spawn(move || nyx_desktop::actions::focus(&item.address, item.minimized, current));
    }

    pub fn cancel(&self) {
        if self.session.borrow_mut().take().is_some() {
            self.log("cancelled");
        }
        self.hide();
    }

    fn hide(&self) {
        for slot in [&self.reveal, &self.guard] {
            if let Some(t) = slot.borrow_mut().take() {
                t.remove();
            }
        }
        self.window.set_visible(false);
    }

    /// Whether the shortcut's modifiers are still down in `state`.
    fn still_held(&self, state: gdk::ModifierType) -> bool {
        let held = self.held.get();
        held.is_empty() || state.intersects(held)
    }

    fn keyboard_state(&self) -> Option<gdk::ModifierType> {
        let seat = gdk::Display::default()?.default_seat()?;
        Some(seat.keyboard()?.modifier_state())
    }

    /// The modifier can go up before the surface has the keyboard (a quick tap), and a
    /// release can be missed if focus moves; check GDK's keyboard state while a held
    /// session is open. This reads local state only (no compositor round trip).
    fn start_guard(&self) {
        if self.held.get().is_empty() || self.guard.borrow().is_some() {
            return;
        }
        let w = self.weak();
        *self.guard.borrow_mut() = Some(glib::timeout_add_local(Duration::from_millis(40), move || {
            let Some(sw) = w.upgrade() else { return glib::ControlFlow::Break };
            if sw.session.borrow().is_none() {
                sw.guard.borrow_mut().take();
                return glib::ControlFlow::Break;
            }
            if sw.window.is_active()
                && let Some(state) = sw.keyboard_state()
                && !sw.still_held(state)
            {
                sw.log("modifier up (keyboard state)");
                sw.guard.borrow_mut().take();
                sw.commit();
                return glib::ControlFlow::Break;
            }
            glib::ControlFlow::Continue
        }));
    }

    // ---------- Input ----------

    fn connect_input(&self) {
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let w = self.weak();
        keys.connect_key_pressed(move |_, key, _, state| {
            let Some(sw) = w.upgrade() else { return glib::Propagation::Proceed };
            sw.key(key, state)
        });
        let w = self.weak();
        keys.connect_key_released(move |_, key, _, state| {
            let Some(sw) = w.upgrade() else { return };
            let released = match key {
                gdk::Key::Alt_L | gdk::Key::Alt_R | gdk::Key::Meta_L | gdk::Key::Meta_R => gdk::ModifierType::ALT_MASK,
                gdk::Key::Super_L | gdk::Key::Super_R => gdk::ModifierType::SUPER_MASK | gdk::ModifierType::META_MASK,
                gdk::Key::Control_L | gdk::Key::Control_R => gdk::ModifierType::CONTROL_MASK,
                _ => return,
            };
            // `state` is from before the release.
            let held = sw.held.get();
            if !held.is_empty() && sw.session.borrow().is_some() && !(state - released).intersects(held) {
                sw.log("modifier up (release)");
                sw.commit();
            }
        });
        let w = self.weak();
        keys.connect_modifiers(move |_, state| {
            let Some(sw) = w.upgrade() else { return glib::Propagation::Proceed };
            if sw.session.borrow().is_some() && !sw.still_held(state) {
                sw.log("modifier up (modifiers)");
                sw.commit();
            }
            glib::Propagation::Proceed
        });
        self.window.add_controller(keys);

        let motion = gtk::EventControllerMotion::new();
        let w = self.weak();
        motion.connect_motion(move |_, x, y| {
            let Some(sw) = w.upgrade() else { return };
            match sw.pointer.get() {
                None => sw.pointer.set(Some((x, y))),
                Some((px, py)) if (px - x).abs() + (py - y).abs() > 3.0 => sw.pointer_moved.set(true),
                _ => {}
            }
        });
        self.window.add_controller(motion);

        let w = self.weak();
        self.window.connect_is_active_notify(move |win| {
            let Some(sw) = w.upgrade() else { return };
            if win.is_active() {
                sw.log("has the keyboard");
            }
        });
    }

    fn key(&self, key: gdk::Key, state: gdk::ModifierType) -> glib::Propagation {
        if self.session.borrow().is_none() {
            return glib::Propagation::Proceed;
        }
        let shift = state.contains(gdk::ModifierType::SHIFT_MASK);
        let cols = self.grid.max_children_per_line().max(1) as isize;
        let mode = self.session.borrow().as_ref().map(|s| s.mode);
        let expanded = self.session.borrow().as_ref().is_some_and(|s| s.expanded.is_some());
        match key {
            gdk::Key::Escape => self.cancel(),
            gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::space => self.commit(),
            gdk::Key::Tab | gdk::Key::ISO_Left_Tab => self.move_by(if shift || key == gdk::Key::ISO_Left_Tab { -1 } else { 1 }),
            gdk::Key::grave | gdk::Key::asciitilde => self.move_by(if shift { -1 } else { 1 }),
            gdk::Key::Left | gdk::Key::KP_Left => self.move_by(-1),
            gdk::Key::Right | gdk::Key::KP_Right => self.move_by(1),
            gdk::Key::Down | gdk::Key::KP_Down if mode == Some(Mode::Apps) => self.expand(),
            gdk::Key::Up | gdk::Key::KP_Up if expanded && self.selected_index() < cols as usize => self.collapse(),
            gdk::Key::Down | gdk::Key::KP_Down => self.move_row(cols),
            gdk::Key::Up | gdk::Key::KP_Up => self.move_row(-cols),
            gdk::Key::Home => self.select(0),
            gdk::Key::End => self.select(usize::MAX),
            _ => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    }

    fn selected_index(&self) -> usize {
        self.session.borrow().as_ref().map(|s| s.selected).unwrap_or(0)
    }

    fn move_by(&self, delta: isize) {
        if let Some(s) = self.session.borrow_mut().as_mut() {
            s.move_by(delta);
        }
        self.show_selection();
    }

    /// Up or down a row, staying put at the edges.
    fn move_row(&self, delta: isize) {
        if let Some(s) = self.session.borrow_mut().as_mut() {
            let to = s.selected as isize + delta;
            if to >= 0 && (to as usize) < s.items.len() {
                s.selected = to as usize;
            }
        }
        self.show_selection();
    }

    fn select(&self, index: usize) {
        if let Some(s) = self.session.borrow_mut().as_mut() {
            s.select(index.min(s.items.len().saturating_sub(1)));
        }
        self.show_selection();
    }

    fn expand(&self) {
        let app = self.session.borrow().as_ref().and_then(|s| s.selected().map(|i| i.app.clone()));
        let Some(app) = app else { return };
        let windows = self.items(Mode::Windows, Some(app));
        let changed = self.session.borrow_mut().as_mut().is_some_and(|s| s.expand(windows));
        if changed {
            self.relayout();
        }
    }

    fn collapse(&self) {
        let apps = self.items(Mode::Apps, None);
        let changed = self.session.borrow_mut().as_mut().is_some_and(|s| s.collapse(apps));
        if changed {
            self.relayout();
        }
    }

    fn relayout(&self) {
        let monitor = self.window.monitor().and_downcast::<gdk::Monitor>();
        self.render();
        self.set_columns(monitor.as_ref());
        self.card.set_css_classes(&self.card_classes());
    }

    /// Windows opened, closed or were renamed during a session.
    fn desktop_changed(&self) {
        let (mode, app) = match self.session.borrow().as_ref() {
            Some(s) => (s.mode, s.expanded.clone()),
            None => return,
        };
        let items = self.items(mode, app);
        let alive = self.session.borrow_mut().as_mut().is_some_and(|s| s.refresh(items));
        if alive {
            self.render();
        } else {
            self.cancel();
        }
    }

    // ---------- Drawing ----------

    fn render(&self) {
        let session = self.session.borrow();
        let Some(s) = session.as_ref() else { return };
        while let Some(child) = self.grid.first_child() {
            self.grid.remove(&child);
        }
        let mut tiles = Vec::with_capacity(s.items.len());
        for (i, item) in s.items.iter().enumerate() {
            let tile = if s.mode == Mode::Apps { self.app_tile(item) } else { self.window_tile(item) };
            let child = gtk::FlowBoxChild::new();
            child.set_child(Some(&tile));
            child.set_can_focus(false);
            child.add_css_class("sw-tile");
            if item.urgent {
                child.add_css_class("urgent");
            }
            let click = gtk::GestureClick::new();
            let w = self.weak();
            click.connect_released(move |g, _, _, _| {
                g.set_state(gtk::EventSequenceState::Claimed);
                if let Some(sw) = w.upgrade() {
                    sw.select(i);
                    sw.commit();
                }
            });
            child.add_controller(click);
            let hover = gtk::EventControllerMotion::new();
            let w = self.weak();
            hover.connect_enter(move |_, _, _| {
                if let Some(sw) = w.upgrade()
                    && sw.pointer_moved.get()
                {
                    sw.select(i);
                }
            });
            hover.connect_motion({
                let w = self.weak();
                move |_, _, _| {
                    if let Some(sw) = w.upgrade()
                        && sw.pointer_moved.get()
                        && sw.selected_index() != i
                    {
                        sw.select(i);
                    }
                }
            });
            child.add_controller(hover);
            self.grid.append(&child);
            tiles.push(child);
        }
        drop(session);
        *self.tiles.borrow_mut() = tiles;
        self.show_selection();
    }

    fn app_tile(&self, item: &Item) -> gtk::Widget {
        let overlay = gtk::Overlay::new();
        let icon = gtk::Image::from_gicon(&app_icon(item));
        icon.set_pixel_size(72);
        icon.add_css_class("sw-app-icon");
        overlay.set_child(Some(&icon));
        if item.windows.len() > 1 {
            let count = gtk::Label::new(Some(&item.windows.len().to_string()));
            count.add_css_class("sw-count");
            count.set_halign(gtk::Align::End);
            count.set_valign(gtk::Align::Start);
            overlay.add_overlay(&count);
        }
        overlay.set_size_request(APP_TILE - 16, APP_TILE - 16);
        overlay.set_tooltip_text(Some(&app_name(item)));
        overlay.upcast()
    }

    fn window_tile(&self, item: &Item) -> gtk::Widget {
        let b = gtk::Box::new(gtk::Orientation::Vertical, 6);
        b.set_size_request(WINDOW_TILE - 16, -1);
        let icon = gtk::Image::from_gicon(&app_icon(item));
        icon.set_pixel_size(56);
        icon.add_css_class("sw-app-icon");
        icon.set_margin_top(10);
        b.append(&icon);
        let title = gtk::Label::new(Some(if item.title.is_empty() { &item.class } else { &item.title }));
        title.add_css_class("sw-title");
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title.set_max_width_chars(1);
        title.set_hexpand(true);
        b.append(&title);
        let desktop = self.desktop.borrow().clone();
        let badge = gtk::Label::new(Some(&place(item, &desktop)));
        badge.add_css_class("sw-badge");
        badge.set_ellipsize(gtk::pango::EllipsizeMode::End);
        badge.set_max_width_chars(1);
        badge.set_hexpand(true);
        b.append(&badge);
        b.upcast()
    }

    /// Move the highlight and the caption to the selected entry.
    fn show_selection(&self) {
        let session = self.session.borrow();
        let Some(s) = session.as_ref() else { return };
        for (i, t) in self.tiles.borrow().iter().enumerate() {
            if i == s.selected {
                t.add_css_class("selected");
            } else {
                t.remove_css_class("selected");
            }
        }
        let Some(item) = s.selected() else { return };
        let desktop = self.desktop.borrow().clone();
        if s.mode == Mode::Apps {
            self.caption.set_text(&app_name(item));
            let n = item.windows.len();
            self.detail.set_text(&if n == 1 { "1 window".to_string() } else { format!("{n} windows · ↓ to choose") });
        } else {
            self.caption.set_text(if item.title.is_empty() { &item.class } else { &item.title });
            self.detail.set_text(&format!("{} · {}", app_name(item), place(item, &desktop)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifier_bits_map_to_gdk() {
        assert_eq!(gdk_mods(binds::ALT), gdk::ModifierType::ALT_MASK);
        assert!(gdk_mods(binds::SUPER).contains(gdk::ModifierType::SUPER_MASK));
        assert!(gdk_mods(0).is_empty());
    }
}

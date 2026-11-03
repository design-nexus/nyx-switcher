//! The overview (Super+Alt+W, or the dock's "Show Windows"): every window on every
//! workspace and monitor, grouped by workspace, on a full-screen layer over the focused
//! monitor. It stays open without holding a key: type to search, Enter or a click
//! switches, Esc closes. Each window offers close, float/tile and move.

use crate::ui::{Switcher, app_name, monitor_named};
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use nyx_desktop::actions;
use nyx_switcher_core::model::Item;
use nyx_switcher_core::overview::{self, Group};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

/// Tile width with its margins, for working out how many fit in a row.
const TILE: i32 = 272;

pub struct Overview {
    sw: Weak<Switcher>,
    window: gtk::Window,
    search: gtk::SearchEntry,
    /// The active workspace profile, and a menu to switch (when there are two or more).
    profile_label: gtk::Label,
    profile_menu: gtk::MenuButton,
    body: gtk::FlowBox,
    scroll: gtk::ScrolledWindow,
    /// Only this app's windows (from the dock), until closed.
    app: RefCell<Option<String>>,
    /// The windows on screen, in reading order, with their tiles.
    tiles: RefCell<Vec<(gtk::FlowBoxChild, Item)>>,
    selected: Cell<usize>,
    columns: Cell<usize>,
    me: RefCell<Weak<Overview>>,
}

impl Overview {
    pub fn build(sw: &Switcher) -> Rc<Overview> {
        let window = gtk::Window::new();
        window.set_application(Some(sw.application()));
        window.add_css_class("nyx-overview");
        window.set_decorated(false);
        window.init_layer_shell();
        window.set_namespace(Some("nyx-overview"));
        window.set_layer(Layer::Overlay);
        window.set_exclusive_zone(-1);
        for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            window.set_anchor(edge, true);
        }
        window.set_keyboard_mode(if sw.keyboard() { KeyboardMode::Exclusive } else { KeyboardMode::None });

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("ov-root");
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        head.add_css_class("ov-head");
        head.set_halign(gtk::Align::Center);
        let profile_label = gtk::Label::new(None);
        profile_label.add_css_class("ov-profile");
        let profile_menu = gtk::MenuButton::new();
        profile_menu.set_icon_name("view-paged-symbolic");
        profile_menu.add_css_class("ov-icon-button");
        head.append(&profile_label);
        head.append(&profile_menu);
        let search = gtk::SearchEntry::new();
        search.add_css_class("ov-search");
        search.set_placeholder_text(Some("Search windows"));
        search.set_width_chars(36);
        head.append(&search);
        let close = gtk::Button::from_icon_name("window-close-symbolic");
        close.add_css_class("ov-icon-button");
        close.set_tooltip_text(Some("Close · Esc"));
        head.append(&close);
        root.append(&head);
        // Workspaces as cards, side by side, wrapping.
        let body = gtk::FlowBox::new();
        body.add_css_class("ov-body");
        body.set_selection_mode(gtk::SelectionMode::None);
        body.set_homogeneous(false);
        body.set_valign(gtk::Align::Start);
        body.set_halign(gtk::Align::Center);
        body.set_column_spacing(18);
        body.set_row_spacing(18);
        body.set_max_children_per_line(8);
        body.set_can_focus(false);
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroll.set_vexpand(true);
        scroll.set_child(Some(&body));
        root.append(&scroll);
        window.set_child(Some(&root));

        let o = Rc::new(Overview {
            sw: sw.weak_ref(),
            window,
            search,
            profile_label,
            profile_menu,
            body,
            scroll,
            app: RefCell::default(),
            tiles: RefCell::default(),
            selected: Cell::new(0),
            columns: Cell::new(4),
            me: RefCell::default(),
        });
        *o.me.borrow_mut() = Rc::downgrade(&o);

        let w = o.weak();
        close.connect_clicked(move |_| {
            if let Some(o) = w.upgrade() {
                o.close();
            }
        });
        let w = o.weak();
        o.search.connect_search_changed(move |_| {
            if let Some(o) = w.upgrade() {
                o.selected.set(0);
                o.refresh();
            }
        });
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let w = o.weak();
        keys.connect_key_pressed(move |_, key, _, state| {
            let Some(o) = w.upgrade() else { return glib::Propagation::Proceed };
            o.key(key, state)
        });
        o.window.add_controller(keys);
        // A click on the background (not on a window) closes.
        let click = gtk::GestureClick::new();
        let w = o.weak();
        click.connect_released(move |g, _, _, _| {
            if let Some(o) = w.upgrade()
                && g.current_event_state().is_empty()
            {
                o.close();
            }
        });
        o.body.add_controller(click);
        o
    }

    fn weak(&self) -> Weak<Overview> {
        self.me.borrow().clone()
    }

    pub fn is_open(&self) -> bool {
        self.window.is_visible()
    }

    pub fn open(&self, app: Option<String>) {
        let Some(sw) = self.sw.upgrade() else { return };
        *self.app.borrow_mut() = app;
        self.search.set_text("");
        self.selected.set(0);
        let monitor = match sw.only() {
            Some(name) => monitor_named(name),
            None => sw.desktop().focused_monitor().and_then(|m| monitor_named(&m.name)),
        };
        let width = monitor.as_ref().map(|m| m.geometry().width()).unwrap_or(1280);
        self.columns.set(((width - 96) / TILE).clamp(1, 8) as usize);
        self.window.set_monitor(monitor.as_ref());
        self.show_profiles();
        self.refresh();
        self.window.present();
        self.search.grab_focus();
        let ids: Vec<String> =
            self.tiles.borrow().iter().map(|(_, i)| i.stable_id.clone()).filter(|id| !id.is_empty()).take(40).collect();
        sw.capture(ids);
        sw.debug_log("overview open");
    }

    /// The active workspace profile and the others to switch to (Nyx Settings applies a
    /// profile). Hidden with fewer than two profiles.
    fn show_profiles(&self) {
        let store = nyx_desktop::profiles::Store::user();
        let list = store.list();
        let active = store.active();
        let shown = list.len() >= 2;
        self.profile_label.set_visible(shown);
        self.profile_menu.set_visible(shown);
        if !shown {
            return;
        }
        let name = list.iter().find(|p| active.as_deref() == Some(p.id.as_str())).map(|p| p.name.clone()).unwrap_or_default();
        self.profile_label.set_text(&name);
        self.profile_menu.set_tooltip_text(Some(&format!("Workspace profile: {name}")));
        let menu = gio::Menu::new();
        for p in &list {
            let item = gio::MenuItem::new(Some(&p.name), None);
            item.set_action_and_target_value(Some("ov-profile.switch"), Some(&p.id.to_variant()));
            menu.append_item(&item);
        }
        self.profile_menu.set_menu_model(Some(&menu));
        let group = gio::SimpleActionGroup::new();
        let action = gio::SimpleAction::new_stateful(
            "switch",
            Some(glib::VariantTy::STRING),
            &active.clone().unwrap_or_default().to_variant(),
        );
        let label = self.profile_label.downgrade();
        let names: Vec<(String, String)> = list.iter().map(|p| (p.id.clone(), p.name.clone())).collect();
        action.connect_activate(move |a, v| {
            let Some(id) = v.and_then(|v| v.get::<String>()) else { return };
            a.set_state(&id.to_variant());
            if let (Some(l), Some((_, name))) = (label.upgrade(), names.iter().find(|(i, _)| *i == id)) {
                l.set_text(name);
            }
            // Settings applies it (workspaces, rules, moving windows); the overview
            // follows the windows as they move.
            std::thread::spawn(move || {
                let _ = std::process::Command::new("nyx-settings").args(["--workspace-profile", &id]).output();
            });
        });
        group.add_action(&action);
        self.profile_menu.insert_action_group("ov-profile", Some(&group));
    }

    pub fn close(&self) {
        self.window.set_visible(false);
    }

    /// Build the groups again (windows changed, or the search did), keeping the
    /// selection on the same window where it can.
    pub fn refresh(&self) {
        let Some(sw) = self.sw.upgrade() else { return };
        let keep = self.tiles.borrow().get(self.selected.get()).map(|(_, i)| i.address.clone());
        let query = self.search.text().to_string();
        let mut items = sw.all_windows(self.app.borrow().clone());
        items.retain(|i| overview::matches(i, &app_name(i), &query));
        let desktop = sw.desktop();
        let groups = overview::groups(&desktop, items);
        while let Some(c) = self.body.first_child() {
            self.body.remove(&c);
        }
        let mut tiles = Vec::new();
        if groups.is_empty() {
            let empty = gtk::Label::new(Some(if query.is_empty() { "No windows open" } else { "No matching windows" }));
            empty.add_css_class("ov-empty");
            empty.set_margin_top(120);
            self.body.append(&empty);
        }
        let several_monitors = desktop.monitors.iter().filter(|m| !m.disabled && !m.is_virtual()).count() > 1;
        for g in &groups {
            let card = self.section(&sw, g, several_monitors, &mut tiles);
            let child = gtk::FlowBoxChild::new();
            child.set_can_focus(false);
            child.set_child(Some(&card));
            child.set_valign(gtk::Align::Start);
            self.body.append(&child);
        }
        let selected = keep.and_then(|a| tiles.iter().position(|(_, i)| i.address == a)).unwrap_or(0);
        *self.tiles.borrow_mut() = tiles;
        self.select(selected.min(self.tiles.borrow().len().saturating_sub(1)));
    }

    fn section(&self, sw: &Switcher, g: &Group, several_monitors: bool, tiles: &mut Vec<(gtk::FlowBoxChild, Item)>) -> gtk::Box {
        let section = gtk::Box::new(gtk::Orientation::Vertical, 6);
        section.add_css_class("ov-section");
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        head.add_css_class("ov-section-head");
        let title = gtk::Label::new(Some(&g.title()));
        title.add_css_class("ov-section-title");
        head.append(&title);
        let mut meta = Vec::new();
        if several_monitors && !g.minimized {
            meta.push(g.monitor.clone());
        }
        if g.visible {
            meta.push("on screen".to_string());
        }
        if !meta.is_empty() {
            let m = gtk::Label::new(Some(&meta.join(" · ")));
            m.add_css_class("ov-section-meta");
            head.append(&m);
        }
        if !g.minimized && g.workspace > 0 {
            // The heading goes to that workspace.
            head.set_tooltip_text(Some("Go to this workspace"));
            head.set_cursor_from_name(Some("pointer"));
            let click = gtk::GestureClick::new();
            let (w, ws) = (self.weak(), g.workspace);
            click.connect_released(move |gest, _, _, _| {
                gest.set_state(gtk::EventSequenceState::Claimed);
                if let Some(o) = w.upgrade() {
                    o.close();
                    std::thread::spawn(move || go_to_workspace(ws));
                }
            });
            head.add_controller(click);
        }
        section.append(&head);
        let flow = gtk::FlowBox::new();
        flow.add_css_class("ov-grid");
        flow.set_selection_mode(gtk::SelectionMode::None);
        flow.set_homogeneous(true);
        flow.set_halign(gtk::Align::Start);
        // A workspace with a few windows keeps them in one or two rows.
        let per_row = match g.items.len() {
            0..=3 => g.items.len().max(1),
            4 => 2,
            _ => 3,
        }
        .min(self.columns.get()) as u32;
        flow.set_max_children_per_line(per_row);
        flow.set_min_children_per_line(per_row);
        flow.set_can_focus(false);
        for item in &g.items {
            let index = tiles.len();
            let (tile, frame) = sw.thumb_tile(item);
            // The card already names the workspace.
            if let Some(place) = tile.last_child() {
                place.set_visible(false);
            }
            frame.add_overlay(&self.actions(item));
            let child = gtk::FlowBoxChild::new();
            child.add_css_class("sw-tile");
            child.set_can_focus(false);
            child.set_child(Some(&tile));
            let click = gtk::GestureClick::new();
            let w = self.weak();
            click.connect_released(move |gest, _, _, _| {
                gest.set_state(gtk::EventSequenceState::Claimed);
                if let Some(o) = w.upgrade() {
                    o.select(index);
                    o.activate();
                }
            });
            child.add_controller(click);
            flow.append(&child);
            tiles.push((child, item.clone()));
        }
        section.append(&flow);
        section
    }

    /// Close, float/tile and move, at the top right of a window's thumbnail.
    fn actions(&self, item: &Item) -> gtk::Box {
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        bar.add_css_class("ov-actions");
        bar.set_halign(gtk::Align::End);
        bar.set_valign(gtk::Align::Start);
        let float = gtk::Button::from_icon_name(if item.floating { "view-dual-symbolic" } else { "view-restore-symbolic" });
        float.set_tooltip_text(Some(if item.floating { "Tile · Ctrl+T" } else { "Float · Ctrl+T" }));
        let addr = item.address.clone();
        float.connect_clicked(move |_| run(actions::toggle_floating(&addr)));
        let mv = gtk::MenuButton::new();
        mv.set_icon_name("go-jump-symbolic");
        mv.set_tooltip_text(Some("Move to a workspace or monitor · Ctrl+1…9, Ctrl+M"));
        mv.set_menu_model(Some(&self.move_menu(item)));
        mv.insert_action_group("ov", Some(&move_actions(&item.address)));
        let close = gtk::Button::from_icon_name("window-close-symbolic");
        close.set_tooltip_text(Some("Close · Ctrl+W"));
        let addr = item.address.clone();
        close.connect_clicked(move |_| run(actions::close_window(&addr)));
        let ungroup = gtk::Button::from_icon_name("edit-cut-symbolic");
        ungroup.set_tooltip_text(Some("Take out of the group · Ctrl+G"));
        ungroup.set_visible(item.group.len() > 1);
        let addr = item.address.clone();
        ungroup.connect_clicked(move |_| run(actions::leave_group(&addr)));
        for b in [ungroup.upcast_ref::<gtk::Widget>(), float.upcast_ref(), mv.upcast_ref(), close.upcast_ref()] {
            b.add_css_class("ov-icon-button");
            bar.append(b);
        }
        bar
    }

    /// The workspaces that exist (and the next free one), then the other monitors.
    fn move_menu(&self, item: &Item) -> gio::Menu {
        let menu = gio::Menu::new();
        let Some(sw) = self.sw.upgrade() else { return menu };
        let desktop = sw.desktop();
        let workspaces = gio::Menu::new();
        let mut ids: Vec<i64> = desktop.workspaces.iter().map(|w| w.id).filter(|&id| id > 0).collect();
        ids.sort_unstable();
        let free = (1..).find(|n| !ids.contains(n)).unwrap_or(1);
        for id in &ids {
            if *id != item.workspace {
                workspaces.append(Some(&format!("Workspace {id}")), Some(&format!("ov.workspace(\"{id}\")")));
            }
        }
        workspaces.append(Some(&format!("New workspace ({free})")), Some(&format!("ov.workspace(\"{free}\")")));
        menu.append_section(None, &workspaces);
        let monitors = gio::Menu::new();
        for m in desktop.monitors.iter().filter(|m| !m.disabled && !m.is_virtual() && m.name != item.monitor) {
            monitors.append(Some(&format!("Monitor {}", m.name)), Some(&format!("ov.monitor(\"{}\")", m.name)));
        }
        if monitors.n_items() > 0 {
            menu.append_section(None, &monitors);
        }
        menu
    }

    // ---------- Keys and selection ----------

    fn key(&self, key: gdk::Key, state: gdk::ModifierType) -> glib::Propagation {
        let ctrl = state.contains(gdk::ModifierType::CONTROL_MASK);
        let item = self.tiles.borrow().get(self.selected.get()).map(|(_, i)| i.clone());
        match key {
            gdk::Key::Escape => {
                if self.search.text().is_empty() {
                    self.close();
                } else {
                    self.search.set_text("");
                }
            }
            gdk::Key::Return | gdk::Key::KP_Enter => self.activate(),
            gdk::Key::Tab => self.move_by(1),
            gdk::Key::ISO_Left_Tab => self.move_by(-1),
            gdk::Key::Left | gdk::Key::KP_Left if self.search.text().is_empty() || ctrl => self.move_by(-1),
            gdk::Key::Right | gdk::Key::KP_Right if self.search.text().is_empty() || ctrl => self.move_by(1),
            gdk::Key::Up | gdk::Key::KP_Up => self.move_vertically(false),
            gdk::Key::Down | gdk::Key::KP_Down => self.move_vertically(true),
            gdk::Key::w | gdk::Key::W if ctrl => {
                if let Some(i) = item {
                    run(actions::close_window(&i.address));
                }
            }
            gdk::Key::g | gdk::Key::G if ctrl => {
                if let Some(i) = item.filter(|i| i.group.len() > 1) {
                    run(actions::leave_group(&i.address));
                }
            }
            gdk::Key::t | gdk::Key::T if ctrl => {
                if let Some(i) = item {
                    run(actions::toggle_floating(&i.address));
                }
            }
            gdk::Key::m | gdk::Key::M if ctrl => {
                if let (Some(i), Some(sw)) = (item, self.sw.upgrade()) {
                    let desktop = sw.desktop();
                    let others: Vec<String> = desktop
                        .monitors
                        .iter()
                        .filter(|m| !m.disabled && !m.is_virtual() && m.name != i.monitor)
                        .map(|m| m.name.clone())
                        .collect();
                    if let Some(m) = others.first() {
                        run(actions::move_to_monitor(&i.address, m));
                    }
                }
            }
            _ if ctrl => match key.to_unicode().and_then(|c| c.to_digit(10)).filter(|d| *d >= 1) {
                Some(n) => {
                    if let Some(i) = item {
                        run(actions::move_to_workspace(&i.address, &n.to_string(), false));
                    }
                }
                None => return glib::Propagation::Proceed,
            },
            _ => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    }

    /// The nearest window above or below the selected one, by position on screen.
    fn move_vertically(&self, down: bool) {
        let tiles = self.tiles.borrow();
        let Some(here) = tiles.get(self.selected.get()).and_then(|(t, _)| t.compute_bounds(&self.body)) else { return };
        let (hx, hy) = (here.x() + here.width() / 2.0, here.y() + here.height() / 2.0);
        let best = tiles
            .iter()
            .enumerate()
            .filter_map(|(i, (t, _))| t.compute_bounds(&self.body).map(|r| (i, r.x() + r.width() / 2.0, r.y() + r.height() / 2.0)))
            .filter(|&(_, _, y)| if down { y > hy + 4.0 } else { y < hy - 4.0 })
            .min_by(|a, b| {
                let da = (a.2 - hy).abs() * 3.0 + (a.1 - hx).abs();
                let db = (b.2 - hy).abs() * 3.0 + (b.1 - hx).abs();
                da.total_cmp(&db)
            })
            .map(|(i, _, _)| i);
        drop(tiles);
        if let Some(i) = best {
            self.select(i);
        }
    }

    fn move_by(&self, delta: isize) {
        let n = self.tiles.borrow().len() as isize;
        if n == 0 {
            return;
        }
        let to = (self.selected.get() as isize + delta).clamp(0, n - 1);
        self.select(to as usize);
    }

    fn select(&self, index: usize) {
        self.selected.set(index);
        for (i, (t, _)) in self.tiles.borrow().iter().enumerate() {
            if i == index {
                t.add_css_class("selected");
                // Keep it in view.
                if let Some(rect) = t.compute_bounds(&self.body) {
                    let adj = self.scroll.vadjustment();
                    let (top, bottom) = (rect.y() as f64, (rect.y() + rect.height()) as f64);
                    if top < adj.value() {
                        adj.set_value(top - 24.0);
                    } else if bottom > adj.value() + adj.page_size() {
                        adj.set_value(bottom - adj.page_size() + 24.0);
                    }
                }
            } else {
                t.remove_css_class("selected");
            }
        }
    }

    fn activate(&self) {
        let Some(item) = self.tiles.borrow().get(self.selected.get()).map(|(_, i)| i.clone()) else { return };
        let current = self.sw.upgrade().and_then(|sw| sw.desktop().focused_monitor().map(|m| m.active_workspace)).unwrap_or(1);
        self.close();
        std::thread::spawn(move || actions::focus(&item.address, item.minimized, current));
    }
}

fn run(d: actions::Dispatch) {
    std::thread::spawn(move || {
        d.run();
    });
}

fn go_to_workspace(id: i64) {
    let d = actions::Dispatch { lua: format!("hl.dsp.focus({{ workspace = \"{id}\" }})"), classic: format!("workspace {id}") };
    d.run();
}

/// `ov.workspace("3")` and `ov.monitor("DP-1")` for one window's move menu.
fn move_actions(address: &str) -> gio::SimpleActionGroup {
    let group = gio::SimpleActionGroup::new();
    let ws = gio::SimpleAction::new("workspace", Some(glib::VariantTy::STRING));
    let a = address.to_string();
    ws.connect_activate(move |_, p| {
        if let Some(id) = p.and_then(|p| p.get::<String>()) {
            run(actions::move_to_workspace(&a, &id, false));
        }
    });
    group.add_action(&ws);
    let mon = gio::SimpleAction::new("monitor", Some(glib::VariantTy::STRING));
    let a = address.to_string();
    mon.connect_activate(move |_, p| {
        if let Some(m) = p.and_then(|p| p.get::<String>()) {
            run(actions::move_to_monitor(&a, &m));
        }
    });
    group.add_action(&mon);
    group
}

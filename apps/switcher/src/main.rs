//! Nyx Switcher: window and app switching for Omarchy and Hyprland.
//!
//! `nyx-switcher` runs hidden (normally as a user service). The shortcuts run
//! `nyx-switcher --next windows` and the like, which only hand the request to the
//! running switcher over a socket and exit, before GTK is touched, so a press costs
//! about a millisecond. Without a running switcher they fall back to Hyprland's own
//! window cycling, so Alt+Tab never goes dead.

mod integration;
mod ipc;
mod overview;
mod theme;
mod ui;

use gtk::prelude::*;
use gtk::{gio, glib};
use nyx_switcher_core::config::Config;
use std::cell::RefCell;
use std::process::ExitCode;
use std::rc::Rc;

pub const APP_ID: &str = "io.github.design_nexus.nyx.Switcher";

const USAGE: &str = "Usage: nyx-switcher [--only <output>]
       nyx-switcher --next|--prev windows|apps|same-app
       nyx-switcher --overview [APP] | --hide
       nyx-switcher --integrate [--force] | --unintegrate | --check-keys

With no option, start (hidden). --next/--prev open the running switcher or move its
selection; releasing the shortcut's modifier switches. --overview opens it without
holding a key (Enter or a click switches, Esc closes). --integrate binds the keys
from ~/.config/nyx/switcher.toml in Hyprland (Alt+Tab, Super+Tab, Alt+` and
Super+Alt+W by default); --unintegrate gives them back to Omarchy; --check-keys lists
other bindings on them.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--next" | "--prev") => {
            let mode = args.get(1).map(String::as_str).unwrap_or("windows");
            if nyx_switcher_core::model::Mode::parse(mode).is_none() {
                eprintln!("nyx-switcher: {} takes windows, apps or same-app", args[0]);
                return ExitCode::FAILURE;
            }
            client(&format!("{} {mode}", &args[0][2..]), Some((args[0] == "--next", mode)))
        }
        Some("--overview") => match args.get(1) {
            Some(app) => client(&format!("overview {app}"), None),
            None => client("overview", None),
        },
        Some("--hide") => client("hide", None),
        Some("--integrate") => report(integration::apply(Some(&Config::load().keys), args.iter().any(|a| a == "--force"))),
        Some("--unintegrate") => report(integration::apply(None, true)),
        Some("--check-keys") => match integration::check(&Config::load().keys) {
            Ok(list) => {
                for c in &list {
                    println!("{}: {}{}", c.key, c.other, if c.expected { " (Omarchy's, replaced)" } else { "" });
                }
                if list.iter().any(|c| !c.expected) { ExitCode::FAILURE } else { ExitCode::SUCCESS }
            }
            Err(e) => report(Err(e)),
        },
        Some("-h" | "--help") => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some("--version") => {
            println!("nyx-switcher {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        _ => service(),
    }
}

fn report(r: Result<String, String>) -> ExitCode {
    match r {
        Ok(msg) => {
            if !msg.is_empty() {
                println!("{msg}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("nyx-switcher: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Hand `request` to the running switcher. Without one, a window step falls back to
/// Hyprland's own cycling.
fn client(request: &str, step: Option<(bool, &str)>) -> ExitCode {
    let sent = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_micros()).unwrap_or(0);
    if ipc::send(&format!("{request} {sent}")).is_ok() {
        return ExitCode::SUCCESS;
    }
    if let Some((forward, _)) = step {
        let lua = if forward { "hl.dsp.window.cycle_next()" } else { "hl.dsp.window.cycle_next({ next = false })" };
        let classic = if forward { "cyclenext" } else { "cyclenext prev" };
        if let Ok(h) = nyx_desktop::hypr::Instance::find() {
            if h.dispatch(lua).is_err() {
                let _ = h.dispatch(classic);
            }
            let _ = h.dispatch("hl.dsp.window.bring_to_top()").or_else(|_| h.dispatch("bringactivetotop"));
        }
    }
    eprintln!("nyx-switcher: the switcher isn't running (systemctl --user start nyx-switcher)");
    ExitCode::FAILURE
}

fn service() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let only = args.iter().position(|a| a == "--only").and_then(|i| args.get(i + 1)).cloned();
    // A test copy (`--only <output>`) is separate: its own id and socket.
    let (id, flags) = if only.is_some() {
        if std::env::var_os("NYX_SWITCHER_SOCKET").is_none() {
            // SAFETY: still single-threaded; nothing else reads the environment yet.
            unsafe { std::env::set_var("NYX_SWITCHER_SOCKET", ipc::default_path().with_file_name("nyx-switcher-test.sock")) };
        }
        (format!("{APP_ID}.Test"), gio::ApplicationFlags::NON_UNIQUE)
    } else {
        (APP_ID.to_string(), gio::ApplicationFlags::empty())
    };
    glib::set_prgname(Some(id.as_str()));
    // The GL renderer maps a first frame sooner than Vulkan's start-up.
    if std::env::var_os("GSK_RENDERER").is_none() {
        // SAFETY: as above.
        unsafe { std::env::set_var("GSK_RENDERER", "ngl") };
    }
    let app = gtk::Application::builder().application_id(id).flags(flags | gio::ApplicationFlags::HANDLES_COMMAND_LINE).build();
    let switcher: Rc<RefCell<Option<Rc<ui::Switcher>>>> = Rc::default();
    let s = switcher.clone();
    app.connect_startup(move |app| {
        theme::install();
        // Resident: hiding the switcher doesn't quit.
        std::mem::forget(app.hold());
        let sw = ui::Switcher::build(app, only.clone());
        let weak = Rc::downgrade(&sw);
        if let Err(e) = ipc::listen(move |req| {
            if let Some(sw) = weak.upgrade() {
                sw.request(req);
            }
        }) {
            eprintln!("nyx-switcher: {e}");
            std::process::exit(1);
        }
        *s.borrow_mut() = Some(sw);
    });
    // A second start (the service again, or by hand) just finds this one running.
    app.connect_command_line(|_, _| glib::ExitCode::SUCCESS);
    let code = app.run_with_args::<String>(&[]);
    ExitCode::from(code.get())
}

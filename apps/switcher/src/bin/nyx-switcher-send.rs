//! What the shortcuts run: `nyx-switcher-send next windows`, `prev apps`, `overview`
//! (optionally one app's windows: `overview <desktop id>`), `hide`. It only writes the request to the running switcher's socket, without
//! loading GTK, so a press costs well under a millisecond. Without a running switcher,
//! a window step falls back to Hyprland's own cycling, so Alt+Tab never goes dead.

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

fn socket() -> PathBuf {
    if let Some(p) = std::env::var_os("NYX_SWITCHER_SOCKET") {
        return PathBuf::from(p);
    }
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    dir.join("nyx-switcher.sock")
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let request = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        [w @ ("next" | "prev"), m @ ("windows" | "apps" | "same-app")] => format!("{w} {m}"),
        ["overview"] => "overview".into(),
        ["overview", app] if !app.is_empty() && !app.contains(char::is_whitespace) => format!("overview {app}"),
        ["hide"] => "hide".into(),
        _ => {
            eprintln!("Usage: nyx-switcher-send next|prev windows|apps|same-app | overview [APP] | hide");
            return ExitCode::FAILURE;
        }
    };
    let sent = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_micros()).unwrap_or(0);
    let ok = UnixStream::connect(socket()).and_then(|mut s| {
        s.set_write_timeout(Some(Duration::from_millis(200)))?;
        s.write_all(format!("{request} {sent}\n").as_bytes())
    });
    if ok.is_ok() {
        return ExitCode::SUCCESS;
    }
    if let [w @ ("next" | "prev"), _] = args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        let forward = w == "next";
        if let Ok(h) = nyx_desktop::hypr::Instance::find() {
            let lua = if forward { "hl.dsp.window.cycle_next()" } else { "hl.dsp.window.cycle_next({ next = false })" };
            if h.dispatch(lua).is_err() {
                let _ = h.dispatch(if forward { "cyclenext" } else { "cyclenext prev" });
            }
            let _ = h.dispatch("hl.dsp.window.bring_to_top()").or_else(|_| h.dispatch("bringactivetotop"));
        }
    }
    eprintln!("nyx-switcher-send: the switcher isn't running (systemctl --user start nyx-switcher)");
    ExitCode::FAILURE
}

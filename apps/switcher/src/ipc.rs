//! The socket the shortcuts talk to: `$XDG_RUNTIME_DIR/nyx-switcher.sock` (or
//! `NYX_SWITCHER_SOCKET`). One line per request: `next windows <sent µs>`,
//! `prev apps …`, `overview …`, `hide …`.

use gtk::glib;
use nyx_switcher_core::model::Mode;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Step { mode: Mode, forward: bool },
    Overview,
    Hide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Request {
    pub kind: Kind,
    /// When the client sent it (µs since the epoch), for latency logs.
    pub sent_us: u128,
}

pub fn parse(line: &str) -> Option<Request> {
    let mut words = line.split_whitespace();
    let kind = match words.next()? {
        w @ ("next" | "prev") => Kind::Step { mode: Mode::parse(words.next()?)?, forward: w == "next" },
        "overview" => Kind::Overview,
        "hide" => Kind::Hide,
        _ => return None,
    };
    let sent_us = words.next().and_then(|w| w.parse().ok()).unwrap_or(0);
    Some(Request { kind, sent_us })
}

pub fn default_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    dir.join("nyx-switcher.sock")
}

pub fn path() -> PathBuf {
    std::env::var_os("NYX_SWITCHER_SOCKET").map(PathBuf::from).unwrap_or_else(default_path)
}

pub fn send(line: &str) -> std::io::Result<()> {
    let mut s = UnixStream::connect(path())?;
    s.set_write_timeout(Some(Duration::from_millis(200)))?;
    s.write_all(line.as_bytes())?;
    s.write_all(b"\n")
}

/// Listen on the main loop; `on_request` runs there for each request.
pub fn listen(on_request: impl Fn(Request) + 'static) -> Result<(), String> {
    let path = path();
    if UnixStream::connect(&path).is_ok() {
        return Err(format!("another switcher is already listening on {}", path.display()));
    }
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    // Accept on a small thread; requests reach the main loop through a channel.
    let (tx, rx) = async_channel::unbounded::<Request>();
    std::thread::Builder::new()
        .name("nyx-switcher-ipc".into())
        .spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
                let mut text = String::new();
                let _ = stream.read_to_string(&mut text);
                for req in text.lines().filter_map(parse) {
                    if tx.send_blocking(req).is_err() {
                        return;
                    }
                }
            }
        })
        .map_err(|e| e.to_string())?;
    glib::MainContext::default().spawn_local(async move {
        while let Ok(req) = rx.recv().await {
            on_request(req);
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_requests() {
        assert_eq!(parse("next windows 12").unwrap(), Request { kind: Kind::Step { mode: Mode::Windows, forward: true }, sent_us: 12 });
        assert_eq!(parse("prev same-app").unwrap().kind, Kind::Step { mode: Mode::SameApp, forward: false });
        assert_eq!(parse("overview 5").unwrap().kind, Kind::Overview);
        assert!(parse("next galaxy").is_none());
        assert!(parse("").is_none());
    }
}

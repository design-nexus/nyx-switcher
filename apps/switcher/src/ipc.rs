//! The socket the shortcuts talk to: `$XDG_RUNTIME_DIR/nyx-switcher.sock` (or
//! `NYX_SWITCHER_SOCKET`). One line per request: `next windows <sent µs>`,
//! `prev apps …`, `overview …`, `hide …`.

use nyx_switcher_core::model::Mode;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Step { mode: Mode, forward: bool },
    /// All windows, or one app's (its desktop id or `class:<class>`).
    Overview { app: Option<String> },
    Hide,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub kind: Kind,
    /// When the client sent it (µs since the epoch), for latency logs.
    pub sent_us: u128,
}

pub fn parse(line: &str) -> Option<Request> {
    let mut words: Vec<&str> = line.split_whitespace().collect();
    // The client's timestamp comes last.
    let sent_us = match words.last().and_then(|w| w.parse::<u128>().ok()) {
        Some(t) if words.len() > 1 => {
            words.pop();
            t
        }
        _ => 0,
    };
    let kind = match words[..] {
        [w @ ("next" | "prev"), m] => Kind::Step { mode: Mode::parse(m)?, forward: w == "next" },
        ["overview"] => Kind::Overview { app: None },
        ["overview", app] => Kind::Overview { app: Some(app.to_string()) },
        ["hide"] => Kind::Hide,
        _ => return None,
    };
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
        assert_eq!(parse("overview 5").unwrap().kind, Kind::Overview { app: None });
        assert_eq!(parse("overview org.mozilla.firefox 5").unwrap(), Request { kind: Kind::Overview { app: Some("org.mozilla.firefox".into()) }, sent_us: 5 });
        assert!(parse("next windows extra 5").is_none());
        assert!(parse("next galaxy").is_none());
        assert!(parse("").is_none());
    }
}

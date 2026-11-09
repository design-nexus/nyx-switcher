//! Window thumbnails for the Quickshell interface: captured off the main thread (as the
//! GTK switcher did), written as PPM files under `$XDG_RUNTIME_DIR/nyx-switcher/shots`,
//! and listed by stable id as each one lands. A window is captured again at most every
//! [`RECAPTURE`] on losing focus.

use serde_json::{Map, Value, json};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

const RECAPTURE: Duration = Duration::from_millis(800);

#[derive(Default)]
pub struct Shots {
    paths: Rc<RefCell<HashMap<String, String>>>,
    captured: RefCell<HashMap<String, Instant>>,
}

fn dir() -> PathBuf {
    glib::user_runtime_dir().join("nyx-switcher/shots")
}

fn file(id: &str) -> PathBuf {
    dir().join(format!("{}.ppm", id.replace('/', "_")))
}

impl Shots {
    pub fn all(&self) -> Value {
        Value::Object(self.paths.borrow().iter().map(|(k, v)| (k.clone(), json!(v))).collect::<Map<_, _>>())
    }

    pub fn recent(&self, id: &str) -> bool {
        self.captured.borrow().get(id).is_some_and(|t| t.elapsed() < RECAPTURE)
    }

    /// Drop the thumbnails of windows that closed. Whether any went.
    pub fn forget(&self, alive: &HashSet<String>) -> bool {
        self.captured.borrow_mut().retain(|id, _| alive.contains(id));
        let gone: Vec<String> = self.paths.borrow().keys().filter(|id| !alive.contains(*id)).cloned().collect();
        for id in &gone {
            self.paths.borrow_mut().remove(id);
            let _ = std::fs::remove_file(file(id));
        }
        !gone.is_empty()
    }

    /// Capture `ids` at most `w`×`h` pixels; `updated` gets the whole list after each.
    pub fn capture(&self, ids: Vec<String>, w: u32, h: u32, updated: impl Fn(Value) + 'static) {
        let now = Instant::now();
        for id in &ids {
            self.captured.borrow_mut().insert(id.clone(), now);
        }
        let (tx, rx) = async_channel::unbounded::<(String, String)>();
        std::thread::spawn(move || {
            let _ = std::fs::create_dir_all(dir());
            let _ = nyx_desktop::capture::capture_blocking(&ids, w, h, |shot| {
                let path = file(&shot.stable_id);
                let mut data = format!("P6\n{} {}\n255\n", shot.width, shot.height).into_bytes();
                data.reserve(shot.width as usize * shot.height as usize * 3);
                let stride = shot.stride();
                for row in 0..shot.height as usize {
                    let line = &shot.data[row * stride..row * stride + shot.width as usize * 4];
                    for px in line.as_chunks::<4>().0 {
                        data.extend_from_slice(&[px[2], px[1], px[0]]);
                    }
                }
                let tmp = path.with_extension("ppm.tmp");
                if std::fs::write(&tmp, &data).and_then(|_| std::fs::rename(&tmp, &path)).is_ok() {
                    let _ = tx.send_blocking((shot.stable_id.clone(), format!("{}?{}", path.display(), glib::real_time())));
                }
            });
        });
        let paths = self.paths.clone();
        glib::spawn_future_local(async move {
            while let Ok((id, at)) = rx.recv().await {
                paths.borrow_mut().insert(id, at);
                let all = Value::Object(paths.borrow().iter().map(|(k, v)| (k.clone(), json!(v))).collect::<Map<_, _>>());
                updated(all);
            }
        });
    }
}

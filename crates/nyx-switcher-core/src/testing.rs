//! A desktop to test against: two monitors, a few workspaces, windows most recent first.

use nyx_desktop::{Desktop, Monitor, Window, Workspace};

fn monitor(id: i64, name: &str, focused: bool, active: i64) -> Monitor {
    Monitor {
        id,
        name: name.into(),
        description: String::new(),
        make: String::new(),
        model: String::new(),
        serial: String::new(),
        width: 2560,
        height: 1600,
        x: id as i32 * 2560,
        y: 0,
        scale: 1.0,
        transform: 0,
        focused,
        disabled: false,
        active_workspace: active,
        special_workspace: None,
        reserved: [0; 4],
        refresh_rate: 60.0,
    }
}

fn workspace(id: i64, name: &str, monitor: &str) -> Workspace {
    Workspace { id, name: name.into(), monitor: monitor.into(), windows: 1, has_fullscreen: false, tiled_layout: "dwindle".into() }
}

pub fn window(n: i32, class: &str, title: &str, workspace: i64, monitor: i64) -> Window {
    Window {
        address: format!("a{n}"),
        stable_id: format!("s{n}"),
        class: class.into(),
        initial_class: class.into(),
        title: title.into(),
        workspace,
        monitor,
        floating: false,
        fullscreen: 0,
        at: [0, 0],
        size: [800, 600],
        pid: n,
        focus_order: 0,
        pinned: false,
        hidden: false,
        urgent: false,
        grouped: Vec::new(),
    }
}

pub fn desktop() -> Desktop {
    let mut windows = vec![
        window(1, "Alacritty", "term 1", 1, 0),
        window(2, "firefox", "web 1", 1, 0),
        window(3, "Alacritty", "term 2", 2, 0),
        window(5, "kitty", "scratch", -98, 0),
        window(4, "firefox", "web 2", 11, 1),
        window(6, "firefox", "agent browser", -97, 0),
        window(7, "notes", "minimized notes", -99, 0),
    ];
    for (i, w) in windows.iter_mut().enumerate() {
        w.focus_order = i as i32;
    }
    // Term 1 and web 1 (both on workspace 1) are tabs of one group; term 1 shows.
    windows[0].grouped = vec!["a1".into(), "a2".into()];
    windows[1].grouped = vec!["a1".into(), "a2".into()];
    windows[1].hidden = true;
    Desktop {
        monitors: vec![monitor(0, "eDP-1", true, 1), monitor(1, "DP-1", false, 11)],
        workspaces: vec![
            workspace(-99, "special:minimized", "eDP-1"),
            workspace(-98, "special:scratchpad", "eDP-1"),
            workspace(-97, "special:nyx-agent-1", "eDP-1"),
            workspace(1, "1", "eDP-1"),
            workspace(2, "2", "eDP-1"),
            workspace(11, "11", "DP-1"),
        ],
        windows,
        active_window: Some("a1".into()),
        connected: true,
        ..Desktop::default()
    }
}

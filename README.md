# Switcher

Window and app switching for [Omarchy](https://omarchy.org) and Hyprland. Hold a key, tap
Tab to choose, let go to switch. It follows your Omarchy theme.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/design-nexus/nyx-switcher/main/install.sh | bash
```

It runs as a user service (`nyx-switcher.service`) and binds its keys in Hyprland.
`./uninstall.sh` removes it and gives the keys back to Omarchy.

## Keys

| Keys | What it does |
|---|---|
| Alt+Tab, Alt+Shift+Tab | Every window as a thumbnail, most recently used first |
| Super+Tab, Super+Shift+Tab | One entry per app; ↓ shows the selected app's windows, ↑ goes back |
| Alt+\`, Alt+Shift+\` | The windows of the app you're in |
| Super+Alt+W | The overview: every window, grouped by workspace |

While the switcher is open: Tab and Shift+Tab or the arrow keys move, Home and End
jump to either end, Enter switches, Esc closes without switching, and a click switches
to that window. Type to narrow the list by title or app name (Backspace widens it
again), and Delete closes the selected window.

Windows grouped as tabs in Hyprland show once, as their most recent tab with a tab
count; ↓ lists the tabs and ↑ goes back. Turn this off with `collapse_groups = false`. A quick tap goes straight back to the
previous window without showing anything.

## Overview

Super+Alt+W shows every window on every workspace and monitor, grouped by workspace,
without holding a key. Type to search; the arrow keys or Tab move; Enter or a click
goes to a window; Esc clears the search, then closes. Click a workspace's name to go
to it. Each window has buttons, and keys, to act on it without going there:

| Button | Key | |
|---|---|---|
| Hide the extra titlebar | | for a floating window whose app draws its own titlebar: Omarchy's floating titlebar is left off for that app (Nyx Settings keeps the list) |
| Take out of the group | Ctrl+G | for a tab of a group; it tiles beside the group |
| Float / Tile | Ctrl+T | switch between floating and tiled |
| Move | Ctrl+1…9, Ctrl+M | to another workspace (silently), or to the next monitor |
| Close | Ctrl+W | the app may still ask about unsaved work |

Tabs of one group sit side by side, each marked with its place in the group.

With two or more workspace profiles (Nyx Settings → Workspace Profiles), the
overview shows the active one beside the search; its menu switches to another.

Nyx Dock's right-click menu has Show windows, which opens the overview on that app's
windows (`nyx-switcher-send overview <desktop id>`).

Switching to a window on another workspace or monitor goes there; the window stays
where it is in its layout. A minimized window comes back to the workspace you're on.

Omarchy binds Alt+Tab to cycling windows and Super+Tab to the next workspace. The
switcher takes those keys while it's installed; Super+Ctrl+Tab still goes to the
previous workspace.

## Settings

Nyx Settings → Desktop → Switcher, or `~/.config/nyx/switcher.toml`. Every key is
optional, and changes apply the next time the switcher opens.

```toml
scope = "all"                 # "all", "monitor" (the focused one) or "workspace" (the one in front)
show_agent_workspaces = false # also list windows on agent workspaces (named nyx-agent…)
thumbnails = true             # window thumbnails (false: app icons)
thumbnail_size = "medium"     # "small", "medium" or "large"
collapse_groups = true        # one entry per group of tabs in the window switcher
gesture = ""                  # a touchpad gesture for the overview: "5 up", "5 down", "4 pinchout", …
show_delay_ms = 80            # how long a key is held before the switcher appears
animation = true

[keys]                        # "" leaves one unbound
windows = "ALT + TAB"
windows_reverse = "ALT + SHIFT + TAB"
apps = "SUPER + TAB"
apps_reverse = "SUPER + SHIFT + TAB"
same_app = "ALT + grave"
same_app_reverse = "ALT + SHIFT + grave"
overview = "SUPER + ALT + W"
```

After changing `[keys]` or `gesture`, run `nyx-switcher --integrate`. It refuses a key
another binding already uses, or a gesture another `hl.gesture` in
`~/.config/hypr/*.lua` already uses (`nyx-switcher --check-keys` lists them), unless
you add `--force`. Hyprland itself refuses a gesture that an earlier one overshadows,
and the change is then undone.

Special workspaces (scratchpad, Quake console) aren't listed; minimized windows are.

Thumbnails are taken when a window loses focus and refreshed when the switcher opens,
so nothing is captured while you aren't switching. Windows on other workspaces show
their last frame.

## Commands

```sh
nyx-switcher --check-keys       # other bindings on the switcher's keys
nyx-switcher --integrate        # bind the keys from switcher.toml
nyx-switcher --unintegrate      # give them back to Omarchy
nyx-switcher-send next windows  # what the keys run: next|prev windows|apps|same-app, overview [APP], hide
```

If the service isn't running, `nyx-switcher-send next windows` falls back to
Hyprland's own window cycling, so Alt+Tab keeps working.

## Build

```sh
cargo test --locked --workspace
cargo build --release --locked -p nyx-switcher -p nyx-switcher-backend
```

The switcher is drawn in Quickshell by default: `nyx-switcher-backend` keeps the
shortcuts' socket and the switching session, and `qml/Switcher` draws the card and the
overview in the shared Nyx runtime (`nyx-shell-qs`; `qml/runtime` is a copy of it, from
nyx-shell's `tools/vendor-runtime.sh`). `nyx-switcher --ui gtk` goes back to the GTK
switcher, `--ui qml` returns, `--ui` alone says which is on. To try it on a test output,
run `NYX_SWITCHER_SOCKET=/tmp/sw.sock NYX_SWITCHER_UI_SOCKET=/tmp/sw-ui.sock
NYX_SWITCHER_ONLY=HEADLESS-1 nyx-switcher-backend` and load `qml/Switcher` in `qs` with
the same `NYX_SWITCHER_UI_SOCKET`, `NYX_SHELL_SCREEN=HEADLESS-1` and
`NYX_SHELL_NO_KEYBOARD=1`, then send `next windows` or `overview` to the socket.
`NYX_SWITCHER_DEBUG=1` logs how long after the key the request arrived and the card
showed.

## License

MIT

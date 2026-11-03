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
| Super+Alt+W | All windows, without holding a key: Enter or a click switches |

While the switcher is open: Tab and Shift+Tab or the arrow keys move, Home and End
jump to either end, Enter switches, Esc closes without switching, and a click switches
to that window. A quick tap goes straight back to the previous window without
showing anything.

Switching to a window on another workspace or monitor goes there; the window stays
where it is in its layout. A minimized window comes back to the workspace you're on.

Omarchy binds Alt+Tab to cycling windows and Super+Tab to the next workspace. The
switcher takes those keys while it's installed; Super+Ctrl+Tab still goes to the
previous workspace.

## Settings

`~/.config/nyx/switcher.toml`. Every key is optional, and changes apply the next time
the switcher opens.

```toml
scope = "all"                 # "all", "monitor" (the focused one) or "workspace" (the one in front)
show_agent_workspaces = false # also list windows on agent workspaces (named nyx-agent…)
thumbnails = true             # window thumbnails (false: app icons)
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

After changing `[keys]`, run `nyx-switcher --integrate`. It refuses a key another
binding already uses (`nyx-switcher --check-keys` lists them) unless you add `--force`.

Special workspaces (scratchpad, Quake console) aren't listed; minimized windows are.

Thumbnails are taken when a window loses focus and refreshed when the switcher opens,
so nothing is captured while you aren't switching. Windows on other workspaces show
their last frame.

## Commands

```sh
nyx-switcher --check-keys       # other bindings on the switcher's keys
nyx-switcher --integrate        # bind the keys from switcher.toml
nyx-switcher --unintegrate      # give them back to Omarchy
nyx-switcher-send next windows  # what the keys run: next|prev windows|apps|same-app, overview, hide
```

If the service isn't running, `nyx-switcher-send next windows` falls back to
Hyprland's own window cycling, so Alt+Tab keeps working.

## Build

```sh
cargo test --locked --workspace
cargo build --release --locked -p nyx-switcher
```

## License

MIT

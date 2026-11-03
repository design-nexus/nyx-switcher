# Switcher

Application source: apps/switcher (GTK, layer-shell; `src/bin/nyx-switcher-send.rs` is
the std-only sender the shortcuts run). Logic without GTK: crates/nyx-switcher-core
(settings, which windows to list, the switching session, the Hyprland shortcut block) —
test it there. Desktop state, app matching, window actions and thumbnails come from
nyx-desktop in nyx-core.

Commands: `cargo test --locked --workspace`; `cargo clippy --workspace --all-targets`;
`./install.sh`. Try it without touching the real switcher:
`NYX_SWITCHER_SOCKET=<scratch>/sw.sock NYX_SWITCHER_NO_KEYBOARD=1 nyx-switcher --only HEADLESS-<n>`,
then `nyx-switcher-send next windows` with the same socket (`NYX_SWITCHER_DEBUG=1`
prints decisions and latency). Never inject input or steal focus; releasing a held
modifier can only be tried by the user.

The shortcut block in hyprland.lua is owned like Clipboard's: only the exact block
written last is replaced, a rejected config is rolled back, and --unintegrate gives the
keys back to Omarchy. Keep the switcher's descriptions stable: conflict checks use them.

Use the public, pinned shared crates; never import another application's internals.
Use canonical STYLE.md from the pinned nyx-design dependency. Reuse existing
widgets, theme tokens, icons, and shortcuts.
Common guidance: https://github.com/design-nexus/nyx/blob/main/docs/AGENTS_COMMON.md

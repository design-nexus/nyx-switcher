//! The switcher's logic, kept apart from GTK so it can be tested on its own.
//!
//! - `config`: `~/.config/nyx/switcher.toml`.
//! - `model`: which windows (or apps) to list, most recent first, by scope.
//! - `session`: one switch, from the first press to the release.
//! - `binds`: the Hyprland shortcuts: the block written to `hyprland.lua`, the
//!   modifiers a shortcut holds, and conflicts with other bindings.

pub mod binds;
pub mod config;
pub mod model;
pub mod session;
#[cfg(test)]
mod testing;

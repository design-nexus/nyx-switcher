//! The shortcuts: a block the switcher owns at the end of `~/.config/hypr/hyprland.lua`
//! (as Glimpse and Clipboard do), which modifiers a shortcut holds, and which other
//! bindings it would replace.
//!
//! Each line unbinds the key first, so Omarchy's own Alt+Tab (cycle windows) and
//! Super+Tab (next workspace) step aside while the block is there and come back when
//! it's removed.

use crate::config::Keys;
use serde::Deserialize;

pub const START: &str = "-- BEGIN NYX SWITCHER";
pub const END: &str = "-- END NYX SWITCHER";

/// Hyprland's modifier bits (`hyprctl -j binds` → `modmask`).
pub const SHIFT: u32 = 1;
pub const CTRL: u32 = 4;
pub const ALT: u32 = 8;
pub const SUPER: u32 = 64;

/// Omarchy bindings the default keys take over on purpose (by their description).
pub const REPLACED: &[&str] = &[
    "Focus on next window",
    "Focus on previous window",
    "Reveal active window on top",
    "Next workspace",
    "Previous workspace",
];

/// One shortcut: the key, its description in Hyprland, and the command it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bind {
    pub key: String,
    pub description: &'static str,
    pub command: &'static str,
}

/// The bound shortcuts (empty keys left out).
pub fn binds(keys: &Keys) -> Vec<Bind> {
    let all = [
        (&keys.windows, "Switch windows", "nyx-switcher-send next windows"),
        (&keys.windows_reverse, "Switch windows backwards", "nyx-switcher-send prev windows"),
        (&keys.apps, "Switch apps", "nyx-switcher-send next apps"),
        (&keys.apps_reverse, "Switch apps backwards", "nyx-switcher-send prev apps"),
        (&keys.same_app, "Switch windows of this app", "nyx-switcher-send next same-app"),
        (&keys.same_app_reverse, "Switch windows of this app backwards", "nyx-switcher-send prev same-app"),
        (&keys.overview, "Show all windows", "nyx-switcher-send overview"),
    ];
    all.into_iter()
        .filter(|(k, _, _)| !k.trim().is_empty())
        .map(|(k, description, command)| Bind { key: normalize(k), description, command })
        .collect()
}

/// `alt+tab` → `ALT + TAB` (modifiers upper case, the key as given).
pub fn normalize(key: &str) -> String {
    let parts: Vec<&str> = key.split('+').map(str::trim).filter(|p| !p.is_empty()).collect();
    let n = parts.len();
    parts
        .iter()
        .enumerate()
        .map(|(i, p)| if i + 1 < n || modifier(p).is_some() { p.to_uppercase() } else { p.to_string() })
        .collect::<Vec<_>>()
        .join(" + ")
}

fn modifier(word: &str) -> Option<u32> {
    match word.to_uppercase().as_str() {
        "SHIFT" => Some(SHIFT),
        "CTRL" | "CONTROL" => Some(CTRL),
        "ALT" | "MOD1" => Some(ALT),
        "SUPER" | "WIN" | "LOGO" | "MOD4" | "META" => Some(SUPER),
        _ => None,
    }
}

/// A key as Omarchy writes it (`SUPER + TAB`): its modifier mask and the key.
pub fn parse(key: &str) -> Option<(u32, String)> {
    let parts: Vec<&str> = key.split('+').map(str::trim).collect();
    let (last, mods) = parts.split_last()?;
    if last.is_empty() || modifier(last).is_some() {
        return None;
    }
    let mut mask = 0;
    for m in mods {
        mask |= modifier(m)?;
    }
    Some((mask, last.to_string()))
}

/// Whether Hyprland would take `key`, and it's safe to write into Lua.
pub fn valid_key(key: &str) -> bool {
    parse(key).is_some()
        && key.split('+').count() <= 5
        && key.split('+').map(str::trim).all(|p| p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':'))
}

/// The modifiers to keep holding while switching: those of the shortcut, except Shift
/// (which only turns the direction around). Releasing them all switches.
pub fn held(key: &str) -> u32 {
    parse(key).map(|(m, _)| m & !SHIFT).unwrap_or(0)
}

fn lua_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The block for `keys`. Keys that aren't valid are left out.
pub fn block(keys: &Keys) -> String {
    let mut out = format!("{START}\n-- Owned by Switcher. Change the keys in Settings → Desktop → Switcher.\n");
    for b in binds(keys).into_iter().filter(|b| valid_key(&b.key)) {
        out.push_str(&format!(
            "hl.unbind({k})\no.bind({k}, {d}, {c})\n",
            k = lua_string(&b.key),
            d = lua_string(b.description),
            c = lua_string(b.command)
        ));
    }
    out.push_str(END);
    out.push('\n');
    out
}

/// `text` with the owned block replaced by `new` (removed when `None`). `old` is the
/// block as last written; a block that differs from it was edited by hand and is left
/// alone.
pub fn edit(text: &str, old: Option<&str>, new: Option<&str>) -> Result<String, String> {
    let starts = text.matches(START).count();
    let ends = text.matches(END).count();
    if starts == 0 && ends == 0 {
        return Ok(match new {
            Some(s) => format!("{}{}{}", text, if text.ends_with('\n') || text.is_empty() { "" } else { "\n" }, s),
            None => text.into(),
        });
    }
    if starts != 1 || ends != 1 {
        return Err("hyprland.lua has more than one Switcher block".into());
    }
    let start = text.find(START).unwrap_or(0);
    let end = text.find(END).map(|e| e + END.len()).unwrap_or(0);
    if start >= end {
        return Err("the Switcher block in hyprland.lua is malformed".into());
    }
    let end = if text.as_bytes().get(end) == Some(&b'\n') { end + 1 } else { end };
    match old {
        Some(o) if &text[start..end] == o => {}
        _ => return Err("the Switcher block in hyprland.lua was edited by hand; leaving it alone".into()),
    }
    Ok(format!("{}{}{}", &text[..start], new.unwrap_or(""), &text[end..]))
}

/// A binding as `hyprctl -j binds` lists it.
#[derive(Debug, Clone, Deserialize)]
pub struct Existing {
    pub modmask: u32,
    pub key: String,
    #[serde(default)]
    pub submap: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub dispatcher: String,
    #[serde(default)]
    pub arg: String,
}

/// Another binding on one of our keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub key: String,
    /// What the other binding does (its description, else dispatcher and argument).
    pub other: String,
    /// One of Omarchy's that the switcher takes over on purpose.
    pub expected: bool,
}

/// Other bindings on `keys`, from `hyprctl -j binds` output. Our own (by description)
/// aren't conflicts.
pub fn conflicts(binds_json: &str, keys: &Keys) -> Result<Vec<Conflict>, String> {
    let existing: Vec<Existing> = serde_json::from_str(binds_json).map_err(|e| e.to_string())?;
    let ours: Vec<Bind> = binds(keys);
    let own: Vec<&str> = ours.iter().map(|b| b.description).collect();
    let mut out = Vec::new();
    for b in &ours {
        let Some((mask, key)) = parse(&b.key) else { continue };
        for e in existing.iter().filter(|e| e.submap.is_empty() && e.modmask == mask && e.key.eq_ignore_ascii_case(&key)) {
            if own.contains(&e.description.as_str()) {
                continue;
            }
            let other = if e.description.is_empty() { format!("{} {}", e.dispatcher, e.arg).trim().to_string() } else { e.description.clone() };
            let c = Conflict { key: b.key.clone(), expected: REPLACED.contains(&e.description.as_str()), other };
            if !out.contains(&c) {
                out.push(c);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_parse_and_hold_their_modifiers() {
        assert_eq!(parse("ALT + TAB"), Some((ALT, "TAB".into())));
        assert_eq!(parse("SUPER + SHIFT + TAB"), Some((SUPER | SHIFT, "TAB".into())));
        assert_eq!(parse("ALT + SHIFT"), None, "no key");
        assert_eq!(parse("HYPER + TAB"), None);
        assert_eq!(held("ALT + SHIFT + grave"), ALT);
        assert_eq!(held("SUPER + ALT + W"), SUPER | ALT);
        assert_eq!(normalize("alt+shift+ grave"), "ALT + SHIFT + grave");
        assert!(valid_key("ALT + TAB") && !valid_key("ALT + \"); os.execute(\"x"));
    }

    #[test]
    fn block_unbinds_then_binds_each_key() {
        let keys = Keys { overview: String::new(), ..Default::default() };
        let b = block(&keys);
        assert!(b.starts_with(START) && b.ends_with(&format!("{END}\n")));
        assert!(b.contains("hl.unbind(\"ALT + TAB\")\no.bind(\"ALT + TAB\", \"Switch windows\", \"nyx-switcher-send next windows\")"));
        assert!(b.contains("\"SUPER + SHIFT + TAB\", \"Switch apps backwards\", \"nyx-switcher-send prev apps\""));
        assert!(!b.contains("overview"), "unbound keys are left out");
    }

    #[test]
    fn owns_only_its_block() {
        let b = block(&Keys::default());
        let added = edit("a\n", None, Some(&b)).unwrap();
        assert_eq!(added, format!("a\n{b}"));
        let other = Keys { apps: "SUPER + CTRL + TAB".into(), ..Default::default() };
        let changed = edit(&added, Some(&b), Some(&block(&other))).unwrap();
        assert!(changed.contains("SUPER + CTRL + TAB") && !changed.contains("\"SUPER + TAB\""));
        assert_eq!(edit(&added, Some(&b), None).unwrap(), "a\n");
        let hand = added.replace("send next windows", "send next apps");
        assert!(edit(&hand, Some(&b), None).is_err());
        assert!(edit(&format!("{added}{b}"), Some(&b), None).is_err(), "two blocks");
    }

    #[test]
    fn conflicts_from_hyprctl() {
        let json = include_str!("../tests/data/binds.json");
        let found = conflicts(json, &Keys::default()).unwrap();
        let expected: Vec<&Conflict> = found.iter().filter(|c| c.expected).collect();
        let other: Vec<&Conflict> = found.iter().filter(|c| !c.expected).collect();
        assert_eq!(expected.len(), 6, "{found:?}");
        assert_eq!(other.len(), 1);
        assert_eq!(other[0].key, "ALT + grave");
        assert_eq!(other[0].other, "exec my-thing");
        // Once our block is in, our own bindings aren't conflicts.
        let ours = r#"[{"modmask":8,"key":"TAB","submap":"","description":"Switch windows","dispatcher":"__lua","arg":"1"}]"#;
        assert!(conflicts(ours, &Keys::default()).unwrap().is_empty());
        assert!(conflicts("nope", &Keys::default()).is_err());
    }
}

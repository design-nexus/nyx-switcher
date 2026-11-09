#!/usr/bin/env bash
# Install Nyx Switcher.
#
#   curl -fsSL https://raw.githubusercontent.com/design-nexus/nyx-switcher/main/install.sh | bash
#
# It runs as a user service and takes Alt+Tab, Super+Tab, Alt+` and Super+Alt+W in
# Hyprland (keys in ~/.config/nyx/switcher.toml). NYX_SKIP_INTEGRATION=1 leaves the
# keys alone; ./uninstall.sh gives them back to Omarchy.
#
# The switcher is drawn in Quickshell by default (the shared runtime, nyx-shell-qs, with
# nyx-switcher-backend keeping the shortcuts' socket); the GTK switcher is still there:
# `nyx-switcher --ui gtk|qml` switches.
#   NYX_UI=gtk|qml ./install.sh     choose the interface on a first install
set -euo pipefail

DEFAULT_UI=qml

REPO="design-nexus/nyx-switcher"
say() { printf '\033[1m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[33m==>\033[0m %s\n' "$*" >&2; }
die() { printf '\033[31m==>\033[0m %s\n' "$*" >&2; exit 1; }

command -v hyprctl >/dev/null || warn "Hyprland wasn't found. Nyx Switcher is made for Omarchy (Hyprland)."

if command -v pacman >/dev/null; then
  need=()
  for p in gtk4 gtk4-layer-shell pkgconf gcc git; do pacman -Qq "$p" >/dev/null 2>&1 || need+=("$p"); done
  if ((${#need[@]})); then
    say "Installing ${need[*]}"
    sudo pacman -S --needed --noconfirm "${need[@]}"
  fi
fi

# This checkout, or a fresh one when piped from curl.
here=""
[[ -n ${BASH_SOURCE[0]:-} && -f ${BASH_SOURCE[0]} ]] && here=$(cd "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
if [[ -n $here && -f $here/Cargo.toml && -d $here/apps/switcher ]]; then
  src=$here
else
  say "Fetching the source"
  git clone -q --depth 1 "https://github.com/$REPO.git" "$work/src"
  src="$work/src"
fi

if ! command -v cargo >/dev/null; then
  command -v pacman >/dev/null || die "Rust (cargo) is needed to build Nyx Switcher. Install it and run this again."
  say "Installing rust"
  sudo pacman -S --needed --noconfirm rust
fi
say "Building Nyx Switcher (a few minutes the first time)"
(cd "$src" && cargo build --release --locked -p nyx-switcher -p nyx-switcher-backend)

bin="$HOME/.local/bin"
units="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
mkdir -p "$bin" "$units"
target="${CARGO_TARGET_DIR:-$src/target}/release"
# Replace the files rather than writing into them, so a running switcher keeps its old
# binary until it restarts.
for b in nyx-switcher nyx-switcher-send nyx-switcher-backend; do
  install -m 755 "$target/$b" "$bin/$b.new" && mv -f "$bin/$b.new" "$bin/$b"
done
install -m 644 "$src/apps/switcher/data/nyx-switcher.service" "$units/nyx-switcher.service"
install -m 644 "$src/apps/switcher-backend/data/nyx-switcher-backend.service" "$units/nyx-switcher-backend.service"
# The Quickshell interface, and the shared runtime it runs in (kept if a newer one is in place).
share="${XDG_DATA_HOME:-$HOME/.local/share}"
mkdir -p "$share/nyx-switcher"
rm -rf "$share/nyx-switcher/qml.new"
cp -r "$src/qml/Switcher" "$share/nyx-switcher/qml.new"
rm -rf "$share/nyx-switcher/qml" && mv "$share/nyx-switcher/qml.new" "$share/nyx-switcher/qml"
bash "$src/qml/runtime/install-runtime.sh" "$src/qml/runtime"
systemctl --user daemon-reload 2>/dev/null || true
# The interface: chosen once (NYX_UI, else the default), then whatever --ui last chose.
chosen="$share/nyx-switcher/ui"
if [[ ! -f $chosen ]]; then
  "$bin/nyx-switcher" --ui "${NYX_UI:-$DEFAULT_UI}" >/dev/null && echo "${NYX_UI:-$DEFAULT_UI}" > "$chosen"
fi
if [[ $("$bin/nyx-switcher" --ui) == qml ]]; then
  systemctl --user restart nyx-switcher-backend.service 2>/dev/null || true
  systemctl --user restart nyx-shell-qs.service 2>/dev/null || true
else
  systemctl --user enable nyx-switcher.service 2>/dev/null || true
  systemctl --user restart nyx-switcher.service 2>/dev/null || true
fi

if [[ ${NYX_SKIP_INTEGRATION:-} != 1 ]] && command -v hyprctl >/dev/null; then
  "$bin/nyx-switcher" --integrate || warn "The shortcuts weren't set. See: nyx-switcher --check-keys"
fi
say "Installed. Hold Alt and press Tab to switch windows; Super+Tab switches apps."
case ":$PATH:" in
  *":$bin:"*) ;;
  *) warn "$bin isn't on your PATH." ;;
esac

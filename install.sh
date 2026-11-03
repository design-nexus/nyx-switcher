#!/usr/bin/env bash
# Install Nyx Switcher.
#
#   curl -fsSL https://raw.githubusercontent.com/design-nexus/nyx-switcher/main/install.sh | bash
#
# It runs as a user service and takes Alt+Tab, Super+Tab, Alt+` and Super+Alt+W in
# Hyprland (keys in ~/.config/nyx/switcher.toml). NYX_SKIP_INTEGRATION=1 leaves the
# keys alone; ./uninstall.sh gives them back to Omarchy.
set -euo pipefail

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
(cd "$src" && cargo build --release --locked -p nyx-switcher)

bin="$HOME/.local/bin"
units="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
mkdir -p "$bin" "$units"
target="${CARGO_TARGET_DIR:-$src/target}/release"
# Replace the files rather than writing into them, so a running switcher keeps its old
# binary until it restarts.
for b in nyx-switcher nyx-switcher-send; do
  install -m 755 "$target/$b" "$bin/$b.new" && mv -f "$bin/$b.new" "$bin/$b"
done
install -m 644 "$src/apps/switcher/data/nyx-switcher.service" "$units/nyx-switcher.service"
systemctl --user daemon-reload 2>/dev/null || true
systemctl --user enable nyx-switcher.service 2>/dev/null || true
systemctl --user restart nyx-switcher.service 2>/dev/null || true

if [[ ${NYX_SKIP_INTEGRATION:-} != 1 ]] && command -v hyprctl >/dev/null; then
  "$bin/nyx-switcher" --integrate || warn "The shortcuts weren't set. See: nyx-switcher --check-keys"
fi
say "Installed. Hold Alt and press Tab to switch windows; Super+Tab switches apps."
case ":$PATH:" in
  *":$bin:"*) ;;
  *) warn "$bin isn't on your PATH." ;;
esac

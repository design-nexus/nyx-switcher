#!/usr/bin/env bash
# Remove Nyx Switcher and give Alt+Tab and Super+Tab back to Omarchy.
#
#   ./uninstall.sh            remove the program, its service and its shortcuts
#   ./uninstall.sh --purge    also remove its settings (~/.config/nyx/switcher.toml)
set -euo pipefail
say() { printf '\033[1m==>\033[0m %s\n' "$*"; }

bin="$HOME/.local/bin"
config="${XDG_CONFIG_HOME:-$HOME/.config}"
if [[ -x $bin/nyx-switcher ]]; then
  "$bin/nyx-switcher" --unintegrate || true
fi
systemctl --user disable --now nyx-switcher.service nyx-switcher-backend.service 2>/dev/null || true
# The Quickshell switcher (the shared runtime stays for the other components using it).
share="${XDG_DATA_HOME:-$HOME/.local/share}"
rm -rf "$share/nyx-switcher" "$share/nyx-shell/components/nyx-switcher"
rm -f "$bin/nyx-switcher" "$bin/nyx-switcher-send" "$bin/nyx-switcher-backend" \
  "$config/systemd/user/nyx-switcher.service" "$config/systemd/user/nyx-switcher-backend.service"
systemctl --user daemon-reload 2>/dev/null || true
if [[ ${1:-} == --purge ]]; then
  rm -f "$config/nyx/switcher.toml"
fi
say "Nyx Switcher removed."

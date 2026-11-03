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
systemctl --user disable --now nyx-switcher.service 2>/dev/null || true
rm -f "$bin/nyx-switcher" "$bin/nyx-switcher-send" "$config/systemd/user/nyx-switcher.service"
systemctl --user daemon-reload 2>/dev/null || true
if [[ ${1:-} == --purge ]]; then
  rm -f "$config/nyx/switcher.toml"
fi
say "Nyx Switcher removed."

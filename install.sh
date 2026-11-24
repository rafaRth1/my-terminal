#!/usr/bin/env bash
# Installs my-terminal for the current user, or updates it:
#   ./install.sh            build and install (run again after each update)
#   ./install.sh --uninstall
#
# - App:      ~/.local/bin/my-terminal (cargo install --root ~/.local)
# - Hook:     ~/.cargo/bin/my-terminal-hook (the path agent hook configs use)
# - Launcher: ~/.local/share/applications/my-terminal.desktop
# - Icon:     ~/.local/share/icons/hicolor/scalable/apps/my-terminal.svg
#
# A running my-terminal keeps the old version until it is closed and opened
# again.
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
bin="$HOME/.local/bin/my-terminal"
desktop="$data/applications/my-terminal.desktop"
icon="$data/icons/hicolor/scalable/apps/my-terminal.svg"

refresh_caches() {
  if command -v update-desktop-database >/dev/null; then
    update-desktop-database -q "$data/applications" || true
  fi
  if command -v gtk-update-icon-cache >/dev/null; then
    gtk-update-icon-cache -q -t "$data/icons/hicolor" || true
  fi
}

if [[ "${1:-}" == "--uninstall" ]]; then
  cargo uninstall --root "$HOME/.local" app 2>/dev/null || rm -f "$bin"
  rm -f "$desktop" "$icon"
  refresh_caches
  echo "Removed my-terminal (the hook in ~/.cargo/bin and your settings stay)."
  exit 0
fi

echo "Building and installing the app..."
cargo install --locked --path "$repo/crates/app" --root "$HOME/.local"
echo "Installing the agent hook..."
cargo install --locked --path "$repo/crates/agent-status"

install -Dm644 "$repo/packaging/my-terminal.svg" "$icon"
mkdir -p "$(dirname "$desktop")"
sed "s|@BIN@|$bin|" "$repo/packaging/my-terminal.desktop.in" > "$desktop"
chmod 644 "$desktop"
refresh_caches

echo "Installed my-terminal at $bin"
echo "Open it from the applications menu (search \"my-terminal\")."

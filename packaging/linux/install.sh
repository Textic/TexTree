#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_DIR="${HOME}/.local/bin"
APP_DIR="${HOME}/.local/share/applications"
ICON_PNG_DIR="${HOME}/.local/share/icons/hicolor/512x512/apps"
ICON_SVG_DIR="${HOME}/.local/share/icons/hicolor/scalable/apps"

mkdir -p "$BIN_DIR" "$APP_DIR" "$ICON_PNG_DIR" "$ICON_SVG_DIR"
install -m 755 "$SCRIPT_DIR/textree" "$BIN_DIR/textree"
install -m 644 "$SCRIPT_DIR/assets/textree.png" "$ICON_PNG_DIR/textree.png"
install -m 644 "$SCRIPT_DIR/assets/textree.svg" "$ICON_SVG_DIR/textree.svg"
install -m 644 "$SCRIPT_DIR/textree.desktop" "$APP_DIR/textree.desktop"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$APP_DIR" || true
fi

echo "Installed TexTree to $BIN_DIR/textree"
echo "Make sure $BIN_DIR is in your PATH."

#!/usr/bin/env bash
set -euo pipefail

# Usage: build-deb.sh <binary_path> <version> <tag> <out_dir>
BIN_PATH="$1"
VERSION="$2"
TAG="$3"
OUT_DIR="$4"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BIN_ABS="$(cd "$(dirname "$BIN_PATH")" && pwd)/$(basename "$BIN_PATH")"
mkdir -p "$OUT_DIR"
OUT_ABS="$(cd "$OUT_DIR" && pwd)"

WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

PKG_ROOT="$WORK_DIR/pkg"
mkdir -p \
  "$PKG_ROOT/DEBIAN" \
  "$PKG_ROOT/usr/bin" \
  "$PKG_ROOT/usr/share/applications" \
  "$PKG_ROOT/usr/share/icons/hicolor/512x512/apps" \
  "$PKG_ROOT/usr/share/icons/hicolor/scalable/apps" \
  "$PKG_ROOT/usr/share/doc/textree"

install -m 755 "$BIN_ABS" "$PKG_ROOT/usr/bin/textree"
install -m 644 "$ROOT_DIR/packaging/linux/textree.desktop" "$PKG_ROOT/usr/share/applications/textree.desktop"
install -m 644 "$ROOT_DIR/assets/textree.png" "$PKG_ROOT/usr/share/icons/hicolor/512x512/apps/textree.png"
install -m 644 "$ROOT_DIR/assets/textree.svg" "$PKG_ROOT/usr/share/icons/hicolor/scalable/apps/textree.svg"
install -m 644 "$ROOT_DIR/README.md" "$PKG_ROOT/usr/share/doc/textree/README.md"

mkdir -p "$WORK_DIR/shlibs/debian"
cat > "$WORK_DIR/shlibs/debian/control" <<'EOF'
Source: textree
Section: utils
Priority: optional
Maintainer: Textic <danielbienchido@gmail.com>

Package: textree
Architecture: amd64
EOF

SHLIBS_LINE="$(cd "$WORK_DIR/shlibs" && dpkg-shlibdeps -O "$PKG_ROOT/usr/bin/textree" | sed -n 's/^shlibs:Depends=//p' | head -n1)"
INSTALLED_KB="$(du -sk "$PKG_ROOT/usr" | cut -f1)"

{
  echo "Package: textree"
  echo "Version: $VERSION"
  echo "Section: utils"
  echo "Priority: optional"
  echo "Architecture: amd64"
  echo "Installed-Size: $INSTALLED_KB"
  if [[ -n "$SHLIBS_LINE" ]]; then
    echo "Depends: $SHLIBS_LINE"
  fi
  echo "Maintainer: Textic <danielbienchido@gmail.com>"
  echo "Homepage: https://github.com/Textic/TexTree"
  echo "Description: Minimalist flat-dark native disk space analyzer"
  echo " Built with Rust and GPUI-Kit for analyzing storage trees, large files, and file locks."
} > "$PKG_ROOT/DEBIAN/control"

chmod 755 "$PKG_ROOT/DEBIAN"
chmod 644 "$PKG_ROOT/DEBIAN/control"

dpkg-deb --build --root-owner-group "$PKG_ROOT" "$OUT_ABS/textree-$TAG-linux-x86_64.deb"

TAR_STAGE="$WORK_DIR/tar_stage"
mkdir -p "$TAR_STAGE/assets"
install -m 755 "$BIN_ABS" "$TAR_STAGE/textree"
install -m 755 "$ROOT_DIR/packaging/linux/install.sh" "$TAR_STAGE/install.sh"
install -m 644 "$ROOT_DIR/packaging/linux/textree.desktop" "$TAR_STAGE/textree.desktop"
install -m 644 "$ROOT_DIR/README.md" "$TAR_STAGE/README.md"
install -m 644 "$ROOT_DIR/assets/textree.png" "$TAR_STAGE/assets/textree.png"
install -m 644 "$ROOT_DIR/assets/textree.svg" "$TAR_STAGE/assets/textree.svg"
tar -C "$TAR_STAGE" -czf "$OUT_ABS/textree-$TAG-linux-x86_64.tar.gz" .

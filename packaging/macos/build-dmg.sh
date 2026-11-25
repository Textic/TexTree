#!/usr/bin/env bash
set -euo pipefail

# Usage: build-dmg.sh <binary_path> <version> <tag> <arch_label> <out_dir>
BIN_PATH="$1"
VERSION="$2"
TAG="$3"
ARCH_LABEL="$4"
OUT_DIR="$5"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BIN_ABS="$(cd "$(dirname "$BIN_PATH")" && pwd)/$(basename "$BIN_PATH")"
mkdir -p "$OUT_DIR"
OUT_ABS="$(cd "$OUT_DIR" && pwd)"

WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

ICONSET="$WORK_DIR/TexTree.iconset"
ICNS="$WORK_DIR/TexTree.icns"
mkdir -p "$ICONSET"
sips -z 16 16     "$ROOT_DIR/assets/textree.png" --out "$ICONSET/icon_16x16.png" >/dev/null
sips -z 32 32     "$ROOT_DIR/assets/textree.png" --out "$ICONSET/icon_16x16@2x.png" >/dev/null
sips -z 32 32     "$ROOT_DIR/assets/textree.png" --out "$ICONSET/icon_32x32.png" >/dev/null
sips -z 64 64     "$ROOT_DIR/assets/textree.png" --out "$ICONSET/icon_32x32@2x.png" >/dev/null
sips -z 128 128   "$ROOT_DIR/assets/textree.png" --out "$ICONSET/icon_128x128.png" >/dev/null
sips -z 256 256   "$ROOT_DIR/assets/textree.png" --out "$ICONSET/icon_128x128@2x.png" >/dev/null
sips -z 256 256   "$ROOT_DIR/assets/textree.png" --out "$ICONSET/icon_256x256.png" >/dev/null
sips -z 512 512   "$ROOT_DIR/assets/textree.png" --out "$ICONSET/icon_256x256@2x.png" >/dev/null
sips -z 512 512   "$ROOT_DIR/assets/textree.png" --out "$ICONSET/icon_512x512.png" >/dev/null
iconutil -c icns "$ICONSET" -o "$ICNS"

APP="$WORK_DIR/TexTree.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN_ABS" "$APP/Contents/MacOS/textree"
chmod +x "$APP/Contents/MacOS/textree"
cp "$ICNS" "$APP/Contents/Resources/TexTree.icns"

cat > "$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleExecutable</key>
  <string>textree</string>
  <key>CFBundleIconFile</key>
  <string>TexTree</string>
  <key>CFBundleIdentifier</key>
  <string>com.textic.textree</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>TexTree</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>$VERSION</string>
  <key>CFBundleVersion</key>
  <string>$VERSION</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
EOF

ditto -c -k --sequesterRsrc --keepParent "$APP" "$OUT_ABS/textree-$TAG-$ARCH_LABEL.zip"

DMG_STAGE="$WORK_DIR/dmg_stage"
mkdir -p "$DMG_STAGE"
cp -R "$APP" "$DMG_STAGE/TexTree.app"
ln -s /Applications "$DMG_STAGE/Applications"
hdiutil create -volname "TexTree $TAG" -srcfolder "$DMG_STAGE" -ov -format UDZO "$OUT_ABS/textree-$TAG-$ARCH_LABEL.dmg"

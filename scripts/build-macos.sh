#!/bin/bash
# Build a universal Stillshot.app and a DMG. Run on a Mac with Xcode CLT.
set -euo pipefail
cd "$(dirname "$0")/.."

rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin

mkdir -p dist
lipo -create \
  target/aarch64-apple-darwin/release/stillshot \
  target/x86_64-apple-darwin/release/stillshot \
  -output dist/stillshot
chmod +x dist/stillshot

APP="dist/Stillshot.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp dist/stillshot "$APP/Contents/MacOS/stillshot"
cp packaging/macos/Info.plist "$APP/Contents/Info.plist"
if [[ -f assets/stillshot.png ]]; then
  ICONSET="dist/stillshot.iconset"
  rm -rf "$ICONSET"
  mkdir -p "$ICONSET"
  sips -z 16 16 assets/stillshot.png --out "$ICONSET/icon_16x16.png" >/dev/null
  sips -z 32 32 assets/stillshot.png --out "$ICONSET/icon_16x16@2x.png" >/dev/null
  sips -z 32 32 assets/stillshot.png --out "$ICONSET/icon_32x32.png" >/dev/null
  sips -z 64 64 assets/stillshot.png --out "$ICONSET/icon_32x32@2x.png" >/dev/null
  sips -z 128 128 assets/stillshot.png --out "$ICONSET/icon_128x128.png" >/dev/null
  sips -z 256 256 assets/stillshot.png --out "$ICONSET/icon_128x128@2x.png" >/dev/null
  sips -z 256 256 assets/stillshot.png --out "$ICONSET/icon_256x256.png" >/dev/null
  sips -z 512 512 assets/stillshot.png --out "$ICONSET/icon_256x256@2x.png" >/dev/null
  sips -z 512 512 assets/stillshot.png --out "$ICONSET/icon_512x512.png" >/dev/null
  cp assets/stillshot.png "$ICONSET/icon_512x512@2x.png"
  iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/stillshot.icns" || true
fi

STAGE="dist/dmg-root"
rm -rf "$STAGE"
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
# hdiutil occasionally returns "Resource busy" on GitHub-hosted runners.
dmg_ok=0
for attempt in 1 2 3; do
  if hdiutil create -volname "Stillshot" -srcfolder "$STAGE" -ov -format UDZO "dist/Stillshot.dmg"; then
    dmg_ok=1
    break
  fi
  echo "hdiutil failed (attempt ${attempt}); retrying" >&2
  sleep 5
done
if [[ "$dmg_ok" -ne 1 ]]; then
  echo "hdiutil could not create dist/Stillshot.dmg" >&2
  exit 1
fi
echo "Built dist/Stillshot.dmg"
file dist/stillshot
lipo -info dist/stillshot

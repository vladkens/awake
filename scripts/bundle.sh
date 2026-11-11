#!/bin/sh
# Awake.app from the release build: LSUIElement (no Dock icon), signed ad hoc, so that
# SMAppService can register it to launch at login. AWAKE_FLAVOR=dev: Awake Dev.app, an app of its
# own beside Awake (main.rs's DEV), with Awake's icon in orange (scripts/dev-icon.py).
#   [AWAKE_FLAVOR=dev] scripts/bundle.sh [dest dir, default target/release]
set -eu
cd "$(dirname "$0")/.."
cargo build --release --locked
dest="${1:-target/release}"
name=Awake id=me.vladkens.awake icon=assets/icon.png
if [ "${AWAKE_FLAVOR:-}" = dev ]; then
  name="Awake Dev" id=me.vladkens.awake-dev icon=target/dev-icon.png
  python3 scripts/dev-icon.py assets/icon.png "$icon"
fi
app="$dest/$name.app"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/awake "$app/Contents/MacOS/$name"
# AppIcon.icns from the icon (1024 × 1024): every size iconutil expects.
set_dir="$(mktemp -d)/AppIcon.iconset"
mkdir -p "$set_dir"
for size in 16 32 128 256 512; do
  sips -z $size $size "$icon" --out "$set_dir/icon_${size}x${size}.png" >/dev/null
  sips -z $((size * 2)) $((size * 2)) "$icon" --out "$set_dir/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$set_dir" -o "$app/Contents/Resources/AppIcon.icns"
rm -rf "$(dirname "$set_dir")"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key><string>$id</string>
  <key>CFBundleName</key><string>$name</string>
  <key>CFBundleExecutable</key><string>$name</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>LSUIElement</key><true/>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
codesign --force --sign - "$app"
echo "$app"

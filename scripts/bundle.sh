#!/usr/bin/env bash
# Builds the menu bar app as target/Binaural.app: a background app with no Dock icon.
# Open it with `open target/Binaural.app`, or copy it to /Applications.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
app="$root/target/Binaural.app"
version="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -1)"

cargo build --release --bin binaural-bar --manifest-path "$root/Cargo.toml"

mkdir -p "$app/Contents/MacOS"
# Copy then rename, so a running copy of the app keeps its original file instead of being
# overwritten in place, which macOS can kill it for.
cp "$root/target/release/binaural-bar" "$app/Contents/MacOS/binaural-bar.new"
mv -f "$app/Contents/MacOS/binaural-bar.new" "$app/Contents/MacOS/binaural-bar"
cat >"$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>binaural-bar</string>
  <key>CFBundleIdentifier</key><string>dev.rampal.binaural</string>
  <key>CFBundleName</key><string>Binaural</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${version}</string>
  <key>CFBundleVersion</key><string>${version}</string>
  <key>LSUIElement</key><true/>
</dict>
</plist>
EOF
# Ad-hoc signature so macOS accepts the bundle as a whole after the binary was replaced.
codesign --force --sign - "$app"
echo "built $app"

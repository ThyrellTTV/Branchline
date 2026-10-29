#!/bin/sh
# Packs the Apple silicon and Intel builds into one universal branchline.app, then a .dmg (drag
# to Applications) and a .zip of it.
# Usage: packaging/macos/build-app.sh <version> <output name without extension>
# Needs both `cargo build --release --target aarch64-apple-darwin` and
# `--target x86_64-apple-darwin` done first. macOS only (lipo, iconutil, codesign, hdiutil).
set -eu

version="$1"
out="$2"
work="$(mktemp -d)"
app="$work/branchline.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

lipo -create -output "$app/Contents/MacOS/branchline" \
  target/aarch64-apple-darwin/release/branchline \
  target/x86_64-apple-darwin/release/branchline
strip -x "$app/Contents/MacOS/branchline"
sed "s/@VERSION@/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"
printf 'APPL????' > "$app/Contents/PkgInfo"

# The icon, from the 256 px PNG (the larger sizes are left to macOS to scale).
iconset="$work/branchline.iconset"
mkdir "$iconset"
for entry in 16x16:16 16x16@2x:32 32x32:32 32x32@2x:64 128x128:128 128x128@2x:256 256x256:256; do
  name="${entry%%:*}"
  px="${entry##*:}"
  sips -z "$px" "$px" assets/app-icon/branchline-256.png --out "$iconset/icon_$name.png" >/dev/null
done
iconutil -c icns -o "$app/Contents/Resources/branchline.icns" "$iconset"

cp README.md ATTRIBUTION.md LICENSE-MIT LICENSE-APACHE "$app/Contents/Resources/"

# Not notarized: an ad-hoc signature, which Apple silicon needs to run the program at all.
codesign --force --deep --sign - "$app"
codesign --verify --strict "$app"

ditto -c -k --keepParent "$app" "$out.zip"

dmg="$work/dmg"
mkdir "$dmg"
cp -R "$app" "$dmg/"
ln -s /Applications "$dmg/Applications"
hdiutil create -volname branchline -srcfolder "$dmg" -ov -format UDZO "$out.dmg"

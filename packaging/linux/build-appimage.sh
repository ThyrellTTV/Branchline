#!/bin/sh
# Packs target/release/gitcraft into an AppImage: one file that runs on most Linux distributions.
# Usage: packaging/linux/build-appimage.sh <output.AppImage>
# Needs appimagetool on PATH (the release workflow downloads it).
set -eu

out="$1"
app="$(mktemp -d)/gitcraft.AppDir"
mkdir -p "$app/usr/bin" "$app/usr/share/applications" "$app/usr/share/icons/hicolor/256x256/apps"

cp target/release/gitcraft "$app/usr/bin/gitcraft"
strip "$app/usr/bin/gitcraft"
cp packaging/gitcraft.desktop "$app/usr/share/applications/gitcraft.desktop"
cp assets/app-icon/gitcraft-256.png "$app/usr/share/icons/hicolor/256x256/apps/gitcraft.png"

# What appimagetool looks for at the top of the AppDir.
cp packaging/gitcraft.desktop "$app/gitcraft.desktop"
cp assets/app-icon/gitcraft-256.png "$app/gitcraft.png"
ln -s gitcraft.png "$app/.DirIcon"
cat > "$app/AppRun" <<'EOF'
#!/bin/sh
here="$(dirname "$(readlink -f "$0")")"
exec "$here/usr/bin/gitcraft" "$@"
EOF
chmod +x "$app/AppRun"

ARCH=x86_64 appimagetool "$app" "$out"

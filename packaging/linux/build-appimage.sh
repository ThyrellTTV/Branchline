#!/bin/sh
# Packs target/release/branchline into an AppImage: one file that runs on most Linux distributions.
# Usage: packaging/linux/build-appimage.sh <output.AppImage>
# Needs appimagetool on PATH (the release workflow downloads it).
set -eu

out="$1"
app="$(mktemp -d)/branchline.AppDir"
mkdir -p "$app/usr/bin" "$app/usr/share/applications" "$app/usr/share/icons/hicolor/256x256/apps"

cp target/release/branchline "$app/usr/bin/branchline"
strip "$app/usr/bin/branchline"
cp packaging/branchline.desktop "$app/usr/share/applications/branchline.desktop"
cp assets/app-icon/branchline-256.png "$app/usr/share/icons/hicolor/256x256/apps/branchline.png"

# What appimagetool looks for at the top of the AppDir.
cp packaging/branchline.desktop "$app/branchline.desktop"
cp assets/app-icon/branchline-256.png "$app/branchline.png"
ln -s branchline.png "$app/.DirIcon"
cat > "$app/AppRun" <<'EOF'
#!/bin/sh
here="$(dirname "$(readlink -f "$0")")"
exec "$here/usr/bin/branchline" "$@"
EOF
chmod +x "$app/AppRun"

ARCH=x86_64 appimagetool "$app" "$out"

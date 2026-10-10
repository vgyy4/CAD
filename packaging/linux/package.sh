#!/usr/bin/env bash
# Builds the Linux artifacts from a staged install tree (packaging/stage.sh):
# a portable .tar.gz, an AppImage and a .deb (ADR 0013).
#
# Usage: packaging/linux/package.sh <version> <arch> <stage-dir> <dist-dir> <appimagetool> <runtime-file>
#   appimagetool / runtime-file: the pinned, checksum-verified downloads from release.yml.
set -euo pipefail

version="$1" arch="$2" stage="$3" dist="$4" appimagetool="$5" runtime="$6"
here="$(cd "$(dirname "$0")" && pwd)"
: "${SOURCE_DATE_EPOCH:=$(git -C "$here" log -1 --format=%ct)}"
export SOURCE_DATE_EPOCH
mkdir -p "$dist"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

det_tar() { # <archive.tar.gz> <dir> <name-in-archive>
  tar --sort=name --owner=0 --group=0 --numeric-owner --mtime="@$SOURCE_DATE_EPOCH" \
    --transform "s|^\.|$3|" -C "$2" -cf - . | gzip -n -9 > "$1"
}

# Portable archive.
name="gusset-$version-linux-$arch"
det_tar "$dist/$name.tar.gz" "$stage" "$name"
cp "$stage.files.json" "$dist/$name.tar.gz.files.json"

# AppImage. AppRun starts the GUI; the CLI is usr/bin/gusset inside the image.
appdir="$work/Gusset.AppDir"
mkdir -p "$appdir/usr"
cp -a "$stage/." "$appdir/usr/"
mkdir -p "$appdir/usr/share/applications" "$appdir/usr/share/icons/hicolor/scalable/apps"
cp "$here/gusset.desktop" "$appdir/usr/share/applications/gusset.desktop"
cp "$here/gusset.svg" "$appdir/usr/share/icons/hicolor/scalable/apps/gusset.svg"
cp "$here/gusset.desktop" "$here/gusset.svg" "$appdir/"
ln -s usr/bin/gusset-gui "$appdir/AppRun"
appimage="Gusset-$version-$arch.AppImage"
# No network access, no update information embedded (the in-app updater handles
# updates), and the runtime comes from the verified local file.
ARCH="$arch" APPIMAGE_EXTRACT_AND_RUN=1 "$appimagetool" --no-appstream --runtime-file "$runtime" \
  "$appdir" "$dist/$appimage"
cp "$stage.files.json" "$dist/$appimage.files.json"

# Debian package (dpkg-deb from the runner's dpkg).
debarch="amd64"; [ "$arch" = "aarch64" ] && debarch="arm64"
deb="$work/deb"
mkdir -p "$deb/DEBIAN" "$deb/usr"
cp -a "$stage/." "$deb/usr/"
mkdir -p "$deb/usr/share/applications" "$deb/usr/share/icons/hicolor/scalable/apps"
cp "$here/gusset.desktop" "$deb/usr/share/applications/gusset.desktop"
cp "$here/gusset.svg" "$deb/usr/share/icons/hicolor/scalable/apps/gusset.svg"
size_kb="$(du -sk "$deb/usr" | cut -f1)"
cat > "$deb/DEBIAN/control" <<EOF
Package: gusset
Version: $version
Architecture: $debarch
Maintainer: Gusset contributors
Installed-Size: $size_kb
Depends: libc6 (>= 2.39), libstdc++6, libgcc-s1
Recommends: libvulkan1, libgl1, libxkbcommon0, libwayland-client0
Section: graphics
Priority: optional
Homepage: https://github.com/vgyy4/CAD
Description: Parametric CAD with a built-in MCP server
 Gusset is a parametric CAD application with a code view and a built-in
 MCP server. Includes the Open CASCADE Technology shared libraries.
EOF
find "$deb" -exec touch -h -d "@$SOURCE_DATE_EPOCH" {} +
dpkg-deb --root-owner-group -Zxz --build "$deb" "$dist/gusset_${version}_$debarch.deb"
echo "Linux packages for $version ($arch) written to $dist"

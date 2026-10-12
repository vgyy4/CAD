#!/usr/bin/env bash
# Stages a release build into an install tree (ADR 0013). Used by
# .github/workflows/release.yml on every target; never run it to publish from a
# local machine.
#
# Usage: packaging/stage.sh <os> <arch> <version> <occt-prefix> <cargo-target-dir> <out-dir>
#   os:   linux | windows | macos
#   arch: x86_64 | aarch64
#
# Layout written to <out-dir>:
#   linux:   bin/{gusset,gusset-gui,gusset-worker}  lib/gusset/libTK*.so*  share/doc/gusset/*
#   windows: gusset.exe gusset-gui.exe gusset-worker.exe TK*.dll <MSVC runtime> doc/*
#   macos:   Gusset.app/Contents/{MacOS,Frameworks,Resources}
# plus <out-dir>.files.json: every installed file with its SHA-256 and size (the
# per-file list the updater checks after extraction, ADR 0014).
set -euo pipefail

os="$1" arch="$2" version="$3" occt="$4" target_dir="$5" out="$6"
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/.." && pwd)"
# shellcheck source=/dev/null
. "$repo/deps/occt/occt.env"

bins=(gusset gusset-gui gusset-worker)
rm -rf "$out"
mkdir -p "$out"

# Licence and provenance files shipped with every artifact (ADR 0001/0013).
write_docs() {
  local dst="$1"
  mkdir -p "$dst"
  cp "$repo/LICENSE-MIT" "$repo/LICENSE-APACHE" "$repo/THIRD_PARTY_LICENSES.md" "$dst/"
  cp "$here/occt/LICENSE_LGPL_21.txt" "$here/occt/OCCT_LGPL_EXCEPTION.txt" "$dst/"
  cp "$repo/deps/occt/build-occt.sh" "$repo/deps/occt/occt.env" "$dst/"
  sed -e "s|@OCCT_TAG@|$OCCT_TAG|g" -e "s|@OCCT_COMMIT@|$OCCT_COMMIT|g" -e "s|@OCCT_REPO@|$OCCT_REPO|g" \
    "$here/occt/NOTICE-OCCT.md" > "$dst/NOTICE-OCCT.md"
  printf 'Gusset %s (%s-%s)\n' "$version" "$os" "$arch" > "$dst/VERSION.txt"
}

case "$os" in
  linux)
    mkdir -p "$out/bin" "$out/lib/gusset"
    for b in "${bins[@]}"; do
      install -m 0755 "$target_dir/release/$b" "$out/bin/$b"
    done
    # Shared libraries only; keep the soname symlinks.
    cp -a "$occt"/lib/libTK*.so* "$out/lib/gusset/"
    # Install-relative search paths only (ADR 0008 §6). The build adds an absolute
    # RPATH to the CI's OCCT directory for tests; it must never ship.
    for b in "${bins[@]}"; do
      patchelf --force-rpath --set-rpath '$ORIGIN/../lib/gusset' "$out/bin/$b"
    done
    for l in "$out"/lib/gusset/*.so*; do
      [ -L "$l" ] && continue
      patchelf --force-rpath --set-rpath '$ORIGIN' "$l"
    done
    write_docs "$out/share/doc/gusset"
    ;;
  windows)
    for b in "${bins[@]}"; do
      cp "$target_dir/release/$b.exe" "$out/"
    done
    # Next to the executables; the app loads them only from the application
    # directory (ADR 0008 §6).
    cp "$occt"/bin/TK*.dll "$out/"
    # The MSVC runtime the Rust binaries and OCCT link against (app-local deployment
    # of the Visual C++ redistributable files).
    vswhere="${ProgramFiles:-C:/Program Files} (x86)/Microsoft Visual Studio/Installer/vswhere.exe"
    vsarch="x64"; [ "$arch" = "aarch64" ] && vsarch="arm64"
    crt="$("$vswhere" -latest -products '*' -find "VC/Redist/MSVC/*/$vsarch/Microsoft.VC14*.CRT" | tr -d '\r' | tail -n 1)"
    if [ -z "$crt" ] || [ ! -d "$crt" ]; then
      echo "MSVC runtime redistributable not found for $vsarch" >&2
      exit 1
    fi
    cp "$crt"/*.dll "$out/"
    write_docs "$out/doc"
    ;;
  macos)
    app="$out/Gusset.app/Contents"
    mkdir -p "$app/MacOS" "$app/Frameworks" "$app/Resources"
    for b in "${bins[@]}"; do
      install -m 0755 "$target_dir/release/$b" "$app/MacOS/$b"
    done
    cp -a "$occt"/lib/libTK*.dylib "$app/Frameworks/"
    # Point every reference to an OCCT library at @rpath, and @rpath at Frameworks.
    for f in "$app"/Frameworks/*.dylib; do
      [ -L "$f" ] && continue
      install_name_tool -id "@rpath/$(basename "$f")" "$f"
    done
    for f in "$app"/MacOS/* "$app"/Frameworks/*.dylib; do
      [ -L "$f" ] && continue
      otool -L "$f" | awk 'NR > 1 { print $1 }' | grep -E '/libTK[^/]*\.dylib$' | while read -r dep; do
        install_name_tool -change "$dep" "@rpath/$(basename "$dep")" "$f"
      done
      # Drop absolute LC_RPATHs added for CI tests, then add the bundle-relative one.
      otool -l "$f" | awk '/cmd LC_RPATH/ { getline; getline; print $2 }' | grep '^/' | while read -r rp; do
        install_name_tool -delete_rpath "$rp" "$f"
      done
    done
    for b in "${bins[@]}"; do
      install_name_tool -add_rpath "@executable_path/../Frameworks" "$app/MacOS/$b"
    done
    # Bundle versions must be dotted numbers: drop a pre-release suffix (0.1.0-mvp).
    sed -e "s|@VERSION@|${version%%-*}|g" "$here/macos/Info.plist" > "$app/Info.plist"
    write_docs "$app/Resources/doc"
    # Ad-hoc signature (required on Apple Silicon after editing load commands). Real
    # Developer ID signing is a separate, disabled step (ADR 0015).
    codesign --force --deep --sign - --options runtime "$out/Gusset.app"
    ;;
  *)
    echo "unknown os: $os" >&2
    exit 1
    ;;
esac

# Per-file hash list of the install tree (regular files only, sorted, `/` separators).
py="$(command -v python3 || command -v python)"
"$py" -I "$here/file_list.py" "$out" > "$out.files.json"
echo "staged Gusset $version for $os-$arch in $out"

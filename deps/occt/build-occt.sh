#!/usr/bin/env bash
# Builds the pinned OCCT as shared libraries into "$1" (install prefix).
# Used by CI on every target and by developers. Verifies the source commit hash.
# Usage: deps/occt/build-occt.sh <install-prefix> [work-dir]
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=/dev/null
. "$here/occt.env"
prefix="$1"
work="${2:-${RUNNER_TEMP:-/tmp}/occt-work}"
mkdir -p "$work"
if [ ! -d "$work/src/.git" ]; then
  git clone --quiet --depth 1 --branch "$OCCT_TAG" "$OCCT_REPO" "$work/src"
fi
actual="$(git -C "$work/src" rev-parse HEAD)"
if [ "$actual" != "$OCCT_COMMIT" ]; then
  echo "OCCT source commit mismatch: expected $OCCT_COMMIT, got $actual" >&2
  exit 1
fi
args=(
  -DCMAKE_BUILD_TYPE=Release
  -DBUILD_LIBRARY_TYPE=Shared
  -DBUILD_MODULE_Draw=OFF
  -DBUILD_MODULE_Visualization=OFF
  -DUSE_TK=OFF -DUSE_FREETYPE=OFF -DUSE_OPENGL=OFF -DUSE_XLIB=OFF
  -DBUILD_DOC_Overview=OFF
  -DINSTALL_DIR_LAYOUT=Unix
  -DINSTALL_DIR="$prefix"
  -DCMAKE_INSTALL_PREFIX="$prefix"
)
case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*)
    arch="x64"; [ "${RUNNER_ARCH:-}" = "ARM64" ] && arch="ARM64"
    cmake -S "$work/src" -B "$work/build" -A "$arch" "${args[@]}"
    cmake --build "$work/build" --config Release --parallel
    cmake --install "$work/build" --config Release
    ;;
  *)
    gen=(); command -v ninja >/dev/null && gen=(-G Ninja)
    cmake -S "$work/src" -B "$work/build" "${gen[@]}" "${args[@]}"
    cmake --build "$work/build" --parallel
    cmake --install "$work/build"
    ;;
esac
echo "OCCT $OCCT_TAG ($OCCT_COMMIT) installed to $prefix"

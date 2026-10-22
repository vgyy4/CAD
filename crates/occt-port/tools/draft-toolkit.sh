#!/usr/bin/env bash
# draft-toolkit.sh <occt-src> <include-dir> <toolkit> <out-dir> [jobs]: runs cxx2rs.py over
# every .cxx file of one OCCT toolkit (ADR 0025 drafts; no network, no models). Writes
# <out>/<Package>/<File>.rs, a failures list and a summary line per file. Mechanical only:
# the drafts are reviewed and finished against the judges before anything is merged.
set -uo pipefail
src=$1; inc=$2; tk=$3; out=$4; jobs=${5:-4}
here="$(cd "$(dirname "$0")" && pwd)"
dir=$(find "$src/src" -maxdepth 2 -type d -name "$tk" | head -1)
[ -n "$dir" ] || { echo "no toolkit $tk" >&2; exit 1; }
mkdir -p "$out"; tmp=$(mktemp -d)
one() {
  f=$1; pkg=$(basename "$(dirname "$f")"); name=$(basename "$f" .cxx)
  mkdir -p "$out/$pkg"; j="$tmp/$name.json"
  if clang++ -std=c++17 -fsyntax-only -I"$inc" -Xclang -ast-dump=json "$f" > "$j" 2> "$tmp/$name.err" &&
     python3 "$here/cxx2rs.py" "$j" "$f" > "$out/$pkg/$name.rs" 2> "$tmp/$name.py.err"; then
    echo "$pkg/$name $(grep -c 'todo!' "$out/$pkg/$name.rs")" >> "$out/summary.txt"
  else
    echo "$pkg/$name" >> "$out/failures.txt"
  fi
  rm -f "$j"
}
export -f one; export out tmp inc here
find "$dir" -mindepth 2 -maxdepth 2 -name '*.cxx' -not -path '*/GTests/*' | sort |
  xargs -P "$jobs" -I{} bash -c 'one "$1"' _ {}
rm -rf "$tmp"
echo "$tk: $(cat "$out/summary.txt" 2>/dev/null | wc -l) drafted, $(cat "$out/failures.txt" 2>/dev/null | wc -l) failed"

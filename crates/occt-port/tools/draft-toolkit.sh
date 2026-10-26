#!/usr/bin/env bash
# draft-toolkit.sh <occt-src> <include-dir> <toolkit> <out-dir> [jobs] [cxx|headers]: runs
# cxx2rs.py over every .cxx file of one OCCT toolkit, or (headers) over every .hxx with its
# .lxx/.gxx: class layouts, enums and inline methods (ADR 0025 drafts; no network, no models). Writes
# <out>/<Package>/<File>.rs, a failures list and a summary line per file. Mechanical only:
# the drafts are reviewed and finished against the judges before anything is merged.
set -uo pipefail
src=$1; inc=$2; tk=$3; out=$4; jobs=${5:-4}; mode=${6:-cxx}
here="$(cd "$(dirname "$0")" && pwd)"
dir=$(find "$src/src" -maxdepth 2 -type d -name "$tk" | head -1)
[ -n "$dir" ] || { echo "no toolkit $tk" >&2; exit 1; }
mkdir -p "$out"; tmp=$(mktemp -d)
# The toolkit's own package directories too: configure copies headers only for the
# toolkits it would build (glTF, for one, is left out without RapidJSON).
pkginc=$(find "$dir" -mindepth 1 -maxdepth 1 -type d -not -name GTests -printf '-I%p ')
one() {
  f=$1; pkg=$(basename "$(dirname "$f")"); name=$(basename "$f"); name=${name%.*}
  mkdir -p "$out/$pkg"; j="$tmp/$name.json"; tu=$f; key=$f; rs="$out/$pkg/$name.rs"
  if [ "$mode" = headers ]; then
    # A one-line translation unit that includes the header from the source tree (so its
    # relative includes such as "../Geom/X.pxx" resolve); declarations are kept when they
    # are located in Name.hxx / Name.lxx / Name.gxx.
    # Some headers rely on their includer for Standard_OStream & co.; when even that is not
    # enough, the package's Name.cxx (which includes what the header needs) is the unit.
    tu="$tmp/$name.cxx"; key="/$name."; rs="$out/$pkg/$name.hxx.rs"
    printf '#include <Standard_OStream.hxx>\n#include <Standard_IStream.hxx>\n#include "%s"\n' "$f" > "$tu"
    if ! clang++ -std=c++17 -fsyntax-only -I"$inc" $pkginc "$tu" 2> /dev/null && [ -f "${f%.hxx}.cxx" ]; then
      tu="${f%.hxx}.cxx"; key="/$name.hxx"
    fi
  fi
  if clang++ -std=c++17 -fsyntax-only -I"$inc" $pkginc -Xclang -ast-dump=json "$tu" > "$j" 2> "$tmp/$name.err" &&
     python3 "$here/cxx2rs.py" "$j" "$key" > "$rs" 2> "$tmp/$name.py.err"; then
    echo "$pkg/$name $(grep -c 'todo!' "$rs")" >> "$out/summary.txt"
  else
    echo "$pkg/$name" >> "$out/failures.txt"
  fi
  rm -f "$j"
}
export -f one; export out tmp inc here mode pkginc
ext=cxx; [ "$mode" = headers ] && ext=hxx
find "$dir" -mindepth 2 -maxdepth 2 -name "*.$ext" -not -path '*/GTests/*' | sort |
  xargs -P "$jobs" -I{} bash -c 'one "$1"' _ {}
rm -rf "$tmp"
echo "$tk: $(cat "$out/summary.txt" 2>/dev/null | wc -l) drafted, $(cat "$out/failures.txt" 2>/dev/null | wc -l) failed"

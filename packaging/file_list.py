"""Prints the per-file list of an install tree as JSON (ADR 0014 `files`).

Usage: python3 -I file_list.py <dir>

Regular files only, sorted by path, with `/` separators. Symlinks (the shared-library
soname links) are skipped; the files they point to are listed.
"""
import hashlib
import json
import os
import sys


def main() -> None:
    root = sys.argv[1]
    files = []
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames.sort()
        for name in sorted(filenames):
            path = os.path.join(dirpath, name)
            if os.path.islink(path) or not os.path.isfile(path):
                continue
            h = hashlib.sha256()
            with open(path, "rb") as f:
                for chunk in iter(lambda: f.read(1 << 20), b""):
                    h.update(chunk)
            rel = os.path.relpath(path, root).replace(os.sep, "/")
            files.append({"path": rel, "sha256": h.hexdigest(), "size": os.path.getsize(path)})
    files.sort(key=lambda e: e["path"])
    json.dump(files, sys.stdout, indent=1)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()

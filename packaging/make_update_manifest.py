"""Writes update-manifest.json for the in-app updater (ADR 0014, schema 1).

Usage:
  python3 -I make_update_manifest.py <dist-dir> <version> <tag> <owner/repo> <channel> \
      [--min-supported X.Y.Z] [--valid-days N] [--released YYYY-MM-DDTHH:MM:SSZ]

Reads the release artifacts in <dist-dir>, their `<artifact>.files.json` per-file lists
(packaging/stage.sh) and, when present, their `<artifact>.minisig` signatures. Without
signatures every `sig` is empty: such a manifest is refused by the app, as ADR 0014
requires until the signing key exists. The manifest itself is signed afterwards
(release.yml), because the signature must cover these exact bytes.
"""
import argparse
import datetime
import hashlib
import json
import os
import re
import sys

# Artifact file name -> (os, arch, install type). Must match gusset_update::InstallType.
PATTERNS = [
    (r"^Gusset-(?P<v>.+)-(?P<arch>x86_64|aarch64)\.AppImage$", "linux", "appimage"),
    (r"^gusset_(?P<v>.+)_(?P<arch>amd64|arm64)\.deb$", "linux", "deb"),
    (r"^gusset-(?P<v>.+)-windows-(?P<arch>x86_64|aarch64)\.zip$", "windows", "zip"),
    (r"^gusset-(?P<v>.+)-windows-(?P<arch>x86_64|aarch64)\.msi$", "windows", "msi-user"),
    (r"^Gusset-(?P<v>.+)-macos-(?P<arch>x86_64|aarch64)\.dmg$", "macos", "dmg"),
]
ARCH = {"amd64": "x86_64", "arm64": "aarch64", "x86_64": "x86_64", "aarch64": "aarch64"}


def sha256(path: str) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("dist")
    ap.add_argument("version")
    ap.add_argument("tag")
    ap.add_argument("repo")
    ap.add_argument("channel", choices=["stable", "nightly"])
    ap.add_argument("--min-supported", default="0.0.0")
    ap.add_argument("--valid-days", type=int, default=90)
    ap.add_argument("--released")
    a = ap.parse_args()
    if not 1 <= a.valid_days <= 120:
        sys.exit("--valid-days must be 1..120 (gusset_update::MAX_VALIDITY_SECS)")

    if a.released:
        released = datetime.datetime.strptime(a.released, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=datetime.timezone.utc)
    else:
        released = datetime.datetime.now(datetime.timezone.utc).replace(microsecond=0)
    expires = released + datetime.timedelta(days=a.valid_days)
    fmt = "%Y-%m-%dT%H:%M:%SZ"
    base = f"https://github.com/{a.repo}/releases/download/{a.tag}/"

    targets = {}
    for name in sorted(os.listdir(a.dist)):
        for pattern, os_name, install in PATTERNS:
            m = re.match(pattern, name)
            if not m:
                continue
            if m.group("v") != a.version:
                sys.exit(f"{name}: version does not match {a.version}")
            path = os.path.join(a.dist, name)
            key = f"{os_name}-{ARCH[m.group('arch')]}-{install}"
            sig_path = path + ".minisig"
            sig = open(sig_path, encoding="utf-8").read() if os.path.exists(sig_path) else ""
            files_path = path + ".files.json"
            files = json.load(open(files_path, encoding="utf-8")) if os.path.exists(files_path) else []
            if key in targets:
                sys.exit(f"duplicate target {key}")
            targets[key] = {
                "url": base + name,
                "size": os.path.getsize(path),
                "sha256": sha256(path),
                "sig": sig,
                "files": files,
            }
    if not targets:
        sys.exit("no release artifacts found")

    manifest = {
        "schema": 1,
        "channel": a.channel,
        "version": a.version,
        "released": released.strftime(fmt),
        "expires": expires.strftime(fmt),
        "min_supported": a.min_supported,
        "notes": f"Gusset {a.version}. See https://github.com/{a.repo}/releases/tag/{a.tag}",
        "revoked_keys": [],
        "targets": targets,
    }
    json.dump(manifest, sys.stdout, indent=2, sort_keys=True)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()

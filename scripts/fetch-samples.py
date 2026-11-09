#!/usr/bin/env python3
"""Fetch the public sample files the import tests and the reverse-engineering notes use.

Usage: python3 -I scripts/fetch-samples.py DEST [SET...]

Each set lands in its own new folder under DEST. Downloaded files are untrusted data:
nothing here runs them, and nothing that reads them should either (use `python3 -I`,
pass paths as arguments). Samples are never committed. Sets: freecad, slippad, zenodo,
arig, adafruit (default: all).

The samples back these tests, gated on environment variables:
  GUSSET_FCSTD_SAMPLES   DEST/freecad-examples
  GUSSET_SLDPRT_SAMPLES  DEST/slip-pad
  GUSSET_IPT_SAMPLES     DEST/zenodo        (Inventor parts, records 4309294, 11061412, 13941029, 8028536)
  GUSSET_OLE_SAMPLES     DEST/zenodo        (Solid Edge record 8028435 and the Inventor ones)
  GUSSET_F3D_SAMPLES     DEST/adafruit
"""

import hashlib
import json
import os
import subprocess
import sys
import urllib.parse
import urllib.request

UA = {"User-Agent": "gusset-sample-fetch"}

# Zenodo records (CC BY 4.0) with Solid Edge, Inventor, SolidWorks and Fusion files.
ZENODO = [8028435, 13941029, 11061412, 4309294, 8028536, 18646903]

# ARIG robotics team's SolidWorks projects: assemblies and part revisions (git LFS).
ARIG = "ARIG-Robotique/cad-projects"

# Adafruit's Fusion designs used by the Fusion notes (docs/formats/fusion-design-stream.md).
ADAFRUIT = "adafruit/Adafruit_CAD_Parts"
ADAFRUIT_NAMES = (
    "MacroPad", "Pico_Case", "BQ25185", "CyberDeck", "LoRa_Radio", "Pixel_Trinkey",
    "HDMI_Extension", "NeoKey", "QTPy_Case", "Crickit",
)

MAX_FILE = 512 << 20


def get(url, dest, sha256=None, md5=None):
    if os.path.exists(dest):
        return True
    req = urllib.request.Request(url, headers=UA)
    with urllib.request.urlopen(req, timeout=300) as r:
        data = r.read(MAX_FILE + 1)
    if len(data) > MAX_FILE:
        print("too large, skipped:", url)
        return False
    if sha256 and hashlib.sha256(data).hexdigest() != sha256:
        print("sha256 mismatch, skipped:", url)
        return False
    if md5 and hashlib.md5(data).hexdigest() != md5:
        print("md5 mismatch, skipped:", url)
        return False
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    with open(dest + ".tmp", "wb") as f:
        f.write(data)
    os.replace(dest + ".tmp", dest)
    return True


def git(*args, cwd=None):
    env = dict(os.environ, GIT_LFS_SKIP_SMUDGE="1", GIT_TERMINAL_PROMPT="0")
    return subprocess.run(["git", *args], cwd=cwd, env=env, check=True, capture_output=True, text=True).stdout


def clone(repo, dest, full=False):
    if os.path.isdir(dest):
        return
    extra = ["--filter=blob:none", "--no-checkout"] if full else ["--depth", "1"]
    git("clone", "-q", *extra, f"https://github.com/{repo}.git", dest)


def lfs(repo, rev, path, pointer, dest):
    oid = next((l.split(":", 1)[1] for l in pointer.splitlines() if l.startswith("oid sha256:")), None)
    if not oid:
        return False
    url = f"https://media.githubusercontent.com/media/{repo}/{rev}/{urllib.parse.quote(path)}"
    try:
        return get(url, dest, sha256=oid)
    except OSError as e:
        print("failed:", path, e)
        return False


def freecad(root):
    clone("FreeCAD/examples", os.path.join(root, "freecad-examples"))


def slippad(root):
    clone("Slip-Pad/Hardware", os.path.join(root, "slip-pad"))


def zenodo(root):
    for rec in ZENODO:
        req = urllib.request.Request(f"https://zenodo.org/api/records/{rec}", headers=UA)
        with urllib.request.urlopen(req, timeout=120) as r:
            meta = json.load(r)
        for f in meta.get("files", []):
            key = f.get("key", "")
            if "/" in key or key.startswith("."):
                continue
            md5 = (f.get("checksum") or "").removeprefix("md5:") or None
            link = (f.get("links") or {}).get("self")
            if link:
                try:
                    get(link, os.path.join(root, "zenodo", f"{rec}__{key}"), md5=md5)
                except OSError as e:
                    print("failed:", key, e)


def arig(root):
    hist = os.path.join(root, "arig-history")
    clone(ARIG, hist, full=True)
    head = git("rev-parse", "HEAD", cwd=hist).strip()
    # The assemblies at the head.
    for p in git("ls-tree", "-r", "--name-only", head, cwd=hist).splitlines():
        if p.lower().endswith(".sldasm"):
            ptr = git("show", f"{head}:{p}", cwd=hist)
            lfs(ARIG, head, p, ptr, os.path.join(root, "arig-sldasm", p))
    # Every revision of the most revised parts (revision-pair analysis).
    counts = {}
    for p in git("log", "--format=", "--name-only", "--", "*.SLDPRT", "*.sldprt", cwd=hist).splitlines():
        if p.strip():
            counts[p] = counts.get(p, 0) + 1
    for p in sorted(counts, key=lambda k: -counts[k])[:8]:
        for line in git("log", "--format=%H %ct %h", "--", p, cwd=hist).splitlines():
            c, t, h = line.split()
            try:
                ptr = git("show", f"{c}:{p}", cwd=hist)
            except subprocess.CalledProcessError:
                continue
            dest = os.path.join(root, "arig-revisions", p.replace("/", "_"), f"{t}_{h}.SLDPRT")
            lfs(ARIG, c, p, ptr, dest)


def adafruit(root):
    repo = os.path.join(root, "adafruit-cad")
    clone(ADAFRUIT, repo, full=True)
    head = git("rev-parse", "HEAD", cwd=repo).strip()
    for p in git("ls-tree", "-r", "--name-only", head, cwd=repo).splitlines():
        if p.lower().endswith((".f3d", ".f3z")) and any(n.lower() in p.lower() for n in ADAFRUIT_NAMES):
            dest = os.path.join(root, "adafruit", os.path.basename(p))
            url = f"https://raw.githubusercontent.com/{ADAFRUIT}/{head}/{urllib.parse.quote(p)}"
            try:
                get(url, dest)
            except OSError as e:
                print("failed:", p, e)


SETS = {"freecad": freecad, "slippad": slippad, "zenodo": zenodo, "arig": arig, "adafruit": adafruit}

if __name__ == "__main__":
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    root = os.path.abspath(sys.argv[1])
    os.makedirs(root, exist_ok=True)
    for name in sys.argv[2:] or list(SETS):
        print("fetching", name)
        try:
            SETS[name](root)
        except Exception as e:  # one failing set must not stop the others
            print(name, "failed:", e)

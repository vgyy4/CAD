#!/usr/bin/env python3
"""Regenerates the minisign test vectors in this directory.

TEST KEYS ONLY. The seeds below are public and must never sign anything real.
Requires Python 3 with the `cryptography` package. Writes the minisign formats by hand
(see https://jedisct1.github.io/minisign/), so no minisign binary is needed:

  public key  : base64("Ed" || key_id[8] || ed25519_pk[32])
  signature   : base64("ED" || key_id[8] || ed25519(BLAKE2b-512(message)))   (prehashed)
                base64("Ed" || key_id[8] || ed25519(message))                (legacy)
  global sig  : ed25519(signature[64] || trusted_comment)

Usage: python3 -I make_vectors.py <this directory>
"""
import base64
import hashlib
import json
import os
import sys

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat

KEYS = {
    # name: (32-byte seed, 8-byte key id as little-endian bytes)
    "a": (bytes([0xA1]) * 32, bytes.fromhex("0102030405060708")),
    "b": (bytes([0xB2]) * 32, bytes.fromhex("1112131415161718")),
    "c": (bytes([0xC3]) * 32, bytes.fromhex("2122232425262728")),  # never trusted
}


def key_id_display(kid: bytes) -> str:
    return "%016X" % int.from_bytes(kid, "little")


def pub_b64(name: str) -> str:
    seed, kid = KEYS[name]
    pk = Ed25519PrivateKey.from_private_bytes(seed).public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    return base64.b64encode(b"Ed" + kid + pk).decode()


def sign(name: str, message: bytes, legacy: bool = False) -> str:
    seed, kid = KEYS[name]
    sk = Ed25519PrivateKey.from_private_bytes(seed)
    if legacy:
        alg, sig = b"Ed", sk.sign(message)
    else:
        alg, sig = b"ED", sk.sign(hashlib.blake2b(message, digest_size=64).digest())
    trusted = "timestamp:1759276800\tfile:update-manifest.json\thashed"
    global_sig = sk.sign(sig + trusted.encode())
    return (
        f"untrusted comment: signature from minisign secret key\n"
        f"{base64.b64encode(alg + kid + sig).decode()}\n"
        f"trusted comment: {trusted}\n"
        f"{base64.b64encode(global_sig).decode()}\n"
    )


def manifest(revoked=()) -> bytes:
    pkg_sig = sign("a", b"package bytes")
    url = "https://github.com/example/gusset/releases/download/v0.2.0/"
    m = {
        "schema": 1,
        "channel": "stable",
        "version": "0.2.0",
        "released": "2026-10-01T00:00:00Z",
        "expires": "2026-12-30T00:00:00Z",
        "min_supported": "0.1.0",
        "notes": "Test release.",
        "revoked_keys": list(revoked),
        "targets": {
            "linux-x86_64-appimage": {
                "url": url + "Gusset-0.2.0-x86_64.AppImage",
                "size": 1234,
                "sha256": "ab" * 32,
                "sig": pkg_sig,
                "files": [{"path": "usr/bin/gusset", "sha256": "cd" * 32, "size": 10}],
                "delta_from": {
                    "0.1.0": {"url": url + "delta-0.1.0.zst", "size": 99, "sha256": "ef" * 32, "sig": pkg_sig}
                },
            },
            "linux-x86_64-deb": {
                "url": url + "gusset_0.2.0_amd64.deb",
                "size": 1000,
                "sha256": "01" * 32,
                "sig": pkg_sig,
            },
        },
    }
    return (json.dumps(m, indent=2) + "\n").encode()


def main() -> None:
    out = sys.argv[1]

    def write(name: str, data) -> None:
        with open(os.path.join(out, name), "wb") as f:
            f.write(data if isinstance(data, bytes) else data.encode())

    for k in KEYS:
        write(f"key-{k}.pub", f"untrusted comment: minisign public key {key_id_display(KEYS[k][1])}\n{pub_b64(k)}\n")
    newer = manifest()
    write("newer.json", newer)
    write("newer.json.minisig", sign("a", newer))
    write("newer.json.by-b.minisig", sign("b", newer))
    write("newer.json.by-c.minisig", sign("c", newer))
    write("newer.json.legacy.minisig", sign("a", newer, legacy=True))
    revoke_a = manifest(revoked=[key_id_display(KEYS["a"][1])])
    write("revoke-a.json", revoke_a)
    write("revoke-a.json.minisig", sign("b", revoke_a))
    write("self-revoke.json.minisig", sign("a", revoke_a))


if __name__ == "__main__":
    main()

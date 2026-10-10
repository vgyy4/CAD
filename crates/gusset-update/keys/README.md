# Update signing public keys

Public minisign keys that this build of Gusset trusts for update manifests (ADR 0014,
"Key management"). The directory holds no key until the maintainer generates the
release keypair. Until then `BUILTIN_KEYS` in `src/lib.rs` is empty and the app refuses
every manifest, so no update is ever installed unverified.

Adding a key:
1. The maintainer runs `minisign -G` (or `rsign generate`) offline and stores the
   secret key only in the protected `release` environment as `UPDATE_SIGNING_KEY`
   (and its password as `UPDATE_SIGNING_KEY_PASSWORD`).
2. Commit the public key here as `<name>.pub`, and add its second line to
   `BUILTIN_KEYS` together with the name.
3. Rotation: ship release N with {old, new}, sign with new from N+1, and drop old after
   the support window. Compromise: sign a manifest with a remaining key whose
   `revoked_keys` lists the compromised key ID, then rotate.

The keys under `tests/data/` are test keys with public seeds and must never be added
here.

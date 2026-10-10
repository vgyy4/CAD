//! Tests with the TEST-ONLY keys in `tests/data/` (see `make_vectors.py` there).

use super::*;

macro_rules! data {
    ($f:literal) => {
        include_str!(concat!("../tests/data/", $f))
    };
}

/// Second line of a minisign `.pub` file.
fn pubkey(file: &str) -> &str {
    file.lines().nth(1).unwrap()
}

fn ring_ab() -> KeyRing {
    KeyRing::new(&[("test-a", pubkey(data!("key-a.pub"))), ("test-b", pubkey(data!("key-b.pub")))]).unwrap()
}

const NEWER: &str = data!("newer.json");
const KEY_A: &str = "0807060504030201";
const KEY_B: &str = "1817161514131211";
/// 2026-10-10T00:00:00Z
const NOW: u64 = 1_791_590_400;

fn installed(version: &str, install_type: InstallType) -> Installed {
    Installed {
        version: Version::parse(version).unwrap(),
        channel: Channel::Stable,
        os: "linux",
        arch: "x86_64",
        install_type,
        url_prefix: "https://github.com/example/gusset/releases/download/".into(),
    }
}

fn verified() -> VerifiedManifest {
    verify_manifest(NEWER.as_bytes(), data!("newer.json.minisig"), &mut ring_ab()).unwrap()
}

/// A verified manifest with a modified body, for policy checks (the signature step is
/// tested separately).
fn edited(f: impl FnOnce(&mut Manifest)) -> VerifiedManifest {
    let mut v = verified();
    f(&mut v.manifest);
    v
}

#[test]
fn builtin_ring_has_no_keys_and_refuses_everything() {
    let mut ring = KeyRing::builtin().unwrap();
    assert!(ring.trusted().is_empty());
    assert_eq!(
        verify_manifest(NEWER.as_bytes(), data!("newer.json.minisig"), &mut ring),
        Err(UpdateError::NoTrustedKeys)
    );
}

#[test]
fn key_ids_display_like_minisign() {
    let ring = ring_ab();
    let ids: Vec<String> = ring.trusted().iter().map(|(id, _)| id.to_string()).collect();
    assert_eq!(ids, [KEY_A, KEY_B]);
    assert!(data!("key-a.pub").contains(KEY_A));
    assert_eq!(KeyId::parse(&KEY_A.to_lowercase()), KeyId::parse(KEY_A));
    assert_eq!(KeyId::parse("123"), None);
    assert_eq!(KeyId::parse("zz07060504030201"), None);
}

#[test]
fn valid_signature_verifies_with_either_trusted_key() {
    let v = verified();
    assert_eq!(v.signed_by.to_string(), KEY_A);
    assert_eq!(v.manifest.version, "0.2.0");
    let v = verify_manifest(NEWER.as_bytes(), data!("newer.json.by-b.minisig"), &mut ring_ab()).unwrap();
    assert_eq!(v.signed_by.to_string(), KEY_B);
}

#[test]
fn untrusted_key_tampering_and_legacy_signatures_are_refused() {
    let mut ring = ring_ab();
    let err = verify_manifest(NEWER.as_bytes(), data!("newer.json.by-c.minisig"), &mut ring);
    assert_eq!(err, Err(UpdateError::BadSignature));
    // One changed byte anywhere in the manifest.
    let tampered = NEWER.replacen("0.2.0", "9.2.0", 1);
    let err = verify_manifest(tampered.as_bytes(), data!("newer.json.minisig"), &mut ring);
    assert_eq!(err, Err(UpdateError::BadSignature));
    let mut trailing = NEWER.as_bytes().to_vec();
    trailing.push(b' ');
    assert_eq!(verify_manifest(&trailing, data!("newer.json.minisig"), &mut ring), Err(UpdateError::BadSignature));
    // Non-prehashed (legacy) signatures are not accepted, even when valid.
    let err = verify_manifest(NEWER.as_bytes(), data!("newer.json.legacy.minisig"), &mut ring);
    assert_eq!(err, Err(UpdateError::BadSignature));
    // A tampered trusted comment breaks the global signature.
    let sig = data!("newer.json.minisig").replace("timestamp:1759276800", "timestamp:1759276801");
    assert_eq!(verify_manifest(NEWER.as_bytes(), &sig, &mut ring), Err(UpdateError::BadSignature));
    assert!(ring.revoked().is_empty());
}

#[test]
fn malformed_signatures_and_oversized_inputs_are_refused() {
    let mut ring = ring_ab();
    for sig in ["", "garbage", "untrusted comment: x\nnot base64\n", &"A".repeat(10_000)] {
        let r = verify_manifest(NEWER.as_bytes(), sig, &mut ring);
        assert!(matches!(r, Err(UpdateError::BadSignatureFormat(_) | UpdateError::TooLarge { .. })), "{sig:?}: {r:?}");
    }
    let big = vec![b' '; MAX_MANIFEST_BYTES + 1];
    assert!(matches!(
        verify_manifest(&big, data!("newer.json.minisig"), &mut ring),
        Err(UpdateError::TooLarge { what: "manifest", .. })
    ));
}

#[test]
fn revocation_removes_keys_and_sticks() {
    let mut ring = ring_ab();
    // A manifest signed by B revokes A.
    let v = verify_manifest(data!("revoke-a.json").as_bytes(), data!("revoke-a.json.minisig"), &mut ring).unwrap();
    assert_eq!(v.signed_by.to_string(), KEY_B);
    assert_eq!(ring.revoked().iter().map(ToString::to_string).collect::<Vec<_>>(), [KEY_A]);
    assert_eq!(ring.trusted().len(), 1);
    // Afterwards, manifests signed by A are refused, even old valid ones.
    let r = verify_manifest(NEWER.as_bytes(), data!("newer.json.minisig"), &mut ring);
    assert_eq!(r, Err(UpdateError::RevokedKey(KeyId::parse(KEY_A).unwrap())));
    // The revocation persists through a restart.
    let saved: Vec<KeyId> = ring.revoked().iter().copied().collect();
    let mut restarted = ring_ab().with_revoked(saved);
    let r = verify_manifest(NEWER.as_bytes(), data!("newer.json.minisig"), &mut restarted);
    assert!(matches!(r, Err(UpdateError::RevokedKey(_))));
    // Once every key is revoked, nothing verifies.
    let mut none = ring_ab().with_revoked([KeyId::parse(KEY_A).unwrap(), KeyId::parse(KEY_B).unwrap()]);
    let r = verify_manifest(NEWER.as_bytes(), data!("newer.json.by-b.minisig"), &mut none);
    assert_eq!(r, Err(UpdateError::NoTrustedKeys));
}

#[test]
fn a_manifest_revoking_its_own_signing_key_is_refused() {
    let mut ring = ring_ab();
    let r = verify_manifest(data!("revoke-a.json").as_bytes(), data!("self-revoke.json.minisig"), &mut ring);
    assert_eq!(r, Err(UpdateError::RevokedKey(KeyId::parse(KEY_A).unwrap())));
    assert!(ring.revoked().is_empty(), "a refused manifest must not change the key ring");
}

#[test]
fn newer_version_is_reported_with_its_package() {
    let UpdateCheck::Available(u) = check(&verified(), &installed("0.1.0", InstallType::AppImage), NOW).unwrap() else {
        panic!("expected an update")
    };
    assert_eq!(u.version, Version::new(0, 2, 0));
    assert_eq!(u.apply, ApplyMode::InApp);
    assert_eq!(u.package.package.size, 1234);
    assert_eq!(u.package.files.len(), 1);
    assert!(u.delta.as_ref().unwrap().url.ends_with("delta-0.1.0.zst"));
    assert_eq!(u.notes, "Test release.");
    // No delta for other versions.
    let UpdateCheck::Available(u) = check(&verified(), &installed("0.1.5", InstallType::AppImage), NOW).unwrap() else {
        panic!("expected an update")
    };
    assert!(u.delta.is_none());
}

#[test]
fn same_or_older_versions_are_up_to_date() {
    for v in ["0.2.0", "0.2.1", "1.0.0"] {
        assert_eq!(check(&verified(), &installed(v, InstallType::AppImage), NOW), Ok(UpdateCheck::UpToDate), "{v}");
    }
    // A pre-release of the same version is older, so the release is offered.
    assert!(matches!(
        check(&verified(), &installed("0.2.0-nightly.5", InstallType::AppImage), NOW),
        Ok(UpdateCheck::Available(_))
    ));
}

#[test]
fn expiry_and_validity_window_are_enforced() {
    let i = installed("0.1.0", InstallType::AppImage);
    let expires = parse_utc("2026-12-30T00:00:00Z").unwrap();
    assert!(check(&verified(), &i, expires - 1).is_ok());
    assert!(matches!(check(&verified(), &i, expires), Err(UpdateError::Expired { .. })));
    let long = edited(|m| m.expires = "2027-06-01T00:00:00Z".into());
    assert_eq!(check(&long, &i, NOW), Err(UpdateError::ValidityTooLong));
    let backwards = edited(|m| m.expires = m.released.clone());
    assert!(matches!(check(&backwards, &i, NOW), Err(UpdateError::BadManifest(_))));
    let bad_time = edited(|m| m.expires = "2026-12-30 00:00:00".into());
    assert!(matches!(check(&bad_time, &i, NOW), Err(UpdateError::BadManifest(_))));
}

#[test]
fn channel_and_target_must_match() {
    let mut i = installed("0.1.0", InstallType::AppImage);
    i.channel = Channel::Nightly;
    assert!(matches!(check(&verified(), &i, NOW), Err(UpdateError::WrongChannel { .. })));
    let mut i = installed("0.1.0", InstallType::AppImage);
    i.arch = "aarch64";
    assert_eq!(check(&verified(), &i, NOW), Err(UpdateError::NoTarget("linux-aarch64-appimage".into())));
}

#[test]
fn package_managed_installs_only_notify_and_old_installs_reinstall() {
    let UpdateCheck::Available(u) = check(&verified(), &installed("0.1.0", InstallType::Deb), NOW).unwrap() else {
        panic!()
    };
    assert_eq!(u.apply, ApplyMode::NotifyOnly);
    let UpdateCheck::Available(u) = check(&verified(), &installed("0.0.9", InstallType::AppImage), NOW).unwrap() else {
        panic!()
    };
    assert_eq!(u.apply, ApplyMode::ReinstallRequired);
}

#[test]
fn package_descriptions_are_validated() {
    let i = installed("0.1.0", InstallType::AppImage);
    let t = "linux-x86_64-appimage";
    type Edit = Box<dyn Fn(&mut Manifest)>;
    let cases: Vec<Edit> = vec![
        Box::new(|m| {
            m.targets.get_mut(t).unwrap().package.url = "http://github.com/example/gusset/releases/download/x".into()
        }),
        Box::new(|m| m.targets.get_mut(t).unwrap().package.url = "https://evil.example/gusset.AppImage".into()),
        Box::new(|m| {
            m.targets.get_mut(t).unwrap().package.url =
                "https://github.com/example/gusset/releases/download/a b".into();
        }),
        Box::new(|m| m.targets.get_mut(t).unwrap().package.sha256 = "AB".repeat(32)),
        Box::new(|m| m.targets.get_mut(t).unwrap().package.sha256 = "ab".into()),
        Box::new(|m| m.targets.get_mut(t).unwrap().package.size = 0),
        Box::new(|m| m.targets.get_mut(t).unwrap().package.sig = "not a signature".into()),
        Box::new(|m| m.targets.get_mut(t).unwrap().files[0].path = "../outside".into()),
        Box::new(|m| m.targets.get_mut(t).unwrap().files[0].sha256 = String::new()),
        Box::new(|m| {
            let f = m.targets[t].files[0].clone();
            m.targets.get_mut(t).unwrap().files.push(FileEntry { path: f.path.to_uppercase(), ..f });
        }),
        Box::new(|m| {
            m.targets.get_mut(t).unwrap().delta_from.get_mut("0.1.0").unwrap().url = "https://x.example/".into()
        }),
        Box::new(|m| {
            let d = m.targets[t].delta_from["0.1.0"].clone();
            m.targets.get_mut(t).unwrap().delta_from.insert("not-a-version".into(), d);
        }),
        Box::new(|m| m.version = "v0.2".into()),
        Box::new(|m| m.min_supported = String::new()),
    ];
    for (n, f) in cases.into_iter().enumerate() {
        let v = edited(f);
        assert!(matches!(check(&v, &i, NOW), Err(UpdateError::BadManifest(_))), "case {n}");
    }
}

#[test]
fn unsigned_edits_are_refused_before_parsing() {
    // Signature checks come first: an unsigned edit is a signature failure, never a
    // parse of attacker-controlled JSON.
    let edited = NEWER.replacen("\"schema\": 1", "\"schema\": 2", 1);
    assert_eq!(
        verify_manifest(edited.as_bytes(), data!("newer.json.minisig"), &mut ring_ab()),
        Err(UpdateError::BadSignature)
    );
}

#[test]
fn safe_relative_paths() {
    for ok in ["usr/bin/gusset", "gusset.exe", "lib/gusset/libTKernel.so.8.0.1", "a/.hidden"] {
        assert!(is_safe_relative_path(ok), "{ok}");
    }
    for bad in [
        "",
        "/etc/passwd",
        "../x",
        "a/../b",
        "a//b",
        "./a",
        "a/",
        "C:/x",
        "a\\b",
        "con",
        "a/NUL.txt",
        "COM1",
        "lpt9.log",
        "a.",
        "a ",
        "x\u{0}y",
        "a\nb",
    ] {
        assert!(!is_safe_relative_path(bad), "{bad:?}");
    }
    assert!(is_safe_relative_path("COM10"));
}

#[test]
fn utc_parsing() {
    assert_eq!(parse_utc("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(parse_utc("2000-03-01T00:00:00Z"), Some(951_868_800));
    assert_eq!(parse_utc("2026-10-10T00:00:00Z"), Some(NOW));
    assert_eq!(parse_utc("2024-02-29T23:59:59Z"), Some(1_709_251_199));
    for bad in [
        "2023-02-29T00:00:00Z",
        "2026-13-01T00:00:00Z",
        "2026-00-01T00:00:00Z",
        "2026-01-32T00:00:00Z",
        "2026-01-01T24:00:00Z",
        "2026-01-01T00:00:00+01:00",
        "2026-01-01 00:00:00Z",
        "1969-12-31T23:59:59Z",
        "+026-01-01T00:00:00Z",
        "",
    ] {
        assert_eq!(parse_utc(bad), None, "{bad}");
    }
}

#[test]
fn base64_decoder_matches_known_vectors() {
    assert_eq!(base64_decode("").unwrap(), b"");
    assert_eq!(base64_decode("Zg==").unwrap(), b"f");
    assert_eq!(base64_decode("Zm8=").unwrap(), b"fo");
    assert_eq!(base64_decode("Zm9v").unwrap(), b"foo");
    assert_eq!(base64_decode("Zm9vYmFy").unwrap(), b"foobar");
    assert_eq!(base64_decode("Zm9"), None);
    assert_eq!(base64_decode("Z==="), None);
    assert_eq!(base64_decode("Zm9-"), None);
}

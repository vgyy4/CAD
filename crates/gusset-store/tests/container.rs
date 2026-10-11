//! `.gus` container: round trip, dedupe, copy-through, corruption, truncation, hostile
//! inputs and salvage.

// Test-only crate: clippy.toml already allows these inside #[test] functions; the
// same allowance covers the helpers here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

mod common;

use std::collections::BTreeSet;

use common::Rng;
use gusset_store::Hash;
use gusset_store::checksum::crc32c;
use gusset_store::container::{
    self, Codec, ContainerWriter, HEADER_LEN, Kind, Limits, OpenError, RecordProblem, SaveInfo, TRAILER_LEN, WriteError,
};

const INFO: SaveInfo<'static> = SaveInfo { save_seq: 7, created_unix: 1_700_000_000, app_version: "0.0.1-test" };

/// Several records of different kinds and codecs plus two roots.
fn sample() -> (Vec<u8>, Vec<(Kind, Vec<u8>)>) {
    let mut rng = Rng(42);
    let items = vec![
        (Kind::PROJECT_META, b"gusset meta 1\nname Bracket\n".to_vec()),
        (Kind::FEATURE, b"gusset feature 1\nsketch s1 on XY\nextrude s1 10 mm\n".repeat(20)),
        (Kind::PARAMS, b"gusset params 1\nwidth = 40 mm\n".to_vec()),
        (Kind::BLOB, vec![0u8; 5000]),
        (Kind::BLOB, rng.bytes(700)),
        (Kind::EVENTS, Vec::new()),
    ];
    let mut w = ContainerWriter::new();
    for (k, d) in &items {
        w.add(*k, d).unwrap();
    }
    w.set_root("branches", Hash::of(&items[1].1).as_bytes()).unwrap();
    w.set_root("settings", b"units=mm").unwrap();
    (w.finish(&INFO).unwrap(), items)
}

#[test]
fn round_trip() {
    let (bytes, items) = sample();
    let c = container::open(&bytes).unwrap();
    assert_eq!(c.header().save_seq, 7);
    assert_eq!(c.header().app_version, "0.0.1-test");
    assert_eq!(c.records().len(), items.len());
    for (k, d) in &items {
        let h = Hash::of(d);
        assert_eq!(c.info(&h).unwrap().kind, *k);
        assert_eq!(c.get(&h).unwrap().unwrap(), *d);
    }
    assert_eq!(c.get(&Hash::of(b"absent")).unwrap(), None);
    assert_eq!(c.root("settings"), Some(&b"units=mm"[..]));
    assert_eq!(c.roots().len(), 2);
    container::verify(&bytes).unwrap();
}

#[test]
fn codecs_raw_fallback_and_compression() {
    let (bytes, items) = sample();
    let c = container::open(&bytes).unwrap();
    assert_eq!(c.info(&Hash::of(&items[3].1)).unwrap().codec, Codec::Zstd);
    assert!(c.info(&Hash::of(&items[3].1)).unwrap().stored_len < 200);
    assert_eq!(c.info(&Hash::of(&items[4].1)).unwrap().codec, Codec::Raw); // random: no gain
    assert_eq!(c.info(&Hash::of(&items[5].1)).unwrap().codec, Codec::Raw); // empty
}

#[test]
fn deterministic_output() {
    assert_eq!(sample().0, sample().0);
}

#[test]
fn dedupe_and_kind_conflict() {
    let mut w = ContainerWriter::new();
    let a = w.add(Kind::FEATURE, b"same").unwrap();
    let b = w.add(Kind::FEATURE, b"same").unwrap();
    assert_eq!(a, b);
    assert!(matches!(w.add(Kind::PARAMS, b"same"), Err(WriteError::KindConflict { .. })));
    assert!(matches!(w.add(Kind(0), b"x"), Err(WriteError::InvalidKind)));
    let bytes = w.finish(&INFO).unwrap();
    assert_eq!(container::open(&bytes).unwrap().records().len(), 1);

    let mut once = ContainerWriter::new();
    once.add(Kind::FEATURE, b"same").unwrap();
    assert_eq!(once.finish(&INFO).unwrap(), bytes);
}

#[test]
fn writer_enforces_limits_and_root_names() {
    let limits = Limits { max_raw_len: 10, ..Limits::default() };
    let mut w = ContainerWriter::new().with_limits(limits);
    assert!(matches!(w.add(Kind::FEATURE, &[1; 11]), Err(WriteError::RecordTooLarge { .. })));
    w.add(Kind::BLOB, &[1; 11]).unwrap(); // large kinds have their own limit
    assert_eq!(w.set_root("", b""), Err(WriteError::InvalidRootName));
    assert_eq!(w.set_root("a\nb", b""), Err(WriteError::InvalidRootName));
    assert_eq!(w.set_root(&"x".repeat(256), b""), Err(WriteError::InvalidRootName));
    w.set_root(&"x".repeat(255), b"ok").unwrap();
}

#[test]
fn copy_through_is_byte_identical() {
    let (bytes, items) = sample();
    let src = container::open(&bytes).unwrap();
    let mut w = ContainerWriter::new().with_level(19); // a different level must not matter
    for r in src.records() {
        assert_eq!(w.copy_from(&src, &r.hash).unwrap(), r.hash);
    }
    let new_item = w.add(Kind::NODE, b"gusset node 1\nparent x\n").unwrap();
    assert!(w.copy_from(&src, &Hash::of(b"nope")).is_err());
    let out = w.finish(&SaveInfo { save_seq: 8, ..INFO }).unwrap();
    let dst = container::open(&out).unwrap();
    for r in src.records() {
        let a = &bytes[r.offset as usize..][..46 + r.stored_len as usize + 4];
        let d = dst.info(&r.hash).unwrap();
        let b = &out[d.offset as usize..][..46 + d.stored_len as usize + 4];
        assert_eq!(a, b, "record {} not copied verbatim", r.hash);
    }
    assert!(dst.contains(&new_item));
    assert_eq!(dst.records().len(), items.len() + 1);
}

/// Record intervals (start, end) of a valid container.
fn record_spans(bytes: &[u8]) -> Vec<(usize, usize, Hash)> {
    container::open(bytes)
        .unwrap()
        .records()
        .iter()
        .map(|r| (r.offset as usize, r.offset as usize + 46 + r.stored_len as usize + 4, r.hash))
        .collect()
}

#[test]
fn every_single_byte_corruption_is_detected_and_salvage_recovers_the_rest() {
    let (bytes, _) = sample();
    let spans = record_spans(&bytes);
    let all: BTreeSet<Hash> = spans.iter().map(|s| s.2).collect();
    for i in 0..bytes.len() {
        for mask in [0x01u8, 0x80, 0xff] {
            let mut d = bytes.clone();
            d[i] ^= mask;
            assert!(container::open(&d).is_err(), "flip {mask:#x} at {i} not detected");
            let s = container::salvage(&d);
            let got: BTreeSet<Hash> = s.records.iter().map(|r| r.hash).collect();
            // Every record not containing the damaged byte is recovered intact.
            let expected: BTreeSet<Hash> =
                spans.iter().filter(|(a, b, _)| !(*a..*b).contains(&i)).map(|s| s.2).collect();
            assert!(got.is_superset(&expected), "flip at {i}: lost an undamaged record");
            assert!(got.is_subset(&all));
            for r in &s.records {
                assert_eq!(Hash::of(&r.data), r.hash);
            }
            // Header damage loses only the header; index/trailer damage only the roots.
            if i >= HEADER_LEN {
                assert!(s.header.is_some());
            }
        }
    }
}

#[test]
fn truncation_at_every_offset_is_detected_and_salvageable() {
    let (bytes, _) = sample();
    let spans = record_spans(&bytes);
    for k in 0..bytes.len() {
        let t = &bytes[..k];
        assert!(container::open(t).is_err(), "truncation to {k} bytes not detected");
        let s = container::salvage(t);
        let got: BTreeSet<Hash> = s.records.iter().map(|r| r.hash).collect();
        let expected: BTreeSet<Hash> = spans.iter().filter(|(_, end, _)| *end <= k).map(|s| s.2).collect();
        assert_eq!(got, expected, "truncated to {k}");
    }
    // Appended garbage is detected too.
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(container::open(&longer).is_err());
}

#[test]
fn salvage_recovers_roots_when_only_the_trailer_is_damaged() {
    let (bytes, items) = sample();
    let mut d = bytes.clone();
    let n = d.len();
    d[n - 3] ^= 0xff; // trailer CRC
    let s = container::salvage(&d);
    assert_eq!(s.records.len(), items.len());
    assert_eq!(s.roots.as_ref().unwrap().get("settings").map(Vec::as_slice), Some(&b"units=mm"[..]));
    assert!(s.complete);
    // Index damaged as well: records survive, roots do not.
    let idx = d.windows(4).rposition(|w| w == b"GIDX").unwrap();
    d[idx + 6] ^= 0x55;
    let s = container::salvage(&d);
    assert_eq!(s.records.len(), items.len());
    assert!(s.roots.is_none());
}

#[test]
fn error_messages_say_what_and_where() {
    let (bytes, _) = sample();
    let spans = record_spans(&bytes);
    let mut d = bytes.clone();
    d[spans[1].0 + 50] ^= 1;
    let e = container::open(&d).unwrap_err();
    assert!(matches!(e, OpenError::Record { number: 1, problem: RecordProblem::Checksum { .. }, .. }), "{e:?}");
    let msg = e.to_string();
    assert!(msg.contains("record #1") && msg.contains(&format!("byte {}", spans[1].0)), "{msg}");
    assert_eq!(container::open(b"hello").unwrap_err(), OpenError::TooSmall { len: 5 });
    assert_eq!(container::open(&[0u8; 200]).unwrap_err(), OpenError::NotAGussetFile);
}

// ---- hostile, hand-crafted files -------------------------------------------------

fn header(seq: u64, version: u16) -> Vec<u8> {
    let mut h = Vec::new();
    h.extend_from_slice(b"GUSSET\r\n\x1a\0");
    h.extend_from_slice(&version.to_le_bytes());
    h.extend_from_slice(&0u32.to_le_bytes());
    h.extend_from_slice(&seq.to_le_bytes());
    h.extend_from_slice(&0u64.to_le_bytes());
    h.extend_from_slice(&[0u8; 16]);
    h.resize(60, 0);
    let c = crc32c(&h);
    h.extend_from_slice(&c.to_le_bytes());
    h
}

fn record(kind: u8, codec: u8, hash: &Hash, stored: &[u8], stored_len: u32, raw_len: u32) -> Vec<u8> {
    let mut r = b"GREC".to_vec();
    r.push(kind);
    r.push(codec);
    r.extend_from_slice(hash.as_bytes());
    r.extend_from_slice(&stored_len.to_le_bytes());
    r.extend_from_slice(&raw_len.to_le_bytes());
    r.extend_from_slice(stored);
    let c = crc32c(&r);
    r.extend_from_slice(&c.to_le_bytes());
    r
}

/// Assembles header + records + an index listing them (with a correct CRC) + trailer.
fn craft(records: &[(u8, Hash, Vec<u8>)], count_override: Option<u32>) -> Vec<u8> {
    let mut f = header(1, 1);
    let mut entries = Vec::new();
    for (kind, hash, bytes) in records {
        entries.push((*kind, *hash, f.len() as u64));
        f.extend_from_slice(bytes);
    }
    let index_offset = f.len() as u64;
    let mut idx = b"GIDX".to_vec();
    idx.extend_from_slice(&count_override.unwrap_or(entries.len() as u32).to_le_bytes());
    for (k, h, o) in &entries {
        idx.push(*k);
        idx.extend_from_slice(h.as_bytes());
        idx.extend_from_slice(&o.to_le_bytes());
    }
    idx.extend_from_slice(&0u32.to_le_bytes());
    let c = crc32c(&idx);
    idx.extend_from_slice(&c.to_le_bytes());
    f.extend_from_slice(&idx);
    let mut t = Vec::new();
    t.extend_from_slice(&index_offset.to_le_bytes());
    t.extend_from_slice(&(idx.len() as u32).to_le_bytes());
    t.extend_from_slice(&1u64.to_le_bytes());
    t.extend_from_slice(b"GSEND\0\0\0");
    let c = crc32c(&t);
    t.extend_from_slice(&c.to_le_bytes());
    f.extend_from_slice(&t);
    f
}

#[test]
fn crafted_valid_file_opens() {
    let data = b"hello";
    let h = Hash::of(data);
    let f = craft(&[(1, h, record(1, 0, &h, data, 5, 5))], None);
    assert_eq!(container::open(&f).unwrap().get(&h).unwrap().unwrap(), data);
}

#[test]
fn huge_declared_lengths_are_rejected_without_allocating() {
    let h = Hash::of(b"x");
    // stored_len far beyond the file.
    let f = craft(&[(1, h, record(1, 0, &h, b"x", 0xFFFF_FFF0, 0xFFFF_FFF0))], None);
    assert!(matches!(container::open(&f), Err(OpenError::Record { problem: RecordProblem::TooLarge { .. }, .. })));
    let f = craft(&[(7, h, record(7, 0, &h, b"x", 0x3000_0000, 0x3000_0000))], None);
    assert!(matches!(container::open(&f), Err(OpenError::Record { problem: RecordProblem::Truncated, .. })));
    // Index claims 4 billion records.
    let f = craft(&[], Some(u32::MAX));
    assert!(matches!(container::open(&f), Err(OpenError::IndexDamaged { .. })));
    // Index claims 10 M records (allowed count) but has none.
    let f = craft(&[], Some(10_000_000));
    assert!(matches!(container::open(&f), Err(OpenError::IndexDamaged { .. })));
    // Trailer points far outside the file.
    let mut f = craft(&[], None);
    let n = f.len();
    let mut t = Vec::new();
    t.extend_from_slice(&u64::MAX.to_le_bytes());
    t.extend_from_slice(&u32::MAX.to_le_bytes());
    t.extend_from_slice(&1u64.to_le_bytes());
    t.extend_from_slice(b"GSEND\0\0\0");
    let c = crc32c(&t);
    t.extend_from_slice(&c.to_le_bytes());
    f[n - TRAILER_LEN..].copy_from_slice(&t);
    assert!(matches!(container::open(&f), Err(OpenError::IndexOutOfBounds { .. })));
    // Newer format version.
    let mut f = craft(&[], None);
    f[..HEADER_LEN].copy_from_slice(&header(1, 99));
    assert!(matches!(container::open(&f), Err(OpenError::UnsupportedVersion { found: 99, .. })));
}

#[test]
fn decompression_bombs_are_bounded() {
    // 64 MiB of zeros compresses to a few KiB.
    let big = vec![0u8; 64 << 20];
    let z = zstd::bulk::compress(&big, 3).unwrap();
    let h = Hash::of(&big);
    // Declared small: decoding stops after raw_len + 1 bytes.
    let declared = z.len() as u32 + 100;
    let f = craft(&[(7, h, record(7, 1, &h, &z, z.len() as u32, declared))], None);
    let e = container::open(&f).unwrap_err();
    let actual = u64::from(declared) + 1;
    assert!(
        matches!(e, OpenError::Record { problem: RecordProblem::LengthMismatch { expected, actual: a }, .. }
            if expected == declared && a == actual),
        "{e:?}"
    );
    // Declared truthfully but as a small kind: over the 16 MiB limit.
    let f = craft(&[(1, h, record(1, 1, &h, &z, z.len() as u32, 64 << 20))], None);
    assert!(matches!(container::open(&f), Err(OpenError::Record { problem: RecordProblem::TooLarge { .. }, .. })));
    // Declared truthfully as a blob: fine with default limits, refused by a lower total budget.
    let f = craft(&[(7, h, record(7, 1, &h, &z, z.len() as u32, 64 << 20))], None);
    assert!(container::open(&f).is_ok());
    let tight = Limits { max_total_raw: 1 << 20, ..Limits::default() };
    assert!(matches!(container::open_with(&f, tight.clone()), Err(OpenError::DecodeBudget { .. })));
    let s = container::salvage_with(&f, &tight);
    assert!(s.records.is_empty() && !s.complete);
}

#[test]
fn zstd_window_is_capped() {
    // A frame whose window (48 MiB, single segment) exceeds the 32 MiB cap.
    let mut rng = Rng(7);
    let chunk = rng.bytes(1 << 16);
    let data: Vec<u8> = chunk.iter().copied().cycle().take(48 << 20).collect();
    let mut c = zstd::bulk::Compressor::new(1).unwrap();
    c.set_parameter(zstd::stream::raw::CParameter::WindowLog(26)).unwrap();
    let z = c.compress(&data).unwrap();
    let h = Hash::of(&data);
    let f = craft(&[(7, h, record(7, 1, &h, &z, z.len() as u32, data.len() as u32))], None);
    let e = container::open(&f).unwrap_err();
    assert!(matches!(e, OpenError::Record { problem: RecordProblem::Decompress(_), .. }), "{e:?}");
    let relaxed = Limits { window_log_max: 27, ..Limits::default() };
    assert!(container::open_with(&f, relaxed).is_ok());
}

#[test]
fn wrong_hash_or_index_mismatch_is_detected() {
    let data = b"hello";
    let h = Hash::of(data);
    let wrong = Hash::of(b"other");
    let f = craft(&[(1, wrong, record(1, 0, &wrong, data, 5, 5))], None);
    assert!(matches!(container::open(&f), Err(OpenError::Record { problem: RecordProblem::HashMismatch, .. })));
    let f = craft(&[(2, h, record(1, 0, &h, data, 5, 5))], None);
    assert!(matches!(container::open(&f), Err(OpenError::Record { problem: RecordProblem::IndexMismatch, .. })));
    let r = record(1, 0, &h, data, 5, 5);
    let f = craft(&[(1, h, r.clone()), (1, h, r)], None);
    assert!(matches!(container::open(&f), Err(OpenError::DuplicateRecord { .. })));
    // Raw codec with stored != raw, zstd codec that does not shrink, unknown codec.
    let f = craft(&[(1, h, record(1, 0, &h, data, 5, 6))], None);
    assert!(matches!(
        container::open(&f),
        Err(OpenError::Record { problem: RecordProblem::StoredLenInvalid { .. }, .. })
    ));
    let f = craft(&[(1, h, record(1, 1, &h, data, 5, 5))], None);
    assert!(matches!(
        container::open(&f),
        Err(OpenError::Record { problem: RecordProblem::StoredLenInvalid { .. }, .. })
    ));
    let f = craft(&[(1, h, record(1, 9, &h, data, 5, 5))], None);
    assert!(matches!(container::open(&f), Err(OpenError::Record { problem: RecordProblem::UnknownCodec(9), .. })));
}

#[test]
fn random_garbage_and_mutations_never_panic() {
    let (bytes, _) = sample();
    let mut rng = Rng(0xDEAD_BEEF);
    for round in 0..3000 {
        let mut d = if round % 3 == 0 {
            let n = rng.below(600) as usize;
            let mut g = rng.bytes(n);
            if round % 2 == 0 && n > 20 {
                g[..10].copy_from_slice(b"GUSSET\r\n\x1a\0");
                let at = rng.below(n as u64 - 4) as usize;
                g[at..at + 4].copy_from_slice(b"GREC");
            }
            g
        } else {
            bytes.clone()
        };
        for _ in 0..=rng.below(8) {
            if d.is_empty() {
                break;
            }
            let i = rng.below(d.len() as u64) as usize;
            match rng.below(4) {
                0 => d[i] = rng.next() as u8,
                1 => d.truncate(i),
                2 => d.insert(i, rng.next() as u8),
                _ => {
                    d.remove(i);
                }
            }
        }
        let _ = container::open(&d).map(|c| {
            for r in c.records() {
                let _ = c.get(&r.hash);
            }
        });
        let _ = container::salvage(&d);
    }
}

/// Review M3: unchanged records are copied from the cache, not recompressed.
#[test]
fn cached_records_are_copied_not_recompressed() {
    use gusset_store::container::RecordCache;
    let data: Vec<u8> = (0..200_000u32).flat_map(|i| (i % 977).to_le_bytes()).collect();
    let info = SaveInfo { save_seq: 1, created_unix: 0, app_version: "t" };
    // A file written at a high level, then opened: its records prime the cache.
    let mut w = ContainerWriter::new().with_level(19);
    let h = w.add(Kind::BLOB, &data).unwrap();
    let a = w.finish(&info).unwrap();
    let opened = container::open(&a).unwrap();
    let mut cache = RecordCache::new();
    cache.prime(&opened, |_| true);
    // Saving at level 1 with the cache reuses the level-19 bytes verbatim.
    let mut w = ContainerWriter::new().with_level(1);
    w.add_cached(Kind::BLOB, &data, Some(h), &mut cache).unwrap();
    let b = w.finish(&info).unwrap();
    let mut w = ContainerWriter::new().with_level(1);
    w.add(Kind::BLOB, &data).unwrap();
    let fresh = w.finish(&info).unwrap();
    assert_ne!(a, fresh, "levels must differ for this test to mean anything");
    assert_eq!(a, b, "the cached record was not copied byte-for-byte");
    container::verify(&b).unwrap();
    // A fresh record is verified and cached; a wrong caller hash is refused.
    let mut cache = RecordCache::new();
    let mut w = ContainerWriter::new();
    w.add_cached(Kind::FEATURE, b"abc", Some(gusset_store::Hash::of(b"abc")), &mut cache).unwrap();
    assert_eq!(cache.len(), 1);
    assert!(w.add_cached(Kind::FEATURE, b"xyz", Some(gusset_store::Hash::of(b"other")), &mut cache).is_err());
    cache.retain_used();
    assert_eq!(cache.len(), 1);
    cache.retain_used();
    assert!(cache.is_empty(), "unused records are dropped");
}

#[test]
fn framing_check_covers_every_byte() {
    let mut w = ContainerWriter::new();
    let h = w.add(Kind::FEATURE, &vec![7u8; 5000]).unwrap();
    w.add(Kind::BLOB, b"some blob bytes").unwrap();
    w.set_root("head", h.as_bytes()).unwrap();
    let good = w.finish(&SaveInfo { save_seq: 3, created_unix: 0, app_version: "t" }).unwrap();
    container::verify_framing(&good).unwrap();
    for i in 0..good.len() {
        let mut bad = good.clone();
        bad[i] ^= 0x20;
        assert!(container::verify_framing(&bad).is_err(), "flipped byte {i} not detected");
    }
}

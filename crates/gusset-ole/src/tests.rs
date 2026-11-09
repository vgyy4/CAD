#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::cast_possible_truncation)]

use super::*;
use crate::fixtures::{b_stream, data_with_blob, ole};

fn transmit(extra: usize) -> Vec<u8> {
    let mut v = b"**ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz****\n**PART1;\nAPPL=SolidEdge;\nFORMAT=binary;\n**PART2;\nSCH=SCH_3101255_31100;\n**PART3;\n**END_OF_HEADER*****\nPS\0\0\x003: TRANSMIT FILE".to_vec();
    v.extend((0..extra).map(|i| (i * 7 + 3) as u8));
    v
}

// --- Solid Edge ----------------------------------------------------------------------------

#[test]
fn solid_edge_transmit_is_found() {
    let x = transmit(9000);
    let f = ole(&[("SKETCH.P_B", b"**sketch"), ("STREAM0.X_B", &x), ("Versions", b"v")]);
    let e = solidedge::extract_xt(&f, &Limits::default()).unwrap();
    assert_eq!(e.xt, x);
    assert_eq!(e.stream, "STREAM0.X_B");
    assert!(e.warnings.is_empty());
    // Small enough for the mini stream.
    let x = transmit(20);
    let f = ole(&[("STREAM0.X_B", &x)]);
    assert_eq!(solidedge::extract_xt(&f, &Limits::default()).unwrap().xt, x);
}

#[test]
fn solid_edge_prefers_the_transmit_over_the_partition() {
    let (x, p) = (transmit(100), transmit(200));
    let f = ole(&[("STREAM0.P_B", &p), ("STREAM0.D_B", &transmit(50)), ("STREAM0.X_B", &x)]);
    assert_eq!(solidedge::extract_xt(&f, &Limits::default()).unwrap().xt, x);
    let f = ole(&[("STREAM0.P_B", &p), ("STREAM0.D_B", &transmit(50)), ("SKETCH.P_B", &transmit(5))]);
    let e = solidedge::extract_xt(&f, &Limits::default()).unwrap();
    assert_eq!((e.xt, e.stream.as_str()), (p, "STREAM0.P_B"));
}

#[test]
fn solid_edge_without_geometry_or_with_noise_is_an_error() {
    let f = ole(&[("JBlocks", b"x"), ("SKETCH.P_B", &transmit(5))]);
    assert!(matches!(solidedge::extract_xt(&f, &Limits::default()), Err(OleError::NoGeometry(_))));
    let f = ole(&[("STREAM0.X_B", b"not a transmit file at all")]);
    assert!(matches!(solidedge::extract_xt(&f, &Limits::default()), Err(OleError::Malformed(_))));
    assert_eq!(solidedge::extract_xt(b"hello", &Limits::default()), Err(OleError::NotOle));
    let big = ole(&[("STREAM0.X_B", &transmit(9000))]);
    let tiny = Limits { max_stream_bytes: 1000, ..Limits::default() };
    assert!(matches!(solidedge::extract_xt(&big, &tiny), Err(OleError::TooLarge(_))));
}

// --- Inventor ------------------------------------------------------------------------------

fn sab() -> Vec<u8> {
    let mut v = b"ASM BinaryFile4".to_vec();
    v.extend((0..500).map(|i| (i % 251) as u8));
    v
}

#[test]
fn inventor_result_blob_is_found_and_history_blobs_are_not() {
    let s = sab();
    let result = data_with_blob([0x1d, 3, 0, 0], &s);
    let history = data_with_blob([0x57, 3, 0, 0], b"ASM BinaryFile4history");
    let f = ole(&[
        ("Bhistory0000000000", &b_stream(&history)),
        ("Mhistory0000000000", b"meta"),
        ("Bresult00000000000", &b_stream(&result)),
        ("RSeSegInfo", b"x"),
    ]);
    let e = inventor::extract_sab(&f, &Limits::default()).unwrap();
    assert!(e.sab.starts_with(&s));
    assert_eq!(e.stream, "Bresult00000000000");
    assert!(e.warnings.is_empty());
    // Only history: no result.
    let f = ole(&[("Bhistory0000000000", &b_stream(&history))]);
    assert!(matches!(inventor::extract_sab(&f, &Limits::default()), Err(OleError::NoGeometry(_))));
}

#[test]
fn inventor_second_result_blob_warns_and_acis_magic_works() {
    let a = data_with_blob([0x1d, 3, 0, 0], &sab());
    let mut acis = b"ACIS BinaryFile8".to_vec();
    acis.extend([1, 2, 3]);
    let b = data_with_blob([0x1d, 3, 0, 0], &acis);
    let f = ole(&[("Ba00000000000000", &b_stream(&a)), ("Bb00000000000000", &b_stream(&b))]);
    let e = inventor::extract_sab(&f, &Limits::default()).unwrap();
    assert_eq!(e.stream, "Ba00000000000000");
    assert_eq!(e.warnings.len(), 1);
    let f = ole(&[("Bb00000000000000", &b_stream(&b))]);
    assert!(inventor::extract_sab(&f, &Limits::default()).unwrap().sab.starts_with(b"ACIS BinaryFile8"));
}

#[test]
fn inventor_limits_and_noise() {
    let big = vec![0u8; 3 << 20];
    let f = ole(&[("Bbomb000000000000", &b_stream(&big))]);
    // The ratio cap: a deflated zeros stream inflates ~1000x; a tight ratio refuses it.
    let ratio = Limits { max_ratio: 4, ..Limits::default() };
    assert!(matches!(inventor::extract_sab(&f, &ratio), Err(OleError::TooLarge(_))));
    let size = Limits { max_stream_bytes: 1 << 20, ..Limits::default() };
    assert!(matches!(inventor::extract_sab(&f, &size), Err(OleError::TooLarge(_))));
    let total = Limits { max_total_bytes: 1 << 20, ..Limits::default() };
    assert!(matches!(inventor::extract_sab(&f, &total), Err(OleError::TooLarge(_))));
    // A B stream that is not zlib is skipped.
    let f = ole(&[("Bjunk0000000000000", &[7u8; 100])]);
    assert!(matches!(inventor::extract_sab(&f, &Limits::default()), Err(OleError::NoGeometry(_))));
    assert_eq!(inventor::extract_sab(&[0u8; 600], &Limits::default()), Err(OleError::NotOle));
    // A blob at the very start of the data has no room for a tag.
    let f = ole(&[("Bedge000000000000", &b_stream(&sab()))]);
    assert!(matches!(inventor::extract_sab(&f, &Limits::default()), Err(OleError::NoGeometry(_))));
}

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> usize {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) as usize
    }
}

#[test]
fn truncation_and_mutation_never_panic() {
    let se = ole(&[("STREAM0.X_B", &transmit(6000)), ("SKETCH.P_B", &transmit(30))]);
    let inv = ole(&[("Bresult00000000000", &b_stream(&data_with_blob([0x1d, 3, 0, 0], &sab()))), ("M1", b"meta")]);
    let mut rng = Lcg(11);
    for base in [&se, &inv] {
        assert!(
            solidedge::extract_xt(base, &Limits::default()).is_ok()
                || inventor::extract_sab(base, &Limits::default()).is_ok()
        );
        for n in (0..base.len()).step_by(5) {
            let _ = solidedge::extract_xt(&base[..n], &Limits::default());
            let _ = inventor::extract_sab(&base[..n], &Limits::default());
        }
        for _ in 0..800 {
            let mut m = base.clone();
            for _ in 0..1 + rng.next() % 6 {
                let i = rng.next() % m.len();
                m[i] = rng.next() as u8;
            }
            let _ = solidedge::extract_xt(&m, &Limits::default());
            let _ = inventor::extract_sab(&m, &Limits::default());
        }
    }
    for len in [0usize, 1, 8, 511, 512, 4096] {
        let noise: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
        assert!(solidedge::extract_xt(&noise, &Limits::default()).is_err());
        assert!(inventor::extract_sab(&noise, &Limits::default()).is_err());
        let mut sig = gusset_sldprt::cfb::SIGNATURE.to_vec();
        sig.extend(&noise);
        assert!(solidedge::extract_xt(&sig, &Limits::default()).is_err());
        assert!(inventor::extract_sab(&sig, &Limits::default()).is_err());
    }
}

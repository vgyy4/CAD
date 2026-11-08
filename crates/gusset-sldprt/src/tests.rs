#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use super::*;
use miniz_oxide::deflate::{compress_to_vec, compress_to_vec_zlib};

fn xt(tag: &str, extra: usize) -> Vec<u8> {
    let mut v = format!("PS\0\0\0?: TRANSMIT FILE ({tag}) created by modeller version 1\0").into_bytes();
    let mut x = 88172645463325252u64;
    v.extend((0..extra).map(|_| {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        (x >> 24) as u8
    }));
    v
}

/// One block of a partition stream.
fn block(plain: &[u8]) -> Vec<u8> {
    let z = compress_to_vec_zlib(plain, 6);
    let mut b = Vec::new();
    b.extend(((24 + z.len() + 8) as u32).to_le_bytes());
    b.extend([0x23u8; 16]);
    b.extend((plain.len() as u32).to_le_bytes());
    b.extend((z.len() as u32).to_le_bytes());
    b.extend(&z);
    b.extend([0u8; 8]);
    b
}

fn partition_stream(extra: usize) -> Vec<u8> {
    let mut s = block(&xt("partition", extra));
    s.extend(block(&xt("deltas", 40)));
    s
}

// --- chunk container ---------------------------------------------------------------------

fn swap(name: &str) -> Vec<u8> {
    name.bytes().map(|b| b.rotate_left(4)).collect()
}

fn record(key: [u8; 4], name: &str, plain: &[u8], crc: Option<u32>) -> Vec<u8> {
    let z = compress_to_vec(plain, 6);
    let mut r = vec![0x14, 0, 6, 0, 8, 0];
    r.extend(key);
    r.extend(crc.unwrap_or_else(|| crc::crc32(plain)).to_le_bytes());
    r.extend((z.len() as u32).to_le_bytes());
    r.extend((plain.len() as u32).to_le_bytes());
    r.extend((name.len() as u32).to_le_bytes());
    r.extend(swap(name));
    r.extend(z);
    r
}

fn chunk_file(records: &[Vec<u8>]) -> Vec<u8> {
    let mut f = vec![0xde, 0xad, 0xbe, 0xef, 0, 0, 0, 4, 1, 2, 3, 4, 5];
    for r in records {
        f.extend(r);
        f.extend([9u8; 3]);
    }
    // The end record: signature, two zero disk numbers, the count twice, size, offset, a
    // zero comment length, then trailing bytes.
    let n = records.len() as u16;
    f.extend([0x5a, 0x5b, 0x5c, 0x5d, 0, 0, 0, 0]);
    f.extend(n.to_le_bytes());
    f.extend(n.to_le_bytes());
    f.extend([7u8, 1, 0, 0, 9, 2, 0, 0, 0, 0]);
    f.extend([0xa5u8; 20]);
    f
}

#[test]
fn a_cut_file_warns() {
    let mut f = chunk_file(&[
        record(KEY, "Contents/Config-0-Partition", &partition_stream(30), None),
        record(KEY, "Preview", &[7u8; 4000], None),
    ]);
    assert!(extract_xt(&f, &Limits::default()).unwrap().warnings.is_empty());
    f.truncate(f.len() - 40);
    let e = extract_xt(&f, &Limits::default()).unwrap();
    assert!(e.warnings.iter().any(|w| w.contains("ends early")), "{:?}", e.warnings);
    let o = ole(&[("Config-0-Partition", &partition_stream(30)), ("Preview", &[7u8; 9000])]);
    assert!(extract_xt(&o, &Limits::default()).unwrap().warnings.is_empty());
    let cut = &o[..o.len() - 4096];
    if let Ok(e) = extract_xt(cut, &Limits::default()) {
        assert!(e.warnings.iter().any(|w| w.contains("ends early")), "{:?}", e.warnings);
    }
}

const KEY: [u8; 4] = [0xdf, 0xff, 0xee, 0x7d];

#[test]
fn chunk_container_yields_the_partition() {
    let f = chunk_file(&[
        record(KEY, "Preview", b"png", None),
        record(KEY, "Contents/Config-0-GhostPartition", &partition_stream(10), None),
        record(KEY, "Contents/Config-0-Partition", &partition_stream(300), None),
    ]);
    assert_eq!(detect(&f), Some(Container::Chunks));
    let e = extract_xt(&f, &Limits::default()).unwrap();
    assert_eq!(e.container, Container::Chunks);
    assert_eq!(e.config, "Config-0");
    assert_eq!(e.xt, xt("partition", 300));
    assert!(e.warnings.is_empty());
}

#[test]
fn records_with_a_wrong_crc_are_skipped() {
    // A decoy with the real name and a bad CRC first, the real record after it.
    let f = chunk_file(&[
        record(KEY, "Contents/Config-0-Partition", &partition_stream(5), Some(1234)),
        record(KEY, "Contents/Config-0-Partition", &partition_stream(77), None),
    ]);
    assert_eq!(extract_xt(&f, &Limits::default()).unwrap().xt, xt("partition", 77));
    let only_decoy = chunk_file(&[record(KEY, "Contents/Config-0-Partition", &partition_stream(5), Some(1))]);
    assert!(matches!(extract_xt(&only_decoy, &Limits::default()), Err(SldprtError::NoGeometry(_))));
}

#[test]
fn the_lowest_configuration_wins_and_the_rest_warn() {
    let f = chunk_file(&[
        record(KEY, "Contents/Config-1-Partition", &partition_stream(11), None),
        record(KEY, "Contents/Config-0-Partition", &partition_stream(22), None),
    ]);
    let e = extract_xt(&f, &Limits::default()).unwrap();
    assert_eq!(e.config, "Config-0");
    assert_eq!(e.xt, xt("partition", 22));
    assert_eq!(e.warnings.len(), 1);
}

#[test]
fn a_part_without_partition_is_no_geometry() {
    let f = chunk_file(&[record(KEY, "Preview", b"x", None)]);
    assert!(matches!(extract_xt(&f, &Limits::default()), Err(SldprtError::NoGeometry(_))));
    // A partition stream holding only deltas.
    let f = chunk_file(&[record(KEY, "Contents/Config-0-Partition", &block(&xt("deltas", 3)), None)]);
    assert!(matches!(extract_xt(&f, &Limits::default()), Err(SldprtError::NoGeometry(_))));
}

#[test]
fn inflate_bombs_are_refused() {
    // 8 MiB of zeros deflate to about 8 KiB: ratio 1000, over a limit of 100.
    let zeros = vec![0u8; 8 << 20];
    let f = chunk_file(&[record(KEY, "Contents/Config-0-Partition", &zeros, None)]);
    let tight = Limits { max_ratio: 100, ..Limits::default() };
    assert!(matches!(extract_xt(&f, &tight), Err(SldprtError::NoGeometry(_))));
    let small = Limits { max_stream_bytes: 1 << 20, ..Limits::default() };
    assert!(matches!(extract_xt(&f, &small), Err(SldprtError::NoGeometry(_))));
    // A block whose declared size is far below its real inflated size is cut off.
    let mut s = block(&xt("partition", 5000));
    s[20..24].copy_from_slice(&10u32.to_le_bytes());
    let f = chunk_file(&[record(KEY, "Contents/Config-0-Partition", &s, None)]);
    assert!(extract_xt(&f, &Limits::default()).is_err());
    let total = Limits { max_total_bytes: 1000, ..Limits::default() };
    let f = chunk_file(&[record(KEY, "Contents/Config-0-Partition", &partition_stream(5000), None)]);
    assert!(matches!(extract_xt(&f, &total), Err(SldprtError::TooLarge(_))));
}

// --- OLE2 ----------------------------------------------------------------------------------

const FREE: u32 = 0xFFFF_FFFF;
const EOC: u32 = 0xFFFF_FFFE;
const FATSEC: u32 = 0xFFFF_FFFD;

/// A version 3 compound file (512-byte sectors, one FAT sector, so up to 127 sectors).
fn ole(streams: &[(&str, &[u8])]) -> Vec<u8> {
    let mut sectors: Vec<[u8; 512]> = Vec::new();
    let mut fat: Vec<u32> = Vec::new();
    let alloc = |data: &[u8], sectors: &mut Vec<[u8; 512]>, fat: &mut Vec<u32>| -> u32 {
        if data.is_empty() {
            return EOC;
        }
        let start = sectors.len() as u32;
        let n = data.len().div_ceil(512);
        for (i, c) in data.chunks(512).enumerate() {
            let mut s = [0u8; 512];
            s[..c.len()].copy_from_slice(c);
            sectors.push(s);
            fat.push(if i + 1 == n { EOC } else { start + i as u32 + 1 });
        }
        start
    };
    // (name, start, size)
    let mut entries: Vec<(String, u32, u32)> = Vec::new();
    let mut mini = Vec::new();
    let mut minifat: Vec<u32> = Vec::new();
    for (name, data) in streams {
        if data.len() < 4096 {
            let start = minifat.len() as u32;
            let n = data.len().div_ceil(64);
            for (i, c) in data.chunks(64).enumerate() {
                let mut s = [0u8; 64];
                s[..c.len()].copy_from_slice(c);
                mini.extend(s);
                minifat.push(if i + 1 == n { EOC } else { start + i as u32 + 1 });
            }
            entries.push(((*name).into(), if data.is_empty() { EOC } else { start }, data.len() as u32));
        } else {
            let start = alloc(data, &mut sectors, &mut fat);
            entries.push(((*name).into(), start, data.len() as u32));
        }
    }
    let mini_size = mini.len() as u32;
    let root_start = alloc(&mini, &mut sectors, &mut fat);
    let minifat_bytes: Vec<u8> = minifat.iter().flat_map(|v| v.to_le_bytes()).collect();
    let minifat_start = alloc(&minifat_bytes, &mut sectors, &mut fat);
    let mut dir = Vec::new();
    let mut entry = |name: &str, ty: u8, start: u32, size: u32| {
        let mut e = [0u8; 128];
        let u: Vec<u16> = name.encode_utf16().collect();
        for (i, c) in u.iter().enumerate() {
            e[2 * i..2 * i + 2].copy_from_slice(&c.to_le_bytes());
        }
        e[64..66].copy_from_slice(&((u.len() as u16 + 1) * 2).to_le_bytes());
        e[66] = ty;
        e[116..120].copy_from_slice(&start.to_le_bytes());
        e[120..124].copy_from_slice(&size.to_le_bytes());
        dir.extend(e);
    };
    entry("Root Entry", 5, root_start, mini_size);
    for (n, s, z) in &entries {
        entry(n, 2, *s, *z);
    }
    let dir_start = alloc(&dir, &mut sectors, &mut fat);
    let fat_sector = sectors.len() as u32;
    fat.push(FATSEC);
    assert!(fat.len() <= 128);
    fat.resize(128, FREE);
    let mut fs = [0u8; 512];
    for (i, v) in fat.iter().enumerate() {
        fs[4 * i..4 * i + 4].copy_from_slice(&v.to_le_bytes());
    }
    sectors.push(fs);
    let mut h = vec![0u8; 512];
    h[..8].copy_from_slice(&cfb::SIGNATURE);
    h[24..26].copy_from_slice(&0x3eu16.to_le_bytes());
    h[26..28].copy_from_slice(&3u16.to_le_bytes());
    h[28..30].copy_from_slice(&0xfffeu16.to_le_bytes());
    h[30..32].copy_from_slice(&9u16.to_le_bytes());
    h[32..34].copy_from_slice(&6u16.to_le_bytes());
    h[44..48].copy_from_slice(&1u32.to_le_bytes());
    h[48..52].copy_from_slice(&dir_start.to_le_bytes());
    h[56..60].copy_from_slice(&4096u32.to_le_bytes());
    h[60..64].copy_from_slice(&(if minifat.is_empty() { EOC } else { minifat_start }).to_le_bytes());
    h[68..72].copy_from_slice(&EOC.to_le_bytes());
    for i in 0..109 {
        let v = if i == 0 { fat_sector } else { FREE };
        h[76 + 4 * i..80 + 4 * i].copy_from_slice(&v.to_le_bytes());
    }
    for s in sectors {
        h.extend(s);
    }
    h
}

#[test]
fn ole2_regular_and_mini_streams() {
    let big = partition_stream(9000);
    assert!(big.len() > 4096, "{}", big.len());
    let f =
        ole(&[("Preview", b"abc"), ("Config-0-Partition", &big), ("Config-0-GhostPartition", &partition_stream(3))]);
    assert_eq!(detect(&f), Some(Container::Ole2));
    let e = extract_xt(&f, &Limits::default()).unwrap();
    assert_eq!(e.container, Container::Ole2);
    assert_eq!(e.xt, xt("partition", 9000));
    // The same partition small enough for the mini stream.
    let small = partition_stream(20);
    assert!(small.len() < 4096);
    let f = ole(&[("Config-0-Partition", &small)]);
    assert_eq!(extract_xt(&f, &Limits::default()).unwrap().xt, xt("partition", 20));
    // An assembly-like file: streams but no partition.
    let f = ole(&[("Preview", b"abc")]);
    assert!(matches!(extract_xt(&f, &Limits::default()), Err(SldprtError::NoGeometry(_))));
}

#[test]
fn ole2_fat_cycles_are_errors() {
    let big = partition_stream(9000);
    let mut f = ole(&[("Config-0-Partition", &big)]);
    // The FAT sector is the last one; make the first data sector point to itself.
    let fat_off = f.len() - 512;
    f[fat_off..fat_off + 4].copy_from_slice(&0u32.to_le_bytes());
    assert!(extract_xt(&f, &Limits::default()).is_err());
}

#[test]
fn ole2_oversized_streams_are_refused() {
    let big = partition_stream(9000);
    let f = ole(&[("Config-0-Partition", &big)]);
    let tiny = Limits { max_stream_bytes: 1000, ..Limits::default() };
    assert!(matches!(extract_xt(&f, &tiny), Err(SldprtError::TooLarge(_))));
}

// --- noise ---------------------------------------------------------------------------------

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> usize {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) as usize
    }
}

#[test]
fn truncation_and_mutation_never_panic() {
    let chunk = chunk_file(&[
        record(KEY, "Contents/Config-0-Partition", &partition_stream(300), None),
        record(KEY, "Preview", b"png", None),
    ]);
    let ole_f = ole(&[("Config-0-Partition", &partition_stream(9000)), ("Preview", b"abc")]);
    let mut rng = Lcg(7);
    for base in [&chunk, &ole_f] {
        assert!(extract_xt(base, &Limits::default()).is_ok());
        for n in (0..base.len()).step_by(7) {
            let _ = extract_xt(&base[..n], &Limits::default());
        }
        for _ in 0..600 {
            let mut m = base.clone();
            for _ in 0..1 + rng.next() % 6 {
                let i = rng.next() % m.len();
                m[i] = rng.next() as u8;
            }
            let _ = extract_xt(&m, &Limits::default());
        }
    }
    for len in [0usize, 1, 8, 511, 512, 4096] {
        let noise: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
        assert!(extract_xt(&noise, &Limits::default()).is_err());
        let mut sig = cfb::SIGNATURE.to_vec();
        sig.extend(&noise);
        let _ = extract_xt(&sig, &Limits::default());
    }
}

#[test]
fn unknown_files_are_not_sldprt() {
    assert_eq!(extract_xt(b"hello world, not a part", &Limits::default()), Err(SldprtError::NotSldprt));
    assert_eq!(detect(&[]), None);
}

#[test]
fn many_configurations_share_one_budget() {
    // Security review S1: streams are read lazily against one budget for the whole file.
    let junk = vec![0u8; 1024];
    let names: Vec<String> = (0..40).map(|i| format!("Config-{i}-Partition")).collect();
    let streams: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), junk.as_slice())).collect();
    let file = ole(&streams);
    let limits = Limits { max_total_bytes: 16 << 10, ..Limits::default() };
    assert!(matches!(extract_xt(&file, &limits), Err(SldprtError::TooLarge(_))));
    // A partition in the first configuration is found without reading the others.
    let good = partition_stream(0);
    let mut streams2: Vec<(&str, &[u8])> = vec![("Config-0-Partition", good.as_slice())];
    streams2.extend(streams.iter().skip(1).copied());
    let r = extract_xt(&ole(&streams2), &limits).unwrap();
    assert_eq!(r.config, "Config-0");
}

#[test]
fn an_imported_feature_keeps_its_bodies_in_local_bodies() {
    // A count, then per body a name record (UTF-16) and a block.
    let mut local = 3u32.to_le_bytes().to_vec();
    for i in 0..3 {
        local.extend([0xff, 0xfe, 0xff, 4]);
        local.extend("Imp1".encode_utf16().flat_map(u16::to_le_bytes));
        local.extend([0, 0, 0, 0, 0, 0x13, 0x39, 0, 0, 0, 0x0e, 0x39, 0, 0]);
        local.extend(block(&xt(&format!("body {i}"), 50 + i)));
    }
    let f = chunk_file(&[
        record(KEY, "Contents/Config-0-Partition", &[0u8; 8], None),
        record(KEY, "Config-0-FeatureBodies/LocalBodies", &local, None),
    ]);
    let bodies = local_bodies(&f, &Limits::default()).unwrap();
    assert_eq!(bodies, (0..3).map(|i| xt(&format!("body {i}"), 50 + i)).collect::<Vec<_>>());
    // None in an ordinary part; damaged streams never panic.
    let plain = chunk_file(&[record(KEY, "Contents/Config-0-Partition", &partition_stream(5), None)]);
    assert!(local_bodies(&plain, &Limits::default()).unwrap().is_empty());
    for cut in [5, 40, 90, local.len() - 3] {
        let f = chunk_file(&[record(KEY, "Config-0-FeatureBodies/LocalBodies", &local[..cut], None)]);
        let _ = local_bodies(&f, &Limits::default());
    }
}

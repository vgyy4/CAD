#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

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
    let bodies = local_bodies(&f, &Limits::default(), &Default::default()).unwrap();
    assert_eq!(bodies, (0..3).map(|i| xt(&format!("body {i}"), 50 + i)).collect::<Vec<_>>());
    // None in an ordinary part; damaged streams never panic.
    let plain = chunk_file(&[record(KEY, "Contents/Config-0-Partition", &partition_stream(5), None)]);
    assert!(local_bodies(&plain, &Limits::default(), &Default::default()).unwrap().is_empty());
    for cut in [5, 40, 90, local.len() - 3] {
        let f = chunk_file(&[record(KEY, "Config-0-FeatureBodies/LocalBodies", &local[..cut], None)]);
        let _ = local_bodies(&f, &Limits::default(), &Default::default());
    }
}

// --- assemblies --------------------------------------------------------------------------

const TREE: &str = "swXmlContents/COMPINSTANCETREE";

fn tree_xml(body: &str) -> String {
    format!(
        r#"<?xml version="1.0"?><swSolidWorks><swHeader>
<swFile id="3" swDocType="ASSEMBLY" swPath="C:\a\Top &amp; Co.SLDASM"/>
<swFile id="6" swDocType="PART" swPath="C:\a\Bracket one.SLDPRT"/>
<swFile id="9" swDocType="ASSEMBLY" swPath="D:/x/Sub.SLDASM"/>
</swHeader><swModelList>
<swModel id="5" swName="Bracket one" swConfigurationName="Default" swFileRef="6" swBoundingBox="0 0 0 0.01 0.02 0.03"/>
<swModel id="8" swName="Sub" swConfigurationName="Default" swFileRef="9"/>
<swModel id="2" swName="Top" swConfigurationName="Default" swFileRef="3">{body}</swModel>
</swModelList></swSolidWorks>"#
    )
}

fn asm_file(xml: &str) -> Vec<u8> {
    chunk_file(&[record(KEY, "Preview", b"png", None), record(KEY, TREE, xml.as_bytes(), None)])
}

const IDENT: &str = "1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1";

#[test]
fn an_assembly_lists_its_components_with_placements() {
    let body = format!(
        r#"<swReference id="4" swName="Bracket one" swReferenceNumber="1" swIsVirtualComponent="NO" swSuppressed="NO" swHidden="NO" swModelRef="5" swTransform="{IDENT}"/>
<swReference id="7" swName="Bracket one" swReferenceNumber="2" swIsVirtualComponent="NO" swSuppressed="YES" swHidden="YES" swModelRef="5" swTransform="0 1 0 0 -1 0 0 0 0 0 1 0 0.5 0.25 -2 1"/>
<swReference id="10" swName="Sub" swReferenceNumber="1" swIsVirtualComponent="NO" swSuppressed="NO" swHidden="NO" swModelRef="8" swTransform="{IDENT}"/>
<swReference id="11" swName="Part1^Top" swReferenceNumber="1" swIsVirtualComponent="YES" swModelRef="5" swTransform="1 2"/>"#
    );
    let a = read_assembly(&asm_file(&tree_xml(&body)), &Limits::default()).unwrap();
    assert_eq!(a.components.len(), 4);
    let c = &a.components;
    assert_eq!((c[0].name.as_str(), c[0].file.as_str()), ("Bracket one-1", "Bracket one.SLDPRT"));
    assert_eq!(c[0].kind, ComponentKind::Part);
    assert_eq!(c[0].bbox, Some([0.0, 0.0, 0.0, 0.01, 0.02, 0.03]));
    assert_eq!(c[1].name, "Bracket one-2");
    assert!(c[1].suppressed && c[1].hidden && !c[0].suppressed);
    // Row-vector layout in, column vectors out: translation in the last column.
    assert_eq!(c[1].placement[0], [0.0, -1.0, 0.0, 0.5]);
    assert_eq!(c[1].placement[1], [1.0, 0.0, 0.0, 0.25]);
    assert_eq!(c[1].placement[2], [0.0, 0.0, 1.0, -2.0]);
    assert_eq!(c[1].placement[3], [0.0, 0.0, 0.0, 1.0]);
    assert_eq!((c[2].kind, c[2].file.as_str()), (ComponentKind::Assembly, "Sub.SLDASM"));
    assert_eq!(c[3].kind, ComponentKind::Virtual);
    assert!(a.warnings.iter().any(|w| w.contains("Part1^Top-1") && w.contains("placement")), "{:?}", a.warnings);
}

#[test]
fn assembly_input_that_is_not_an_assembly_is_refused() {
    let part = chunk_file(&[record(KEY, "Contents/Config-0-Partition", &partition_stream(30), None)]);
    assert!(matches!(read_assembly(&part, &Limits::default()), Err(SldprtError::Malformed(_))));
    assert!(matches!(read_assembly(b"hello", &Limits::default()), Err(SldprtError::NotSldprt)));
    let o = ole(&[("Config-0-Partition", &partition_stream(30))]);
    assert!(read_assembly(&o, &Limits::default()).is_err());
    let none = asm_file(&tree_xml(""));
    assert!(matches!(read_assembly(&none, &Limits::default()), Err(SldprtError::Malformed(_))));
}

#[test]
fn hostile_assembly_trees_stay_bounded() {
    let l = Limits::default();
    // Truncations and noise never panic.
    let body = format!(r#"<swReference swName="A" swReferenceNumber="1" swModelRef="5" swTransform="{IDENT}"/>"#);
    let xml = tree_xml(&body);
    for cut in (0..xml.len()).step_by(7) {
        let _ = read_assembly(&asm_file(xml.get(..cut).unwrap_or_default()), &l);
    }
    for junk in ["<", "<a", "<a b", "<a b=", "<a b=\"", "<!--", "<?x", "&#xFFFFFFFF;<swFile id=\"&#;\" ", "\u{0}\u{1}"]
    {
        let _ = read_assembly(&asm_file(junk), &l);
    }
    // Numbers that are not finite or not 16 of them leave the component at the origin.
    for t in
        ["nan 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1", "1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1 1", "inf 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1"]
    {
        let b = format!(r#"<swReference swName="A" swReferenceNumber="1" swModelRef="5" swTransform="{t}"/>"#);
        let a = read_assembly(&asm_file(&tree_xml(&b)), &l).unwrap();
        assert_eq!(a.components[0].placement[0], [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(a.warnings.len(), 1);
    }
    // A path that is only a separator names no file.
    let x = tree_xml(&body).replace(r"C:\a\Bracket one.SLDPRT", r"C:\a\..");
    let a = read_assembly(&asm_file(&x), &l).unwrap();
    assert!(a.components.is_empty() && a.warnings.len() == 1);
    // Huge component counts are capped.
    let many = body.repeat(MAX_COMPONENTS + 50);
    let a = read_assembly(&asm_file(&tree_xml(&many)), &l).unwrap();
    assert_eq!(a.components.len(), MAX_COMPONENTS);
    assert!(a.warnings.iter().any(|w| w.contains("first")));
    // Deeply nested or endless tags: bounded by the stream limit and the tag budget.
    let deep = "<a>".repeat(100_000);
    let _ = read_assembly(&asm_file(&deep), &l);
    // An inflate bomb larger than the stream limit is refused, not allocated.
    let tight = Limits { max_stream_bytes: 1000, ..Limits::default() };
    assert!(read_assembly(&asm_file(&tree_xml(&many)), &tight).is_err());
}

/// The NIST single-die assembly, when present (the sample is not in the repository).
#[test]
fn the_nist_assembly_sample() {
    let Ok(dir) = std::env::var("GUSSET_SLDASM_SAMPLE_DIR") else { return };
    let path = std::path::Path::new(&dir).join("NIST_Nanocalorimeter_single_die_3D_model.SLDASM");
    let Ok(bytes) = std::fs::read(path) else { return };
    let a = read_assembly(&bytes, &Limits::default()).unwrap();
    let real: Vec<_> = a.components.iter().filter(|c| c.kind != ComponentKind::Virtual).collect();
    assert_eq!(a.components.len(), 5);
    assert_eq!(real.len(), 3);
    assert!(real.iter().all(|c| c.file.ends_with(".SLDPRT") && !c.suppressed && !c.hidden));
    let die = real.iter().find(|c| c.name.contains("die layer")).unwrap();
    assert!((die.placement[0][3] + 0.068_197_763_517_917_76).abs() < 1e-15);
}

#[test]
fn virtual_components_keep_a_part_inside_the_assembly() {
    let inner = chunk_file(&[record(KEY, "Contents/Config-0-Partition", &partition_stream(60), None)]);
    let body = format!(
        r#"<swReference swName="Part1^Top" swReferenceNumber="1" swIsVirtualComponent="YES" swModelRef="5" swTransform="{IDENT}"/>
<swReference swName="Part9^Top" swReferenceNumber="1" swIsVirtualComponent="YES" swModelRef="5" swTransform="{IDENT}"/>
<swReference swName="^x" swReferenceNumber="1" swIsVirtualComponent="YES" swModelRef="5" swTransform="{IDENT}"/>
<swReference swName="Bracket one" swReferenceNumber="1" swModelRef="5" swTransform="{IDENT}"/>"#
    );
    let xml = tree_xml(&body);
    let f = chunk_file(&[record(KEY, TREE, xml.as_bytes(), None), record(KEY, "VirtualComp/Part1", &inner, None)]);
    let a = read_assembly(&f, &Limits::default()).unwrap();
    let names: Vec<_> = a.components.iter().map(|c| c.embedded.as_deref()).collect();
    assert_eq!(names, [Some("VirtualComp/Part1"), Some("VirtualComp/Part9"), None, None]);
    // The embedded stream is a part the ordinary reader reads.
    let got = embedded_part(&f, &Limits::default(), "VirtualComp/Part1").unwrap().unwrap();
    assert_eq!(got, inner);
    assert_eq!(extract_xt(&got, &Limits::default()).unwrap().xt, xt("partition", 60));
    // A stream that is not there, one outside the prefix, and the wrong containers.
    assert_eq!(embedded_part(&f, &Limits::default(), "VirtualComp/Part9").unwrap(), None);
    assert_eq!(embedded_part(&f, &Limits::default(), "Preview").unwrap(), None);
    assert!(embedded_part(b"junk", &Limits::default(), "VirtualComp/Part1").is_err());
    let o = ole(&[("Config-0-Partition", &partition_stream(30))]);
    assert_eq!(embedded_part(&o, &Limits::default(), "VirtualComp/Part1").unwrap(), None);
    // A stream over the limit is refused; a garbage or cut inner file is an error of its own.
    let tight = Limits { max_stream_bytes: 100, ..Limits::default() };
    assert_eq!(embedded_part(&f, &tight, "VirtualComp/Part1").unwrap(), None);
    let junk =
        chunk_file(&[record(KEY, TREE, xml.as_bytes(), None), record(KEY, "VirtualComp/Part1", &[0xa5; 300], None)]);
    let g = embedded_part(&junk, &Limits::default(), "VirtualComp/Part1").unwrap().unwrap();
    assert!(extract_xt(&g, &Limits::default()).is_err());
    for cut in [10, 40, inner.len() / 2, inner.len() - 5] {
        let part = chunk_file(&[record(KEY, "VirtualComp/Part1", inner.get(..cut).unwrap_or_default(), None)]);
        if let Ok(Some(b)) = embedded_part(&part, &Limits::default(), "VirtualComp/Part1") {
            let _ = extract_xt(&b, &Limits::default());
        }
    }
}

// --- feature history -----------------------------------------------------------------------

/// A class declaration: `FF FF 01 00 <len> 00 <name>`.
fn class_decl(name: &str) -> Vec<u8> {
    let mut v = vec![0xff, 0xff, 1, 0, name.len() as u8, 0];
    v.extend(name.as_bytes());
    v
}

/// A node-name object (tag 0x8045) with a UTF-16 name.
fn node_name(name: &str) -> Vec<u8> {
    let mut v = vec![0x45, 0x80, 0xff, 0xfe, 0xff, name.encode_utf16().count() as u8];
    v.extend(name.encode_utf16().flat_map(u16::to_le_bytes));
    v
}

/// A feature node of class `cls`: its declaration, name, flags, id and the 46-byte block.
fn feature_node(cls: &str, name: &str, id: u32) -> Vec<u8> {
    let mut v = class_decl(cls);
    v.extend(node_name(name));
    v.extend(0u32.to_le_bytes());
    v.extend(0x4000_0000u32.to_le_bytes());
    v.extend(id.to_le_bytes());
    v.extend([0u8; 20]);
    v.extend([0, 0xff, 0xff, 0, 0, 1, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0, 0, 0x80, 0xbf]);
    v.extend([0u8; 38]);
    v
}

/// A sketch item: `u16 0, u32 key, u32 id`, the entity header and its fixed fields.
fn sketch_item(key: u32, id: u32, kind: u16) -> Vec<u8> {
    let mut v = vec![0, 0];
    v.extend(key.to_le_bytes());
    v.extend(id.to_le_bytes());
    v.extend([0xff, 0xff, 0x1f, 0, 1, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0, 0, 0x80, 0xbf]);
    v.extend([0u8; 12]);
    v.extend(kind.to_le_bytes());
    v.extend((-1.0f32).to_le_bytes());
    v.extend([0, 0, 4, 0, 0]); // no reference object, flags, a byte
    v.extend(0.0f64.to_le_bytes());
    v.extend(1.0f64.to_le_bytes());
    v
}

/// `Config-0`: declares moUnitComponent_c as class 1.
fn config0() -> Vec<u8> {
    let mut v = vec![1, 0];
    v.extend(class_decl("moUnitComponent_c"));
    v.extend([0u8; 8]);
    v
}

/// A 10 x 10 mm square sketch on the Front plane, extruded 5 mm (and `extra` nodes after).
fn feature_stream(extra: &[u8]) -> Vec<u8> {
    let mut rf = vec![3, 0];
    rf.extend(feature_node("moProfileFeature_c", "Sketch1", 5));
    // The plane: a reference-plane component of plane feature 2 (Front), identity, t = 0.
    rf.extend([0x01, 0x80, 2, 0]);
    rf.extend([0u8; 10]);
    rf.extend([0u8; 31]);
    rf.extend(2u32.to_le_bytes());
    rf.extend(9u32.to_le_bytes());
    rf.extend([0, 0, 0, 0, 0, 0, 0]);
    rf.extend([0u8; 24]);
    rf.extend(1.0f64.to_le_bytes());
    rf.extend([0u8; 16]);
    let corners = [(0.0, 0.0), (0.01, 0.0), (0.01, 0.01), (0.0, 0.01)];
    rf.extend(4u32.to_le_bytes());
    for (i, (x, y)) in corners.iter().enumerate() {
        rf.extend(sketch_item(100 + i as u32, 10 + i as u32, 0));
        rf.extend(0x1eu16.to_le_bytes());
        rf.extend(f64::to_le_bytes(*x));
        rf.extend(f64::to_le_bytes(*y));
        rf.extend([0u8; 12]);
    }
    rf.extend(4u32.to_le_bytes());
    for i in 0..4u16 {
        rf.extend(sketch_item(1 + u32::from(i), 20 + u32::from(i), 1));
        rf.extend(i.to_le_bytes());
        rf.extend(((i + 1) % 4).to_le_bytes());
        rf.extend([1, 0, 0, 0]);
        rf.extend((-1.0f64).to_le_bytes());
        rf.extend([0u8; 24]);
    }
    rf.extend([0u8; 16]);
    rf.extend(feature_node("moExtrusion_c", "Boss-Extrude1", 7));
    // Blind, then the depth parameter D1 = 5 mm.
    rf.extend(class_decl("moEndSpec_c"));
    rf.extend([0, 0, 1, 0, 0, 0]);
    rf.extend([0u8; 10]);
    rf.extend([0u8; 8]);
    rf.extend([0u8; 8]);
    rf.extend(class_decl("moLengthParameter_c"));
    rf.extend(node_name("D1"));
    rf.extend([0, 0, 0, 0, 0, 0, 0, 0x40, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0, 0, 0]);
    rf.extend(0.005f64.to_le_bytes());
    rf.extend([0u8; 16]);
    rf.extend(extra);
    rf
}

fn history(rf: &[u8]) -> Result<Option<serde_json::Value>, String> {
    let f = ole(&[("Config-0", &config0()), ("Config-0-ResolvedFeatures", rf), ("Preview", b"png")]);
    features::feature_tree(&f, "dir/part.SLDPRT", &Limits::default())
}

#[test]
fn a_sketch_and_an_extrude_become_a_tree() {
    let t = history(&feature_stream(&[])).unwrap().unwrap();
    assert_eq!(t["format"], "gusset-feature-tree");
    assert_eq!(t["version"], 2);
    assert_eq!(t["source"]["file"], "part.SLDPRT");
    let f = t["features"].as_array().unwrap();
    assert_eq!(f.len(), 2, "{t}");
    let sk = &f[0];
    assert_eq!(sk["type"], "sketch");
    assert_eq!(sk["name"], "Sketch1");
    assert_eq!(sk["plane"]["ref"], "Front");
    assert_eq!(sk["plane"]["x_axis"], serde_json::json!([1.0, 0.0, 0.0]));
    let pts = sk["points"].as_array().unwrap();
    assert_eq!(pts.len(), 4);
    assert_eq!((pts[2]["x"].as_f64(), pts[2]["y"].as_f64()), (Some(10.0), Some(10.0)));
    let curves = sk["curves"].as_array().unwrap();
    assert_eq!(curves.len(), 4);
    assert_eq!(curves[3]["type"], "line");
    assert_eq!((curves[3]["start"].as_str(), curves[3]["end"].as_str()), (Some("p3"), Some("p0")));
    let ex = &f[1];
    assert_eq!(ex["type"], "extrude");
    assert_eq!(ex["sketch"], "Sketch1");
    assert_eq!(ex["operation"], "new");
    assert_eq!(ex["end_condition"], "blind");
    assert_eq!(ex["distance"].as_f64(), Some(5.0));
    assert_eq!(ex["symmetric"], false);
    // The same streams in the chunk container.
    let rf = feature_stream(&[]);
    let c = chunk_file(&[
        record(KEY, "Contents/Config-0", &config0(), None),
        record(KEY, "Contents/Config-0-ResolvedFeatures", &rf, None),
    ]);
    assert_eq!(
        features::feature_tree(&c, "part.sldprt", &Limits::default()).unwrap().unwrap()["features"],
        t["features"]
    );
}

#[test]
fn a_mid_plane_cut_and_an_unknown_feature() {
    // A second sketch and a "Cut-" extrude: a cut against the first solid.
    let mut extra = feature_node("moProfileFeature_c", "Sketch2", 8);
    let base = feature_stream(&[]);
    let sk = base.windows(4).position(|w| w == [0x01, 0x80, 2, 0]).unwrap();
    let ex = base.windows(13).position(|w| w == b"moExtrusion_c").unwrap() - 6;
    extra.extend(&base[sk..ex]);
    extra.extend(feature_node("moICE_c", "Cut-Extrude1", 9));
    extra.extend(class_decl("moEndSpec_c"));
    extra.extend([0, 0, 1, 0, 0, 0]);
    extra.extend([0u8; 10]);
    extra.extend(6u32.to_le_bytes());
    extra.extend([0u8; 12]);
    extra.extend(class_decl("moLengthParameter_c"));
    extra.extend(node_name("D1"));
    extra.extend([0, 0, 0, 0, 0, 0, 0, 0x40, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0, 0, 0]);
    extra.extend(0.002f64.to_le_bytes());
    extra.extend([0u8; 16]);
    let t = history(&feature_stream(&extra)).unwrap().unwrap();
    let cut = &t["features"][3];
    assert_eq!(cut["operation"], "cut", "{t}");
    assert_eq!(cut["target"], "Boss-Extrude1");
    assert_eq!(cut["sketch"], "Sketch2");
    assert_eq!(cut["end_condition"], "mid_plane");
    assert_eq!((cut["distance"].as_f64(), cut["symmetric"].as_bool()), (Some(2.0), Some(true)));
    // A feature class the reader does not know refuses the whole history, naming it.
    let mut hole = extra.clone();
    hole.extend(feature_node("moHoleWzd_c", "Tapped Hole1", 11));
    let why = history(&feature_stream(&hole)).unwrap_err();
    assert!(why.contains("Tapped Hole1") && why.contains("moHoleWzd_c"), "{why}");
}

#[test]
fn parts_without_a_feature_stream_have_no_history() {
    let f = ole(&[("Config-0-Partition", &partition_stream(30))]);
    assert_eq!(features::feature_tree(&f, "p.sldprt", &Limits::default()), Ok(None));
    assert!(features::feature_tree(b"not a part at all", "p.sldprt", &Limits::default()).is_err());
    // A stream without node names, or with no solid feature, is a reason.
    assert!(history(&[0u8; 300]).is_err());
    let rf = feature_stream(&[]);
    let only_sketch = &rf[..rf.windows(13).position(|w| w == b"moExtrusion_c").unwrap() - 6];
    assert!(history(only_sketch).unwrap_err().contains("no extrude"));
}

#[test]
fn damaged_feature_streams_never_panic() {
    let rf = feature_stream(&feature_node("moHoleWzd_c", "Hole1", 11));
    let c0 = config0();
    let mut rng = Lcg(11);
    for n in 0..rf.len() {
        let _ = features::convert(&rf[..n], &c0, "p");
    }
    for _ in 0..1500 {
        let mut m = rf.clone();
        for _ in 0..1 + rng.next() % 8 {
            let i = rng.next() % m.len();
            m[i] = rng.next() as u8;
        }
        let _ = features::convert(&m, &c0, "p");
        let _ = features::convert(&m, &m, "p");
    }
    // Whole files, cut anywhere.
    let f = ole(&[("Config-0", &c0), ("Config-0-ResolvedFeatures", &rf)]);
    for n in (0..f.len()).step_by(13) {
        let _ = features::feature_tree(&f[..n], "p", &Limits::default());
    }
    for len in [0usize, 7, 64, 4096] {
        let noise: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
        assert!(features::convert(&noise, &noise, "p").is_err());
    }
}

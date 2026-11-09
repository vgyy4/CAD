//! Synthetic Solid Edge and Inventor containers, for tests (here and in `gusset-commands`).
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::cast_possible_truncation)]

use miniz_oxide::deflate::compress_to_vec_zlib;

const FREE: u32 = 0xFFFF_FFFF;
const EOC: u32 = 0xFFFF_FFFE;
const FATSEC: u32 = 0xFFFF_FFFD;

/// A version 3 compound file with the given streams in its root storage.
pub fn ole(streams: &[(&str, &[u8])]) -> Vec<u8> {
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
    h[..8].copy_from_slice(&gusset_sldprt::cfb::SIGNATURE);
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

const GUID: [u8; 16] = [0x9e, 0xc2, 0x2b, 0xa4, 0xd4, 0x11, 0xe8, 0x01, 0x60, 0x00, 0x2d, 0xb3, 0xee, 0x29, 0xfb, 0xb0];

/// An Inventor `B` stream: the 18-byte header, then the zlib stream of `plain`.
pub fn b_stream(plain: &[u8]) -> Vec<u8> {
    let mut v = GUID.to_vec();
    v.extend([4, 1]);
    v.extend(compress_to_vec_zlib(plain, 6));
    v
}

/// Inflated `B` stream data: filler, a blob preceded by `tag` and a count, filler.
pub fn data_with_blob(tag: [u8; 4], blob: &[u8]) -> Vec<u8> {
    let mut d = vec![0u8, 1, 0, 0, 0, 0, 0, 0, 0xce, 0, 0, 0, 0];
    d.extend([2, 0]);
    d.extend(tag);
    d.extend(42u32.to_le_bytes());
    d.extend(blob);
    d.extend([0xffu8; 40]);
    d
}

/// An Inventor part holding `sab` as its result body.
pub fn inventor_part(sab: &[u8]) -> Vec<u8> {
    ole(&[("Bresult00000000000", &b_stream(&data_with_blob([0x1d, 3, 0, 0], sab))), ("Mresult00000000000", b"meta")])
}

/// A Solid Edge part holding `xt` (a transmit with its header) as `STREAM0.X_B`.
pub fn solid_edge_part(xt: &[u8]) -> Vec<u8> {
    ole(&[("SKETCH.P_B", b"**sketch"), ("STREAM0.X_B", xt)])
}

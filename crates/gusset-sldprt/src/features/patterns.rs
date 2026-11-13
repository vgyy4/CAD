//! Feature patterns and mirrors of features (`moLPattern_c`, `moCirPattern_c`,
//! `moMirrorPattern_c`): the seed features they repeat and the lines they repeat them along.
//!
//! The feature stream is one archive of objects (MFC `CArchive` style, layout notes §2): a
//! reference to an object written earlier is its archive index, a `u16` (or `ff 7f` and a `u32`
//! from 0x7fff on). A pattern names its seed features that way, so the index of each feature
//! node has to be known. The stream does not write it, but two kinds of back references do:
//! a feature's own dimension objects point at it (`P 01 00 01 00 00 00 P`, inside its
//! span), and face references to a feature's own faces carry it as their owner word (the word
//! after the reference's class tag, zero in references made by other features).

use std::collections::HashMap;

use super::{V3, dot, f64_at, f64s_at, mm, round_to, u16_at, u32_at};

/// Seed lists longer than this are not read.
const MAX_SEEDS: usize = 64;

/// The three lists after a pattern's header: the seed features, an (always empty) second list,
/// `01 00 00 00 00 00 01 00 00 00 00`, and the bodies the pattern repeats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Seeds {
    /// Archive indices of the seed features, in the written order.
    pub ptrs: Vec<u32>,
    /// How many bodies the pattern repeats (a pattern of bodies writes no seed feature).
    pub bodies: usize,
}

/// The first seed list in `[a, b)`: `T n <n pointers> T 00 00 01 00 00 00 00 00 01 00 00 00 00
/// T m`, where `T` is the list's class tag (it differs between files: `08 80`, `58 80`,
/// `a3 81`) and `m` the body count. Spacer_long_base_square's LPattern1:
/// `08 80 02 00 84 04 33 06 08 80 00 00 ...` (Boss-Extrude3 and Cut-Extrude1).
pub(super) fn seed_list(d: &[u8], a: usize, b: usize) -> Option<Seeds> {
    const MID: [u8; 11] = [1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0];
    let end = b.min(d.len());
    let mut i = a;
    while i + 4 <= end {
        let Some(t) = u16_at(d, i).filter(|t| t & 0x8000 != 0 && *t != 0xffff) else {
            i += 1;
            continue;
        };
        if let Some(s) = list_at(d, i, t, end, &MID) {
            return Some(s);
        }
        i += 1;
    }
    None
}

fn list_at(d: &[u8], i: usize, t: u16, end: usize, mid: &[u8; 11]) -> Option<Seeds> {
    let n = usize::from(u16_at(d, i + 2)?);
    if n > MAX_SEEDS {
        return None;
    }
    let mut j = i + 4;
    let mut ptrs = Vec::with_capacity(n);
    for _ in 0..n {
        let mut p = u32::from(u16_at(d, j)?);
        j += 2;
        if p == 0x7fff {
            p = u32_at(d, j)?;
            j += 4;
        }
        if p == 0 {
            return None;
        }
        ptrs.push(p);
    }
    let tail_ok = u16_at(d, j) == Some(t)
        && d.get(j + 2..j + 4) == Some(&[0, 0][..])
        && d.get(j + 4..j + 15) == Some(&mid[..])
        && u16_at(d, j + 15) == Some(t);
    if !tail_ok || j + 19 > end {
        return None;
    }
    Some(Seeds { ptrs, bodies: usize::from(u16_at(d, j + 17)?) })
}

/// The archive index of each feature node, from the back references described above.
/// `spans` are the nodes' `(start, end, id)`; returns index -> position in `spans`. An index
/// two nodes claim is left out.
pub(super) fn object_indices(d: &[u8], spans: &[(usize, usize, u32)]) -> HashMap<u32, usize> {
    let mut found: HashMap<u32, Option<usize>> = HashMap::new();
    let mut claim = |p: u32, k: usize| {
        found
            .entry(p)
            .and_modify(|v| {
                if *v != Some(k) {
                    *v = None
                }
            })
            .or_insert(Some(k));
    };
    // A feature's dimension objects: `P 01 00 01 00 00 00 P` inside its span.
    for (k, &(a, b, _)) in spans.iter().enumerate() {
        let end = b.min(d.len());
        let mut i = a;
        while i + 10 <= end {
            if d.get(i + 2..i + 8) == Some(&[1, 0, 1, 0, 0, 0][..])
                && let (Some(p), Some(q)) = (u16_at(d, i), u16_at(d, i + 8))
                && p == q
                && p > 1
                && p < 0x7fff
            {
                claim(u32::from(p), k);
            }
            i += 1;
        }
    }
    // Face references to a feature's own faces: `<class tag | end of a class name> P <class tag>
    // u16 <u32 feature id> <u32 stamp>`.
    let by_id: HashMap<u32, usize> = spans.iter().enumerate().filter(|(_, s)| s.2 > 5).map(|(k, s)| (s.2, k)).collect();
    let mut i = 2;
    while i + 14 <= d.len() {
        let owner_ok = u16_at(d, i).is_some_and(|p| p > 1 && p < 0x7fff)
            && u16_at(d, i + 2).is_some_and(|t| t & 0x8000 != 0 && t != 0xffff)
            && (u16_at(d, i - 2).is_some_and(|t| t & 0x8000 != 0 && t != 0xffff)
                || d.get(i - 2..i) == Some(&b"_c"[..]));
        if owner_ok
            && let (Some(p), Some(fid), Some(stamp)) = (u16_at(d, i), u32_at(d, i + 6), u32_at(d, i + 10))
            && stamp != 0
            && let Some(&k) = by_id.get(&fid)
        {
            claim(u32::from(p), k);
        }
        i += 1;
    }
    found.into_iter().filter_map(|(p, k)| k.map(|k| (p, k))).collect()
}

/// The lines a pattern repeats along (line references: a reference axis, an edge or a sketch
/// line): after the marker word `c7 cf ff ff`, `00 00 00 00`, a `u32` and 4 to 20 zero bytes,
/// `f64 length`, the start point and the unit direction in model coordinates (m). Returns the
/// start in mm and the direction, in the written order.
pub(super) fn pattern_lines(d: &[u8], a: usize, b: usize) -> Vec<(V3, V3)> {
    const MARK: [u8; 8] = [0xc7, 0xcf, 0xff, 0xff, 0, 0, 0, 0];
    let mut out = Vec::new();
    let end = b.min(d.len());
    let mut from = a;
    while let Some(i) = super::find(d, &MARK, from, end) {
        from = i + 1;
        for at in (i + 16..=i + 36).step_by(4) {
            let (Some(len), Some(o), Some(dir)) = (f64_at(d, at), f64s_at::<3>(d, at + 8), f64s_at::<3>(d, at + 32))
            else {
                continue;
            };
            if len.is_finite()
                && len > 0.0
                && len < 100.0
                && o.iter().all(|c| c.is_finite() && c.abs() < 100.0)
                && (dot(dir, dir) - 1.0).abs() < 1e-9
            {
                out.push((o.map(mm), dir.map(|c| round_to(c, 12) + 0.0)));
                from = at + 56;
                break;
            }
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn the_seed_list_is_the_first_of_three_lists() {
        // Spacer_long_base_square LPattern1: two seeds, no body.
        let mut d = vec![0u8, 0, 0x6e, 0x81];
        d.extend([0x08, 0x80, 2, 0, 0x84, 0x04, 0x33, 0x06, 0x08, 0x80, 0, 0]);
        d.extend([1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0x08, 0x80, 0, 0, 0, 0]);
        assert_eq!(seed_list(&d, 0, d.len()), Some(Seeds { ptrs: vec![0x484, 0x633], bodies: 0 }));
        // The gear's body pattern: no seed, one body; an index past 0x7fff is escaped.
        let mut g = vec![0x77u8, 0x80, 1, 0, 0xff, 0x7f, 0x10, 0x80, 0, 0, 0x77, 0x80, 0, 0];
        g.extend([1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0x77, 0x80, 1, 0, 0, 0]);
        assert_eq!(seed_list(&g, 0, g.len()), Some(Seeds { ptrs: vec![0x8010], bodies: 1 }));
        // Without the middle words it is no seed list.
        g[16] = 9;
        assert_eq!(seed_list(&g, 0, g.len()), None);
    }

    #[test]
    fn back_references_give_the_object_index() {
        // Node 0 (id 37) holds a dimension owned by 0x0484; node 1 (id 61) is named as the
        // owner of a face reference to its own face; 0x0500 is claimed by both and dropped.
        let mut d = vec![0u8; 8];
        d.extend([0x84, 0x04, 1, 0, 1, 0, 0, 0, 0x84, 0x04, 1, 0, 0, 0]);
        d.extend([0x00, 0x05, 1, 0, 1, 0, 0, 0, 0x00, 0x05, 1, 0, 0, 0]);
        let n1 = d.len();
        d.extend([0x8b, 0x8a, 0x7c, 0x13, 0x10, 0x83, 0xc7, 0x11, 61, 0, 0, 0, 0xe9, 0x28, 0xd7, 0x5a, 1, 0, 0, 0]);
        d.extend([0x00, 0x05, 1, 0, 1, 0, 0, 0, 0x00, 0x05, 1, 0, 0, 0]);
        let spans = [(0, n1, 37), (n1, d.len(), 61)];
        let m = object_indices(&d, &spans);
        assert_eq!(m.get(&0x484), Some(&0));
        assert_eq!(m.get(&0x137c), Some(&1));
        assert_eq!(m.get(&0x500), None);
    }

    #[test]
    fn pattern_lines_read_the_start_and_direction() {
        // V-Slot Actuator End Mount: 20 bytes from the marker word; Front_panel: 32.
        for gap in [20usize, 32] {
            let mut d = vec![0u8; 3];
            d.extend([0xc7, 0xcf, 0xff, 0xff, 0, 0, 0, 0, 0x70, 0x17, 0, 0]);
            d.resize(3 + gap, 0);
            for v in [0.033f64, 0.0, 0.0365, 0.0016, 0.0, -1.0, 0.0] {
                d.extend(v.to_le_bytes());
            }
            let l = pattern_lines(&d, 0, d.len());
            assert_eq!(l.len(), 1, "{gap}");
            assert!((l[0].0[1] - 36.5).abs() < 1e-9 && l[0].1 == [0.0, -1.0, 0.0], "{l:?}");
        }
    }
}

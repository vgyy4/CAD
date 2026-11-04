//! Component placements in a Fusion 360 design stream (`Design1/BulkStream.dat`), read
//! clean-room from sample files (ADR 0039 amendment).
//!
//! What the samples show: names are UTF-16LE strings. Each component lists its bodies as
//! `BREP.<guid>.smb` (the `Breps.BlobParts` entry holding the body, in the component's own
//! coordinates). The component's occurrence follows: a rigid 4×4 matrix (16 little-endian
//! doubles, row-major, last row 0 0 0 1, centimetres), then its name `<component>:<instance>`
//! and the string `CreateComponent`. Bodies with no occurrence after them, before the next
//! body, belong to the root component and stay where they are.
//!
//! Only flat assemblies are read: a component placed inside another gets its transform
//! relative to its parent, as stored.

/// A rigid placement in millimetres: `p' = R p + t` (column vectors), `r` by rows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub r: [[f64; 3]; 3],
    pub t: [f64; 3],
}

/// A body's placement and the name of the occurrence that places it.
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    /// The body's blob id (`BREP.<id>.smb`).
    pub brep: String,
    pub occurrence: String,
    pub placement: Placement,
}

/// How far before an occurrence name its matrix may start, and how far before
/// `CreateComponent` the name may be (bytes).
const MATRIX_REACH: usize = 600;
const NAME_REACH: usize = 400;
/// Most occurrences read from one stream.
const MAX_OCCURRENCES: usize = 10_000;
/// Largest translation accepted (cm): beyond it the numbers are not a placement.
const MAX_OFFSET_CM: f64 = 1e7;

/// The UTF-16LE runs of printable ASCII (at least 3 characters) with their byte offsets.
fn utf16_runs(b: &[u8]) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < b.len() {
        let start = i;
        let mut s = String::new();
        while let (Some(&lo), Some(&hi)) = (b.get(i), b.get(i + 1)) {
            if hi == 0 && (0x20..0x7f).contains(&lo) {
                s.push(char::from(lo));
                i += 2;
            } else {
                break;
            }
        }
        if s.len() >= 3 {
            out.push((start, s));
        }
        if i == start {
            i += 1;
        }
    }
    out
}

fn f64_at(b: &[u8], at: usize) -> Option<f64> {
    let s: [u8; 8] = b.get(at..at + 8)?.try_into().ok()?;
    Some(f64::from_le_bytes(s))
}

/// A rigid matrix starting at `at`: finite, orthonormal rows, determinant +1, last row
/// 0 0 0 1, a translation within reach.
fn rigid_at(b: &[u8], at: usize) -> Option<Placement> {
    // The last element (1.0) first: most offsets fail here, cheaply.
    if f64_at(b, at + 120).map(f64::to_bits) != Some(1.0f64.to_bits()) {
        return None;
    }
    let v: Vec<f64> = (0..16).map(|k| f64_at(b, at + 8 * k)).collect::<Option<_>>()?;
    let g = |i: usize| v.get(i).copied().unwrap_or(f64::NAN);
    // The stored markers are exact: compare bit patterns (-0.0 counts as 0).
    let is = |x: f64, want: f64| x.to_bits() == want.to_bits() || (want == 0.0 && x.to_bits() == (-0.0f64).to_bits());
    if !(is(g(12), 0.0) && is(g(13), 0.0) && is(g(14), 0.0) && is(g(15), 1.0)) || !v.iter().all(|x| x.is_finite()) {
        return None;
    }
    let r = [[g(0), g(1), g(2)], [g(4), g(5), g(6)], [g(8), g(9), g(10)]];
    let dot = |a: [f64; 3], c: [f64; 3]| a[0] * c[0] + a[1] * c[1] + a[2] * c[2];
    let [a, c, d] = r;
    let unit = [a, c, d].iter().all(|row| (dot(*row, *row) - 1.0).abs() < 1e-6);
    let orth = dot(a, c).abs() < 1e-6 && dot(a, d).abs() < 1e-6 && dot(c, d).abs() < 1e-6;
    let det =
        a[0] * (c[1] * d[2] - c[2] * d[1]) - a[1] * (c[0] * d[2] - c[2] * d[0]) + a[2] * (c[0] * d[1] - c[1] * d[0]);
    let t = [g(3), g(7), g(11)];
    if !(unit && orth && (det - 1.0).abs() < 1e-6 && t.iter().all(|x| x.abs() <= MAX_OFFSET_CM)) {
        return None;
    }
    Some(Placement { r, t: t.map(|x| x * 10.0) })
}

/// The id in `BREP.<id>.smb` (not `.smbh`, which holds history).
fn brep_id(s: &str) -> Option<String> {
    let rest = s.strip_prefix("BREP.")?;
    let (id, tail) = rest.split_once('.')?;
    let ok = tail.starts_with("smb") && !tail.starts_with("smbh") && !tail.starts_with("smbB");
    (ok && !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-'))
        .then(|| id.to_string())
}

/// The placed bodies of a design stream. Bodies of the root component are left out.
pub fn placements(bulk: &[u8]) -> Vec<Placed> {
    let runs = utf16_runs(bulk);
    // Occurrences: (name offset, name, matrix).
    let mut occ: Vec<(usize, String, Placement)> = Vec::new();
    let mut seen = 0usize;
    for (k, (c, s)) in runs.iter().enumerate() {
        if !s.starts_with("CreateComponent") {
            continue;
        }
        // A design has a few hundred occurrences; a stream naming more is not looked at
        // further (each costs a bounded backward search).
        seen += 1;
        if seen > MAX_OCCURRENCES {
            break;
        }
        let Some((o, name)) = runs
            .get(k.saturating_sub(16)..k)
            .unwrap_or_default()
            .iter()
            .rev()
            .find(|(o, n)| c - o < NAME_REACH && n.contains(':'))
        else {
            continue;
        };
        // The matrix nearest before the name.
        let m = (o.saturating_sub(MATRIX_REACH)..*o).rev().find_map(|at| {
            if at + 128 > *o {
                return None;
            }
            rigid_at(bulk, at)
        });
        if let Some(m) = m {
            occ.push((*o, name.clone(), m));
        }
    }
    // Every `BREP.` in a run (neighbouring strings can run together).
    let refs: Vec<(usize, String)> = runs
        .iter()
        .flat_map(|(o, s)| {
            s.match_indices("BREP.").filter_map(move |(i, _)| brep_id(s.get(i..)?).map(|id| (o + 2 * i, id)))
        })
        .collect();
    let mut out = Vec::new();
    for (i, (at, id)) in refs.iter().enumerate() {
        let next_ref = refs.get(i + 1).map_or(usize::MAX, |r| r.0);
        // The first occurrence after the body, unless another body comes first.
        let k = occ.partition_point(|(o, _, _)| o < at);
        if let Some((o, name, m)) = occ.get(k)
            && *o < next_ref
        {
            out.push(Placed { brep: id.clone(), occurrence: name.clone(), placement: *m });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;

    fn u16s(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    fn matrix(t: [f64; 3], quarter_turn: bool) -> Vec<u8> {
        // Rows (x', y', z'): a quarter turn about Z maps x to y.
        let (a, b) = if quarter_turn { ([0.0, -1.0], [1.0, 0.0]) } else { ([1.0, 0.0], [0.0, 1.0]) };
        let m = [a[0], a[1], 0.0, t[0], b[0], b[1], 0.0, t[1], 0.0, 0.0, 1.0, t[2], 0.0, 0.0, 0.0, 1.0];
        m.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    #[test]
    fn bodies_take_the_occurrence_after_them() {
        let mut b = vec![7u8; 40];
        b.extend(u16s("BREP.aa11.smb"));
        b.extend([0u8; 90]);
        b.extend(u16s("BREP.bb22.smbh"));
        b.extend(u16s("BREP.bb22.smb"));
        b.extend([1u8; 33]);
        b.extend(matrix([0.8, 2.0, 0.157], true));
        b.extend([0u8; 20]);
        b.extend(u16s("BTN:SW1"));
        b.extend([0u8; 30]);
        b.extend(u16s("CreateComponent!"));
        b.extend(u16s("BREP.cc33.smb"));
        let p = placements(&b);
        // aa11 has a body after it before the occurrence: the root's. bb22 is placed.
        assert_eq!(p.len(), 1, "{p:?}");
        assert_eq!(p[0].brep, "bb22");
        assert_eq!(p[0].occurrence, "BTN:SW1");
        let t = p[0].placement.t;
        assert!((t[0] - 8.0).abs() + (t[1] - 20.0).abs() + (t[2] - 1.57).abs() < 1e-12, "{t:?}");
        assert!(p[0].placement.r[0].iter().zip([0.0, -1.0, 0.0]).all(|(a, b)| (a - b).abs() < 1e-15));
    }

    #[test]
    fn a_mirror_or_a_scale_is_not_a_placement_and_noise_is_harmless() {
        let mut m = matrix([1.0, 2.0, 3.0], false);
        m[80..88].copy_from_slice(&(-1.0f64).to_le_bytes()); // z row flipped: a mirror
        assert!(rigid_at(&m, 0).is_none());
        let mut s = matrix([1.0, 2.0, 3.0], false);
        s[0..8].copy_from_slice(&2.0f64.to_le_bytes());
        assert!(rigid_at(&s, 0).is_none());
        for n in [0usize, 1, 7, 127, 128, 1000] {
            let noise: Vec<u8> = (0..n).map(|i| (i * 37 % 251) as u8).collect();
            let _ = placements(&noise);
        }
    }
}

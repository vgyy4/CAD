//! Inventor derived parts and iPart members (`docs/formats/inventor-feature-tree.md`, note AO):
//! a part whose history starts with the body of another part file (feature `8e29cbee`,
//! "Solid1::AN806.ipt"). Clean room, from sample bytes.
//!
//! The derive feature's slot 2 leads (through a `2148c03c` record) to the derive's definition:
//! `3f3634a0` for a derived part, `338634ac` for an iPart member. The definition refers to the
//! base file's name (a `255d7ed7` record: a length-prefixed UTF-16 plain file name), a placement
//! (`90874d18`, the sketch placement's mask-compressed 4x4, cm; a mirrored part's is a reflection)
//! and, through a `3f36349f` record, the scale parameter. Every derived part also holds a tagging
//! feature (`dfdb99e8`) that changes no geometry.
//!
//! Only what the file stores is read here; finding and reading the base file is the caller's
//! (the caller decides which folder may be read, and how deep a chain of bases may go).

use crate::Limits;
use crate::inventor_dc::{Dc, FeatType, Role, u32_at};

/// A derive feature.
const T_DERIVE: u32 = 0x8e29_cbee;
/// The tagging feature every derived part holds; no geometry.
const T_TAGGING: u32 = 0xdfdb_99e8;
/// The definition of a derived part.
const G_DERIVED_PART: u32 = 0x3f36_34a0;
/// The definition of an iPart member.
const G_IPART_MEMBER: u32 = 0x3386_34ac;
/// The definition's options record (the scale parameter, the mirror flags, the placement).
const G_DERIVE_OPTIONS: u32 = 0x3f36_349f;
/// The record that names the base file.
const G_BASE_FILE: u32 = 0x255d_7ed7;
/// How far from the derive feature its definition is looked for (records).
const MAX_WALK: usize = 3;
/// Longest base file name read (UTF-16 units).
const MAX_NAME: usize = 255;

/// What a derived part or iPart member stores about its base.
#[derive(Debug, Clone, PartialEq)]
pub struct DerivedBase {
    /// The base part's plain file name as the part stores it (e.g. "AN806.ipt").
    pub file: String,
    /// An iPart member (the base is its factory); else a derived part.
    pub member: bool,
    /// The placement of the base's body in this part: row-major 4x4, translation in mm.
    pub transform: [f64; 16],
    /// The scale factor of the derive (1 when not scaled).
    pub scale: f64,
    /// The features of the part after the derive (besides its tagging feature), by name.
    pub own_features: Vec<String>,
}

/// The base a derived part or iPart member stores; `None` for a part that derives nothing.
///
/// # Errors
/// The file is not a readable Inventor part, or its derive is not shaped as read here (several
/// derives, no definition, no file name, placement or scale).
pub fn derived_base(bytes: &[u8], limits: &Limits) -> Result<Option<DerivedBase>, String> {
    let dc = Dc::open(bytes, limits)?;
    let feats = dc.ordered_features();
    let derives: Vec<_> = feats.iter().filter(|f| f.typ == Some(FeatType::Other(T_DERIVE))).collect();
    let Some(derive) = derives.first() else { return Ok(None) };
    if derives.len() > 1 {
        return Err("it derives the bodies of more than one part".into());
    }
    let suppressed = |rec: usize| dc.r(rec).get(10).is_some_and(|b| b & 0x04 != 0);
    let own_features: Vec<String> = feats
        .iter()
        .filter(|f| f.rec != derive.rec && f.typ != Some(FeatType::Other(T_TAGGING)) && !suppressed(f.rec))
        .map(|f| f.name.clone().unwrap_or_else(|| format!("Feature{}", f.rec)))
        .collect();
    let def = find_near(&dc, derive.rec, |c| c == G_DERIVED_PART || c == G_IPART_MEMBER)
        .ok_or("the derive's definition is not readable")?;
    let member = dc.class(def) == G_IPART_MEMBER;
    let file = dc
        .refs(def)
        .into_iter()
        .find(|&(_, x)| dc.class(x) == G_BASE_FILE)
        .and_then(|(_, x)| file_name(dc.r(x)))
        .ok_or("the derive names no base file")?;
    let options = dc.refs(def).into_iter().map(|x| x.1).find(|&x| dc.class(x) == G_DERIVE_OPTIONS);
    // The placement: the definition's own, else its options record's.
    let placement = [Some(def), options]
        .into_iter()
        .flatten()
        .flat_map(|i| dc.refs(i))
        .map(|x| x.1)
        .find(|&x| dc.role(x) == Role::SkPlacement)
        .and_then(|x| dc.placement(x))
        .ok_or("the derive's placement is not readable")?;
    let mut transform = placement;
    for k in [3, 7, 11] {
        if let Some(t) = transform.get_mut(k) {
            *t *= 10.0;
        }
    }
    let scale = options
        .into_iter()
        .flat_map(|o| dc.refs(o))
        .map(|x| x.1)
        .find(|&x| dc.role(x) == Role::Param)
        .and_then(|x| dc.param(x))
        .map(|p| p.1)
        .ok_or("the derive's scale is not readable")?;
    if !scale.is_finite() || scale <= 0.0 || !transform.iter().all(|v| v.is_finite()) {
        return Err("the derive's scale or placement is not a number".into());
    }
    Ok(Some(DerivedBase { file, member, transform, scale, own_features }))
}

/// The part's parameters (name, value in cm or radians), in record order.
///
/// # Errors
/// The file is not a readable Inventor part.
pub fn parameters(bytes: &[u8], limits: &Limits) -> Result<Vec<(String, f64)>, String> {
    let dc = Dc::open(bytes, limits)?;
    Ok((0..dc.n()).filter(|&i| dc.role(i) == Role::Param).filter_map(|i| dc.param(i)).collect())
}

/// The nearest record of a class `want` takes, within [`MAX_WALK`] references of `from`.
fn find_near(dc: &Dc, from: usize, want: impl Fn(u32) -> bool) -> Option<usize> {
    let mut seen = vec![from];
    let mut front = vec![from];
    for _ in 0..MAX_WALK {
        let mut next = Vec::new();
        for i in front {
            for (_, x) in dc.refs(i) {
                if seen.contains(&x) || dc.role(x) == Role::PartFeature {
                    continue;
                }
                if want(dc.class(x)) {
                    return Some(x);
                }
                seen.push(x);
                next.push(x);
            }
        }
        front = next;
    }
    None
}

/// The first length-prefixed UTF-16 string of a record that is a plain `.ipt` file name.
fn file_name(r: &[u8]) -> Option<String> {
    for q in 0..r.len().saturating_sub(4) {
        let Some(l) = u32_at(r, q).map(|l| l as usize) else { continue };
        if !(5..=MAX_NAME).contains(&l) {
            continue;
        }
        let Some(raw) = r.get(q + 4..q + 4 + 2 * l) else { continue };
        let units: Vec<u16> = raw
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c.first().copied().unwrap_or(0), c.get(1).copied().unwrap_or(0)]))
            .collect();
        let Ok(s) = String::from_utf16(&units) else { continue };
        if s.to_ascii_lowercase().ends_with(".ipt") && !s.chars().any(char::is_control) {
            return Some(s);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;
    use crate::fixtures::{inventor_dc_part, inventor_derived_part};

    #[test]
    fn a_derived_part_names_its_base_and_placement() {
        let d = derived_base(&inventor_derived_part("AN806.ipt", false, false, false), &Limits::default())
            .unwrap()
            .unwrap();
        assert_eq!(d.file, "AN806.ipt");
        assert!(!d.member);
        assert!((d.scale - 1.0).abs() < 1e-12);
        assert!(d.own_features.is_empty(), "{:?}", d.own_features);
        let id: [f64; 16] = std::array::from_fn(|k| if k % 5 == 0 { 1.0 } else { 0.0 });
        assert!(d.transform.iter().zip(&id).all(|(a, b)| (a - b).abs() < 1e-12), "{:?}", d.transform);
        // A mirrored part's placement turns z round; an iPart member says so.
        let m = derived_base(&inventor_derived_part("rev1 corners.ipt", true, true, true), &Limits::default())
            .unwrap()
            .unwrap();
        assert!(m.member);
        assert_eq!(m.file, "rev1 corners.ipt");
        assert!(m.transform.get(10).is_some_and(|v| (v + 1.0).abs() < 1e-12));
        assert_eq!(m.own_features, vec!["Extrusion1".to_string()]);
        let p = parameters(&inventor_derived_part("a.ipt", true, false, false), &Limits::default()).unwrap();
        assert_eq!(p, vec![("d0".to_string(), 1.0)]);
    }

    #[test]
    fn a_part_without_a_derive_has_no_base() {
        let mut name = vec![0u8; 0x1a];
        name.extend(10u32.to_le_bytes());
        for u in "Extrusion1".encode_utf16() {
            name.extend(u.to_le_bytes());
        }
        name.extend(0x0ca9_1131u32.to_le_bytes());
        name.extend([0x11u8; 12]);
        let mut feature = vec![0u8; 22];
        feature[6..10].copy_from_slice(&0x8000_0001u32.to_le_bytes());
        let p = inventor_dc_part(&[0x2b48_a42b, 0x9087_4d91], &[(0, name), (1, feature)], &[]);
        assert_eq!(derived_base(&p, &Limits::default()).unwrap(), None);
        assert!(derived_base(b"not a part", &Limits::default()).is_err());
    }

    #[test]
    fn file_names_are_plain_ipt_names_only() {
        let utf16 = |s: &str| {
            let mut v = (s.chars().count() as u32).to_le_bytes().to_vec();
            v.extend(s.encode_utf16().flat_map(u16::to_le_bytes));
            v
        };
        assert_eq!(file_name(&utf16("a.IPT")), Some("a.IPT".into()));
        assert_eq!(file_name(&utf16("a.iam")), None);
        assert_eq!(file_name(&utf16("ab\u{1}.ipt")), None);
    }
}

//! MirrorComponent `71BBEDD5` and its DerivedInstance `66E375EA` (section 33 of
//! `docs/formats/fusion-design-stream.md`): a new component whose bodies are the mirror of the
//! bodies of another component's occurrence, placed by an occurrence of its own.
//!
//! The mirror plane is a plane of the component that holds both occurrences; the new
//! component's occurrence is a proper rigid placement, so the reflection is in the new
//! component's own bodies. In the new component's coordinates those bodies are the source's
//! bodies moved by `T_new^-1 . M . T_source`; the tree writes one body mirror per source body
//! with the plane of that map (it must be a reflection in a plane, which it is when the two
//! placements differ by the reflection's own turn).

use super::feature::plane_frame;
use super::geom::{V3, cross, dot, mm, round6, round9};
use super::stream::{Design, ref_ids};
use crate::fusion::Placement;
use serde_json::{Value, json};

/// The class of a MirrorComponent feature.
pub(super) const MIRROR_COMPONENT: &str = "71BBEDD5";
/// The class of the DerivedInstance that places a mirrored component.
pub(super) const DERIVED_INSTANCE: &str = "66E375EA";
const OCCURRENCE: &str = "CE2913AA";
const COMPONENT: &str = "E03784ED";
const OCCURRENCE_HOLDER: &str = "2D6E13A1";
const OCCURRENCE_LIST: &str = "904F885E";

/// What one MirrorComponent names.
pub(super) struct MirrorComp {
    /// The component mirrored and the new one.
    pub source: u64,
    pub new: u64,
    /// The component that holds both occurrences (the mirror plane is in its coordinates).
    pub parent: u64,
    /// The mirror plane: origin (mm) and unit normal.
    pub plane: (V3, V3),
    /// The occurrences' placements in the parent (mm).
    pub ts: Placement,
    pub tn: Placement,
}

fn first_of(d: &Design, q: u64, class: &str) -> Option<u64> {
    ref_ids(d, d.rec(q)).into_iter().find(|r| d.is(*r, class))
}

/// The rigid placement an occurrence record stores (the first such matrix in it).
fn occurrence_matrix(d: &Design, occ: u64) -> Option<Placement> {
    let rec = d.rec(occ);
    (0..rec.len().saturating_sub(127)).find_map(|at| crate::fusion::rigid_at(rec, at))
}

/// The component an occurrence record lists it in: its `904F885E` list names that component.
fn occurrence_parent(d: &Design, occ: u64) -> Option<u64> {
    first_of(d, occ, OCCURRENCE_LIST).and_then(|l| ref_ids(d, d.rec(l)).into_iter().rfind(|r| d.is(*r, COMPONENT)))
}

/// Reads a MirrorComponent: its source occurrence, mirror plane and the new component's
/// occurrence (through its DerivedInstance).
pub(super) fn read(cx: &super::Ctx, fid: u64) -> Result<MirrorComp, String> {
    let d = cx.d;
    let refs = ref_ids(d, d.rec(fid));
    let src_occ =
        refs.iter().copied().find(|r| d.is(*r, OCCURRENCE)).ok_or("the mirror names no occurrence to mirror")?;
    let source = first_of(d, src_occ, COMPONENT).ok_or("the mirrored occurrence names no component")?;
    let di = refs.iter().copied().find(|r| d.is(*r, DERIVED_INSTANCE)).ok_or("the mirror names no DerivedInstance")?;
    let new_occ = first_of(d, di, OCCURRENCE_HOLDER)
        .and_then(|h| first_of(d, h, OCCURRENCE))
        .ok_or("the DerivedInstance names no occurrence")?;
    let new = first_of(d, new_occ, COMPONENT).ok_or("the new occurrence names no component")?;
    if new == source {
        return Err("the new occurrence is of the mirrored component itself".into());
    }
    let parent = occurrence_parent(d, src_occ).ok_or("the mirrored occurrence is in no component's list")?;
    if occurrence_parent(d, new_occ) != Some(parent) {
        return Err("the two occurrences are not in the same component".into());
    }
    let sel = refs.iter().copied().find(|r| d.is(*r, "5A1BF548")).ok_or("the mirror names no plane")?;
    let (o, n) = cx
        .under(sel, "CC54ECAD", 3, None)
        .into_iter()
        .find_map(|q| plane_frame(d, q))
        .ok_or("the mirror plane is not an origin/construction plane frame")?;
    let ts = occurrence_matrix(d, src_occ).ok_or("the mirrored occurrence stores no placement")?;
    let tn = occurrence_matrix(d, new_occ).ok_or("the new occurrence stores no placement")?;
    // The DerivedInstance repeats the new occurrence's placement.
    let same = |a: &Placement, b: &Placement| {
        a.r.iter().flatten().zip(b.r.iter().flatten()).all(|(x, y)| (x - y).abs() < 1e-9)
            && a.t.iter().zip(&b.t).all(|(x, y)| (x - y).abs() < 1e-6)
    };
    if occurrence_matrix(d, di).is_none_or(|m| !same(&m, &tn)) {
        return Err("the DerivedInstance and the new occurrence are placed differently".into());
    }
    Ok(MirrorComp { source, new, parent, plane: ([mm(o[0]), mm(o[1]), mm(o[2])], n), ts, tn })
}

/// The reflection in the plane through `o` (mm) with unit normal `n`, as a (improper) placement.
pub(super) fn reflection(o: V3, n: V3) -> Placement {
    let mut r = [[0.0; 3]; 3];
    for (i, row) in r.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            let ni = n.get(i).copied().unwrap_or(0.0);
            let nj = n.get(j).copied().unwrap_or(0.0);
            *v = f64::from(u8::from(i == j)) - 2.0 * ni * nj;
        }
    }
    let k = 2.0 * dot(o, n);
    Placement { r, t: [k * n[0], k * n[1], k * n[2]] }
}

/// The plane (origin in mm, unit normal) of a map that is a reflection in a plane, else `None`.
pub(super) fn reflection_plane(a: &Placement) -> Option<(V3, V3)> {
    let g = |i: usize, j: usize| a.r.get(i).and_then(|row| row.get(j)).copied().unwrap_or(f64::NAN);
    // I - R = 2 n n^T: its largest diagonal entry gives a column along n.
    let k = (0..3).max_by(|x, y| (1.0 - g(*x, *x)).total_cmp(&(1.0 - g(*y, *y))))?;
    let col = [0, 1, 2].map(|i| f64::from(u8::from(i == k)) - g(i, k));
    let len = dot(col, col).sqrt();
    if len.is_nan() || len <= 1.0 {
        return None;
    }
    let n = col.map(|x| x / len);
    let want = reflection([0.0; 3], n);
    let ok = want.r.iter().enumerate().all(|(i, row)| row.iter().enumerate().all(|(j, w)| (g(i, j) - w).abs() < 1e-9));
    let along = dot(a.t, n);
    let across = cross(a.t, n);
    if !ok || dot(across, across).sqrt() > 1e-6 {
        return None;
    }
    let o = n.map(|x| x * along / 2.0);
    Some((o, n))
}

/// The map from the source component's coordinates to the new one's, each where it stands in
/// the parent (`ts`, `tn`), with the importer's own placements of the two (`ps`, `pn`) applied.
pub(super) fn body_map(m: &MirrorComp, ps: &Placement, pn: &Placement) -> Placement {
    let mirror = reflection(m.plane.0, m.plane.1);
    pn.then(&m.tn.inverse()).then(&mirror).then(&m.ts).then(&ps.inverse())
}

fn placement_json(p: &Placement) -> Value {
    json!({"r": p.r, "t": p.t})
}

impl MirrorComp {
    pub(super) fn keys_json(&self) -> Value {
        json!({"source": self.source, "new": self.new, "parent": self.parent,
            "origin": self.plane.0, "normal": self.plane.1,
            "ts": placement_json(&self.ts), "tn": placement_json(&self.tn)})
    }
}

/// The keys back from a feature's `_mirror_component`.
pub(super) fn keys_of(v: &Value) -> Option<MirrorComp> {
    let get3 = |k: &str| -> Option<V3> {
        let a = v.get(k)?.as_array()?;
        Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?])
    };
    Some(MirrorComp {
        source: v.get("source")?.as_u64()?,
        new: v.get("new")?.as_u64()?,
        parent: v.get("parent")?.as_u64()?,
        plane: (get3("origin")?, get3("normal")?),
        ts: super::json::placement_of(v.get("ts")?)?,
        tn: super::json::placement_of(v.get("tn")?)?,
    })
}

/// The plane JSON of a reflection plane.
pub(super) fn plane_json((o, n): (V3, V3)) -> Value {
    json!({"origin": [round6(o[0]), round6(o[1]), round6(o[2])], "normal": [round9(n[0]), round9(n[1]), round9(n[2])]})
}

/// The bodies a component has after the features read so far, by the tree's names (a
/// feature's name stands for its body, a body pattern's copies are `<name>[i]`).
pub(super) fn component_bodies_so_far(feats: &[Value], comp: u64) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    let s = |f: &Value, k: &str| f.get(k).and_then(Value::as_str).map(str::to_string);
    for f in feats.iter().filter(|f| f.get("_component").and_then(Value::as_u64) == Some(comp)) {
        let ty = s(f, "type").unwrap_or_default();
        let name = s(f, "name").unwrap_or_default();
        let made = f.get("creates_bodies").and_then(Value::as_u64).unwrap_or(0);
        let merge = f.get("merge").and_then(Value::as_bool);
        let no_features = f.get("features").and_then(Value::as_array).is_none_or(Vec::is_empty);
        match ty.as_str() {
            "sketch" | "fillet" | "chamfer" | "shell" | "draft" | "offset_face" | "hole" | "scale_body" => {}
            "split" => out.push(name),
            "extrude" | "revolve" | "sweep" | "loft" => {
                let op = s(f, "operation").unwrap_or_default();
                match (op.as_str(), made) {
                    ("new", 1) => out.push(name),
                    ("new", _) => return Err(format!("{name} makes {made} bodies")),
                    (_, 0) => {}
                    _ => return Err(format!("{name} changes a body and makes {made}")),
                }
            }
            "stored_body" => out.push(name),
            "mirror" if merge == Some(false) && no_features && made == 1 => out.push(name),
            "mirror" if made == 0 => {}
            "pattern_linear" | "pattern_circular" if merge == Some(false) && no_features => {
                let count = f.get("count").and_then(Value::as_u64).unwrap_or(0);
                if f.get("count2").is_some() || !(2..=10_000).contains(&count) {
                    return Err(format!("the body pattern {name} is not one direction of copies"));
                }
                out.extend((1..count).map(|i| format!("{name}[{i}]")));
            }
            "pattern_linear" | "pattern_circular" if merge != Some(false) => {}
            "move_body" if f.get("copy").and_then(Value::as_bool) != Some(true) => {}
            "remove_body" => {
                let t = s(f, "target").unwrap_or_default();
                out.retain(|b| *b != t);
            }
            _ => return Err(format!("{name} ({ty}) of the mirrored component is not followed")),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;

    #[test]
    fn a_reflection_gives_its_plane_back() {
        let n = [0.0, 0.6, 0.8];
        let m = reflection([1.0, 2.0, 3.0], n);
        let (o, back) = reflection_plane(&m).unwrap();
        assert!((dot(back, n) - 1.0).abs() < 1e-12);
        // The plane through o with normal n holds the original origin's projection.
        assert!((dot(o, n) - dot([1.0, 2.0, 3.0], n)).abs() < 1e-12);
        // A turn is no reflection.
        assert!(reflection_plane(&Placement::IDENTITY).is_none());
    }

    #[test]
    fn the_header_mirror_is_a_reflection_in_its_own_xy_plane() {
        // 5905: the header at (-24.12, -8.9, 1.54) mm, the mirror turned half about x at
        // (-24.12, 8.9, 1.54) mm, the plane y = 0 of the root: in the components' own
        // coordinates the bodies are reflected in z = 0.
        let ts = Placement { r: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], t: [-24.12, -8.9, 1.54] };
        let tn = Placement { r: [[1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, -1.0]], t: [-24.12, 8.9, 1.54] };
        let k = MirrorComp { source: 1, new: 2, parent: 3, plane: ([0.0; 3], [0.0, 1.0, 0.0]), ts, tn };
        let a = body_map(&k, &Placement::IDENTITY, &Placement::IDENTITY);
        let (o, n) = reflection_plane(&a).unwrap();
        assert!((n[2].abs() - 1.0).abs() < 1e-12 && dot(o, o) < 1e-18, "{o:?} {n:?}");
    }
}

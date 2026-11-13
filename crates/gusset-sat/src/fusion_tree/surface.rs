//! Surface extrudes and Thicken `BCE0AD0E` (section 49 of the format notes).
//!
//! A surface extrude is an ordinary extrude record (`DD405BC2`) whose profile selection
//! (`2CA5A1CD`) names sketch curves (`5A1BF548` -> `E2CEFD18`, as a sweep path does) instead
//! of a `4BD53E5A` profile: it sweeps the curves into a sheet.
//!
//! Thicken's body: `u32 1`, `u8 0`, the operation as a `u32` (as an extrude's: 1 join, 4 new),
//! then `u32 n` and `n` selections (`2CA5A1CD`, the sheets), two flag bytes (`01 01` in every
//! record of the corpus: 4632 and 2530), the parameter holder (`Thickness`), and a second list:
//! for a join the one selection of the body joined to (4632: `Extrude10`'s hub), for a new body
//! the body links made (2530: two). The flags are not decoded (Fusion's thicken input has
//! `isSymmetric` and `isChainSelection`); the thicken is read as symmetric, which no rebuild has
//! confirmed yet (4632's file stores no thickened blade; 2530 stops before its checks).

use super::bodyops::Edit;
use super::stream::{Design, parse_header, ref_ids, ref_lists, u32_at};
use crate::cover;
use serde_json::{Value, json};

/// The sketch and curves a surface extrude sweeps: its profile selection's sketch curves
/// (`2CA5A1CD` -> `5A1BF548` -> `E2CEFD18`, whose last 16 bytes are the sketch's id and the
/// curve's `crv_primary_id`, as a sweep path's). Lines, arcs and splines (4632: one B-spline).
pub(super) fn surface_curves(cx: &mut super::Ctx, fid: u64) -> Option<(u64, Vec<u64>)> {
    let d = cx.d;
    let sels: Vec<u64> =
        ref_ids(d, d.rec(fid)).into_iter().filter(|q| d.is(*q, "2CA5A1CD") && curve_selection(d, *q)).collect();
    let mut sketch: Option<u64> = None;
    let mut curves: Vec<u64> = Vec::new();
    for sel in sels {
        for q in ref_ids(d, d.rec(sel)).into_iter().filter(|q| d.is(*q, "5A1BF548")) {
            for c in ref_ids(d, d.rec(q)).into_iter().filter(|c| d.is(*c, "E2CEFD18")) {
                let r = d.rec(c);
                let at = r.len().checked_sub(16)?;
                let word = |o: usize| r.get(o..o + 8).and_then(|b| b.try_into().ok()).map(u64::from_le_bytes);
                let (sk, prim) = (word(at)?, word(at + 8)?);
                cover::field(r, at, 16, "surface-extrude.sketch-curve");
                if sketch.is_some_and(|s| s != sk) {
                    return None;
                }
                sketch = Some(sk);
                let s = cx.sketch(sk);
                let cid = s
                    .lines
                    .keys()
                    .chain(s.arcs.keys())
                    .chain(s.splines.keys())
                    .copied()
                    .find(|q| super::sketch::crv_key(d, *q).is_some_and(|k| k.0 == prim))?;
                if !curves.contains(&cid) {
                    curves.push(cid);
                }
            }
        }
    }
    sketch.filter(|_| !curves.is_empty()).map(|s| (s, curves))
}

/// Whether a selection names sketch curves.
fn curve_selection(d: &Design, sel: u64) -> bool {
    ref_ids(d, d.rec(sel))
        .into_iter()
        .filter(|q| d.is(*q, "5A1BF548"))
        .any(|q| ref_ids(d, d.rec(q)).iter().any(|c| d.is(*c, "E2CEFD18")))
}

impl Edit<'_> {
    /// The tree entry of a Thicken: `{"type": "thicken", "sheets": [...], "thickness": mm,
    /// "symmetric": true, "operation": "join"|"new", "target": ...}`.
    pub(super) fn thicken(&self, fid: u64, name: &str) -> Result<Value, String> {
        let d = self.d;
        let rec = d.rec(fid);
        let h = parse_header(rec).ok_or("the Thicken's record is not recognised")?;
        // `u32 1`, `u8 0`, the operation, then the list of sheets (`u32 n`, a reference).
        let b = (h.body.saturating_sub(8)..h.body + 24)
            .find(|&o| {
                u32_at(rec, o) == Some(1)
                    && rec.get(o + 4) == Some(&0)
                    && matches!(u32_at(rec, o + 5), Some(1..=4))
                    && u32_at(rec, o + 9).is_some_and(|n| (1..=256).contains(&n))
                    && rec.get(o + 13) == Some(&1)
            })
            .ok_or("the Thicken's record does not start as the known ones")?;
        cover::skip(rec, b, 5, "thicken.lead");
        cover::field(rec, b + 5, 4, "thicken.operation");
        let op = match u32_at(rec, b + 5) {
            Some(1) => "join",
            Some(4) => "new",
            _ => return Err("the Thicken's operation is not join or new".into()),
        };
        let lists: Vec<Vec<u64>> = ref_lists(d, rec, b + 9).into_iter().map(|(_, ids)| ids).collect();
        let sheets_sel = lists.first().filter(|l| l.iter().all(|q| d.is(*q, "2CA5A1CD")));
        let sheets_sel = sheets_sel.ok_or("the Thicken's sheets are not where the known layout has them")?;
        let mut sheets: Vec<String> = Vec::new();
        for s in sheets_sel {
            let [l] = self.links_of(*s)[..] else { return Err("a sheet of the Thicken is not one body".into()) };
            match self.maker(l) {
                Some((_, f))
                    if f.get("type").and_then(Value::as_str) == Some("extrude") && f.get("curves").is_some() =>
                {
                    let n = f.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
                    if sheets.contains(&n) {
                        return Err("the Thicken names a sheet twice".into());
                    }
                    sheets.push(n);
                }
                _ => return Err("a sheet of the Thicken is not a surface extrude of the tree".into()),
            }
        }
        let target = if op == "join" {
            let t = lists.get(1).filter(|l| l.len() == 1 && l.iter().all(|q| d.is(*q, "2CA5A1CD")));
            let t = t.and_then(|l| l.first()).ok_or("the body the Thicken joins is not one selection")?;
            let [l] = self.links_of(*t)[..] else { return Err("the Thicken joins more than one body".into()) };
            match self.maker(l) {
                Some((_, f)) if f.get("creates_bodies").and_then(Value::as_u64) == Some(1) => {
                    Value::from(f.get("name").and_then(Value::as_str).unwrap_or_default())
                }
                _ => return Err("the body the Thicken joins is not the one body of a feature of the tree".into()),
            }
        } else {
            Value::Null
        };
        let t = super::feature::param_values(d, fid, &["Thickness"]);
        let [t] = t[..] else { return Err("the Thicken has no thickness".into()) };
        if !(t.is_finite() && t != 0.0) {
            return Err("the Thicken's thickness is zero".into());
        }
        Ok(json!({"type": "thicken", "name": name, "source_id": fid, "sheets": sheets,
            "thickness": super::geom::round6(super::geom::mm(t)), "symmetric": true, "operation": op,
            "target": target,
            "note": "symmetric: the flag bytes are not decoded and no rebuild has confirmed the side"}))
    }
}

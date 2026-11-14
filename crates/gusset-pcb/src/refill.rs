//! Filling on edit (ADR 0053): after an edit that touches copper, the zones it may have
//! changed are poured again.
//!
//! The edit is compared with the board before it: tracks, vias, pads (a footprint moved,
//! flipped or removed shows as its pads) and zones (outline, layers, net, settings) that
//! are new, gone or different. A zone is *affected* when such an item lies within reach
//! of its outline (the larger of its clearance and the rules', plus the thermal gap and a
//! millimetre) on a layer it covers; zones that overlap an affected one are poured with
//! it, as priorities make them depend on each other. A zone the edit itself just filled
//! (new fills that differ from the old ones) is left as it is.

use crate::board::{Board, Error, Limits, Pt, Zone};
use crate::rules::Rules;
use crate::zones::{self, Fill};
use std::collections::HashSet;

/// A changed item: where it is (min, max) and the layer it is on (`None`: all copper).
type Spot = ([Pt; 2], Option<String>);

fn spot(points: &[Pt], r: f64, layer: Option<String>) -> Spot {
    let lo = points.iter().fold([f64::MAX; 2], |a, p| [a[0].min(p[0]), a[1].min(p[1])]);
    let hi = points.iter().fold([f64::MIN; 2], |a, p| [a[0].max(p[0]), a[1].max(p[1])]);
    ([[lo[0] - r, lo[1] - r], [hi[0] + r, hi[1] + r]], layer)
}

/// Every copper item of a board as (comparison key, where it is).
fn items(b: &Board) -> Vec<(String, Spot)> {
    let mut v = Vec::new();
    for t in &b.tracks {
        let key = format!("t {:?} {:?} {:?} {} {} {:?}", t.start, t.end, t.mid, t.width, t.layer, b.net_name(&t.net));
        let pts: Vec<Pt> = match t.mid {
            Some(m) => crate::geom::arc_points(t.start, m, t.end, 0.01),
            None => vec![t.start, t.end],
        };
        v.push((key, spot(&pts, t.width / 2.0, Some(t.layer.clone()))));
    }
    for x in &b.vias {
        let key = format!("v {:?} {} {} {:?} {:?}", x.at, x.size, x.drill, x.layers, b.net_name(&x.net));
        v.push((key, spot(&[x.at], x.size / 2.0, None)));
    }
    for p in &b.pads {
        let key = format!(
            "p {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?}",
            p.at,
            p.angle,
            p.size,
            p.shape,
            p.kind,
            p.layers,
            p.zone_connect,
            b.net_name(&p.net),
            p.primitives
        );
        let r = libm::hypot(p.size[0], p.size[1]) / 2.0;
        let r = p.primitives.iter().fold(r, |r, s| match s {
            crate::board::Shape::Circle { center, radius } => {
                r.max(libm::hypot(center[0] - p.at[0], center[1] - p.at[1]) + radius)
            }
            crate::board::Shape::Poly { pts } => {
                pts.iter().fold(r, |r, q| r.max(libm::hypot(q[0] - p.at[0], q[1] - p.at[1])))
            }
            _ => r,
        });
        v.push((key, spot(&[p.at], r, None)));
    }
    v
}

/// The box of the tracks, vias and pads that differ between two boards (`None`: none).
pub fn changed_box(old: &Board, new: &Board) -> Option<[Pt; 2]> {
    let (a, b) = (items(old), items(new));
    let before: HashSet<&str> = a.iter().map(|(k, _)| k.as_str()).collect();
    let after: HashSet<&str> = b.iter().map(|(k, _)| k.as_str()).collect();
    let gone = a.iter().filter(|(k, _)| !after.contains(k.as_str()));
    let new_ones = b.iter().filter(|(k, _)| !before.contains(k.as_str()));
    gone.chain(new_ones)
        .map(|(_, s)| s.0)
        .reduce(|x, y| [[x[0][0].min(y[0][0]), x[0][1].min(y[0][1])], [x[1][0].max(y[1][0]), x[1][1].max(y[1][1])]])
}

/// What makes a zone the same zone (fills left out).
fn zone_key(b: &Board, z: &Zone) -> String {
    format!(
        "{:?} {:?} {:?} {:?} {:?} {:?}",
        z.outline,
        z.more_outlines,
        z.layers,
        b.net_name(&z.net),
        z.settings,
        z.keepout
    )
}

fn box_of(z: &Zone) -> [Pt; 2] {
    let all: Vec<Pt> = z.outlines().flatten().copied().collect();
    spot(&all, 0.0, None).0
}

fn overlap(a: &[Pt; 2], b: &[Pt; 2], m: f64) -> bool {
    crate::copper::boxes_near(a, b, m)
}

/// The zones of `new` (copper pours, by index) that an edit from `old` to `new` may have
/// changed the pour of.
pub fn affected_zones(old: &Board, new: &Board, rules: &Rules) -> Vec<usize> {
    let (a, b) = (items(old), items(new));
    let before: HashSet<&str> = a.iter().map(|(k, _)| k.as_str()).collect();
    let after: HashSet<&str> = b.iter().map(|(k, _)| k.as_str()).collect();
    let mut changed: Vec<Spot> = Vec::new();
    changed.extend(a.iter().filter(|(k, _)| !after.contains(k.as_str())).map(|(_, s)| s.clone()));
    changed.extend(b.iter().filter(|(k, _)| !before.contains(k.as_str())).map(|(_, s)| s.clone()));
    // Zones that are new, gone or different count as changed copper over their outlines.
    let old_keys: HashSet<String> = old.zones.iter().map(|z| zone_key(old, z)).collect();
    let new_keys: HashSet<String> = new.zones.iter().map(|z| zone_key(new, z)).collect();
    let mut own: HashSet<usize> = HashSet::new();
    for (i, z) in new.zones.iter().enumerate() {
        if !old_keys.contains(&zone_key(new, z)) {
            own.insert(i);
            if !z.keepout || z.settings.no_pour {
                changed.push((box_of(z), None));
            }
        }
    }
    for z in old.zones.iter().filter(|z| !new_keys.contains(&zone_key(old, z))) {
        changed.push((box_of(z), None));
    }
    if changed.is_empty() {
        return Vec::new();
    }
    let pours: Vec<usize> = (0..new.zones.len())
        .filter(|&i| new.zones.get(i).is_some_and(|z| !z.keepout && z.outlines().next().is_some()))
        .collect();
    let mut hit: HashSet<usize> = HashSet::new();
    for &i in &pours {
        let Some(z) = new.zones.get(i) else { continue };
        let zb = box_of(z);
        let reach = z.settings.clearance.max(rules.clearance) + z.settings.thermal_gap + 1.0;
        let touched = changed
            .iter()
            .any(|(bx, layer)| overlap(bx, &zb, reach) && layer.as_ref().is_none_or(|l| new.on_layer(&z.layers, l)));
        if touched {
            hit.insert(i);
        }
    }
    // A zone the edit just filled is done.
    let done = |i: usize| {
        own.contains(&i)
            && new.zones.get(i).is_some_and(|z| {
                !z.fills.is_empty()
                    && !old.zones.iter().any(|o| zone_key(old, o) == zone_key(new, z) && o.fills == z.fills)
            })
    };
    hit.retain(|&i| !done(i));
    // Zones overlapping a poured one depend on it (priorities).
    let first: Vec<usize> = hit.iter().copied().collect();
    for &i in &pours {
        if hit.contains(&i) || done(i) {
            continue;
        }
        let Some(z) = new.zones.get(i) else { continue };
        let zb = box_of(z);
        if first.iter().filter_map(|&j| new.zones.get(j)).any(|o| overlap(&box_of(o), &zb, 0.0)) {
            hit.insert(i);
        }
    }
    let mut v: Vec<usize> = hit.into_iter().collect();
    v.sort_unstable();
    v
}

/// Pours the zones an edit from `old` to `new` affected, writing the fills into `new`.
/// Returns the zones poured, and the fills' notes.
///
/// # Errors
/// A failed re-read of the board.
pub fn refill_after_edit(
    old: &Board,
    new: &mut Board,
    rules: &Rules,
    limits: &Limits,
) -> Result<(Vec<usize>, zones::Filled), Error> {
    let zs = affected_zones(old, new, rules);
    if zs.is_empty() {
        return Ok((zs, zones::Filled::default()));
    }
    let filled = zones::fill(new, rules, Some(&zs));
    let fills: Vec<Fill> = filled.fills.clone();
    new.set_zone_fills(&fills, limits)?;
    Ok((zs, filled))
}

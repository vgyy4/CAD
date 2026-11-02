//! Picking sketch regions in the viewport: while a form that takes `regions` (extrude,
//! revolve, ...) is open, a click inside a closed region of its sketch toggles that region.
//! A region's outer loop is rebuilt from its entities' polylines (chained end to end); the
//! smallest region whose loop contains the click wins, so a hole's own region is picked
//! inside the hole.

use gusset_eval::SketchResult;

/// The 2D polyline of a sketch entity (sketch coordinates).
#[must_use]
pub fn entity_polyline(s: &SketchResult, id: &str) -> Option<Vec<[f64; 2]>> {
    let pt = |p: &str| s.points.get(p).copied();
    if let Some((_, a, b)) = s.lines.iter().find(|(l, ..)| l == id) {
        return Some(vec![pt(a)?, pt(b)?]);
    }
    let arc = |c: [f64; 2], r: f64, a0: f64, a1: f64| -> Vec<[f64; 2]> {
        (0..=48)
            .map(|i| {
                let t = a0 + (a1 - a0) * f64::from(i) / 48.0;
                [c[0] + r * t.cos(), c[1] + r * t.sin()]
            })
            .collect()
    };
    if let Some((_, c)) = s.circles.iter().find(|(e, _)| e == id) {
        return Some(arc(pt(c)?, *s.radii.get(id)?, 0.0, std::f64::consts::TAU));
    }
    if let Some((_, c, st, en)) = s.arcs.iter().find(|(e, ..)| e == id) {
        let (c, st, en) = (pt(c)?, pt(st)?, pt(en)?);
        let a0 = (st[1] - c[1]).atan2(st[0] - c[0]);
        let mut a1 = (en[1] - c[1]).atan2(en[0] - c[0]);
        if a1 <= a0 {
            a1 += std::f64::consts::TAU;
        }
        return Some(arc(c, (st[0] - c[0]).hypot(st[1] - c[1]), a0, a1));
    }
    s.splines.iter().find(|(e, _)| e == id).map(|(_, p)| p.clone())
}

/// Chains polylines end to end into one loop (each used once, reversed when needed).
#[must_use]
pub fn chain(mut pieces: Vec<Vec<[f64; 2]>>) -> Vec<[f64; 2]> {
    let d = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]);
    let Some(mut out) = pieces.pop() else { return Vec::new() };
    while !pieces.is_empty() {
        let Some(&end) = out.last() else { break };
        let best = pieces
            .iter()
            .enumerate()
            .flat_map(|(i, p)| {
                let first = p.first().map_or(f64::INFINITY, |q| d(*q, end));
                let last = p.last().map_or(f64::INFINITY, |q| d(*q, end));
                [(first, i, false), (last, i, true)]
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        let Some((_, i, reverse)) = best else { break };
        let mut p = pieces.swap_remove(i);
        if reverse {
            p.reverse();
        }
        out.extend(p.into_iter().skip(1));
    }
    out
}

/// Whether `p` is inside the closed polygon (even-odd rule).
#[must_use]
pub fn contains(poly: &[[f64; 2]], p: [f64; 2]) -> bool {
    let mut inside = false;
    for (i, a) in poly.iter().enumerate() {
        let Some(b) = poly.get((i + 1) % poly.len()) else { continue };
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            if p[0] < x {
                inside = !inside;
            }
        }
    }
    inside
}

/// Each region's outer loop (sketch coordinates).
#[must_use]
pub fn outlines(s: &SketchResult) -> Vec<(String, Vec<[f64; 2]>)> {
    let Ok(regions) = &s.regions else { return Vec::new() };
    regions
        .iter()
        .map(|r| (r.id.clone(), chain(r.outer.iter().filter_map(|e| entity_polyline(s, e)).collect())))
        .collect()
}

/// The smallest region whose outer loop contains `p`.
#[must_use]
pub fn region_at(s: &SketchResult, p: [f64; 2]) -> Option<String> {
    let Ok(regions) = &s.regions else { return None };
    let outlines = outlines(s);
    regions
        .iter()
        .filter(|r| outlines.iter().any(|(id, poly)| *id == r.id && contains(poly, p)))
        .min_by(|a, b| a.area.total_cmp(&b.area))
        .map(|r| r.id.clone())
}

/// Toggles `id` in a comma-separated list.
#[must_use]
pub fn toggle(list: &str, id: &str) -> String {
    let mut items: Vec<&str> = list.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
    match items.iter().position(|s| *s == id) {
        Some(i) => {
            items.remove(i);
        }
        None => items.push(id),
    }
    items.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chains_pieces_in_any_direction_and_tests_containment() {
        let sq = chain(vec![
            vec![[0.0, 0.0], [10.0, 0.0]],
            vec![[10.0, 10.0], [10.0, 0.0]],
            vec![[0.0, 10.0], [10.0, 10.0]],
            vec![[0.0, 0.0], [0.0, 10.0]],
        ]);
        assert_eq!(sq.len(), 5);
        assert!(contains(&sq, [5.0, 5.0]));
        assert!(!contains(&sq, [15.0, 5.0]));
        assert!(!contains(&sq, [5.0, -1.0]));
    }

    #[test]
    fn toggling_adds_and_removes() {
        assert_eq!(toggle("", "R1"), "R1");
        assert_eq!(toggle("R1", "R2"), "R1, R2");
        assert_eq!(toggle("R1, R2", "R1"), "R2");
    }
}

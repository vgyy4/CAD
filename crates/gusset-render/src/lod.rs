//! A coarse copy of a mesh for bodies drawn small (spec: level of detail for large
//! assemblies). Vertex clustering: vertices snap to a grid of cells about a twentieth of the
//! body's diagonal, each cell becomes one vertex at the mean of its vertices, triangles that
//! collapse go, and normals are rebuilt smooth. Only for meshes with many triangles, and only
//! when it at least halves them.

use gusset_kernel::Mesh;
use std::collections::HashMap;

/// Meshes with fewer triangles are drawn as they are.
const MIN_TRIANGLES: usize = 2000;
/// Cells along the bounding box diagonal.
const CELLS: f64 = 20.0;

/// The coarse copy of `m`, or `None` when it would not be much smaller.
#[must_use]
pub(crate) fn coarse(m: &Mesh) -> Option<Mesh> {
    let tris = m.indices.len() / 3;
    if tris < MIN_TRIANGLES || m.positions.is_empty() {
        return None;
    }
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for p in &m.positions {
        for ((l, h), v) in lo.iter_mut().zip(hi.iter_mut()).zip(p) {
            *l = l.min(f64::from(*v));
            *h = h.max(f64::from(*v));
        }
    }
    let diag = lo.iter().zip(&hi).map(|(l, h)| (h - l) * (h - l)).sum::<f64>().sqrt();
    let cell = diag / CELLS;
    if !(cell.is_finite() && cell > 0.0) {
        return None;
    }
    let mut cells: HashMap<[i64; 3], u32> = HashMap::new();
    let mut sums: Vec<([f64; 3], f64)> = Vec::new();
    let remap: Vec<u32> = m
        .positions
        .iter()
        .map(|p| {
            let key = [0, 1, 2].map(|k| {
                let (v, l) = (p.get(k).copied().map_or(0.0, f64::from), lo.get(k).copied().unwrap_or(0.0));
                ((v - l) / cell).floor() as i64
            });
            let id = *cells.entry(key).or_insert_with(|| {
                sums.push(([0.0; 3], 0.0));
                (sums.len() - 1) as u32
            });
            if let Some((s, n)) = sums.get_mut(id as usize) {
                for (a, v) in s.iter_mut().zip(p) {
                    *a += f64::from(*v);
                }
                *n += 1.0;
            }
            id
        })
        .collect();
    let positions: Vec<[f32; 3]> = sums.iter().map(|(s, n)| s.map(|v| (v / n.max(1.0)) as f32)).collect();
    let mut indices = Vec::new();
    for t in m.indices.chunks_exact(3) {
        let [a, b, c] = [0, 1, 2].map(|k| t.get(k).and_then(|i| remap.get(*i as usize)).copied().unwrap_or(0));
        if a != b && b != c && a != c {
            indices.extend([a, b, c]);
        }
    }
    if indices.is_empty() || indices.len() / 3 * 2 > tris {
        return None;
    }
    // Smooth normals: area-weighted face normals summed at each vertex.
    let mut normals = vec![[0.0f32; 3]; positions.len()];
    for t in indices.chunks_exact(3) {
        let p = |k: usize| t.get(k).and_then(|i| positions.get(*i as usize)).copied().unwrap_or([0.0; 3]);
        let (a, b, c) = (p(0), p(1), p(2));
        let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        for i in t {
            if let Some(acc) = normals.get_mut(*i as usize) {
                for (x, y) in acc.iter_mut().zip(n) {
                    *x += y;
                }
            }
        }
    }
    for n in &mut normals {
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        *n = if l > 1e-20 { n.map(|x| x / l) } else { [0.0, 0.0, 1.0] };
    }
    let count = (indices.len() / 3) as u32;
    // Edges too: a body drawn small does not need a thousand segments per circle (they cost
    // more than its faces on a software GPU). Each polyline keeps its ends and the points at
    // least half a cell from the last one kept.
    let edges = m
        .edges
        .iter()
        .map(|line| {
            let mut out: Vec<[f32; 3]> = Vec::new();
            for (i, p) in line.iter().enumerate() {
                let last = i + 1 == line.len();
                let far = out.last().is_none_or(|q| {
                    let d2: f64 = q.iter().zip(p).map(|(a, b)| f64::from(a - b) * f64::from(a - b)).sum();
                    d2 >= (cell * 0.5) * (cell * 0.5)
                });
                if far || last {
                    out.push(*p);
                }
            }
            out
        })
        .filter(|l| l.len() >= 2)
        .collect();
    Some(Mesh { positions, normals, indices, face_ranges: std::iter::once(0..count).collect(), edges })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo;

    #[test]
    fn a_dense_cylinder_becomes_much_coarser_and_a_box_stays() {
        let fine = demo::cylinder_mesh(3.0, 8.0, 1000);
        let c = coarse(&fine).expect("coarse copy");
        assert!(c.indices.len() * 4 < fine.indices.len(), "{} vs {}", c.indices.len(), fine.indices.len());
        assert!(coarse(&demo::box_mesh([1.0; 3])).is_none());
        // Edges are thinned too, keeping every polyline's ends.
        let (n_fine, n_coarse) =
            (fine.edges.iter().map(Vec::len).sum::<usize>(), c.edges.iter().map(Vec::len).sum::<usize>());
        assert!(!c.edges.is_empty() && n_coarse * 10 < n_fine, "{n_coarse} vs {n_fine}");
        for (f, k) in fine.edges.iter().zip(&c.edges) {
            assert_eq!((f.first(), f.last()), (k.first(), k.last()));
        }
    }
}

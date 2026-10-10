//! Small procedural meshes for tests, the GPU self-test and examples. They have the
//! same structure as kernel output: per-face triangle ranges and edge polylines.

use gusset_kernel::Mesh;

/// An axis-aligned box centered on the origin. Faces in order: bottom (-Z), top (+Z),
/// front (-Y), back (+Y), left (-X), right (+X). Twelve edges.
#[must_use]
pub fn box_mesh(size: [f32; 3]) -> Mesh {
    let h = size.map(|s| s * 0.5);
    let c = |x: f32, y: f32, z: f32| [x * h[0], y * h[1], z * h[2]];
    // (normal, four corners counter-clockwise seen from outside)
    let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
        ([0.0, 0.0, -1.0], [c(-1., -1., -1.), c(-1., 1., -1.), c(1., 1., -1.), c(1., -1., -1.)]),
        ([0.0, 0.0, 1.0], [c(-1., -1., 1.), c(1., -1., 1.), c(1., 1., 1.), c(-1., 1., 1.)]),
        ([0.0, -1.0, 0.0], [c(-1., -1., -1.), c(1., -1., -1.), c(1., -1., 1.), c(-1., -1., 1.)]),
        ([0.0, 1.0, 0.0], [c(1., 1., -1.), c(-1., 1., -1.), c(-1., 1., 1.), c(1., 1., 1.)]),
        ([-1.0, 0.0, 0.0], [c(-1., 1., -1.), c(-1., -1., -1.), c(-1., -1., 1.), c(-1., 1., 1.)]),
        ([1.0, 0.0, 0.0], [c(1., -1., -1.), c(1., 1., -1.), c(1., 1., 1.), c(1., -1., 1.)]),
    ];
    let mut m = Mesh::default();
    for (i, (n, q)) in faces.iter().enumerate() {
        let base = m.positions.len() as u32;
        m.positions.extend_from_slice(q);
        m.normals.extend_from_slice(&[*n; 4]);
        m.indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        let t = i as u32 * 2;
        m.face_ranges.push(t..t + 2);
    }
    for z in [-1.0, 1.0] {
        let ring = [c(-1., -1., z), c(1., -1., z), c(1., 1., z), c(-1., 1., z)];
        for (a, b) in ring.iter().zip(ring.iter().cycle().skip(1)) {
            m.edges.push(vec![*a, *b]);
        }
    }
    for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        m.edges.push(vec![c(x, y, -1.0), c(x, y, 1.0)]);
    }
    m
}

/// A cylinder around +Z from `z = 0` to `z = height` with smooth side normals. Faces:
/// side, bottom cap, top cap. Edges: bottom and top circles.
#[must_use]
pub fn cylinder_mesh(radius: f32, height: f32, segments: u32) -> Mesh {
    let n = segments.max(3);
    let mut m = Mesh::default();
    let ring: Vec<(f32, f32)> = (0..n)
        .map(|i| {
            let a = i as f32 / n as f32 * std::f32::consts::TAU;
            (a.cos(), a.sin())
        })
        .collect();
    // Side.
    for &(cx, cy) in &ring {
        m.positions.push([cx * radius, cy * radius, 0.0]);
        m.normals.push([cx, cy, 0.0]);
        m.positions.push([cx * radius, cy * radius, height]);
        m.normals.push([cx, cy, 0.0]);
    }
    for i in 0..n {
        let j = (i + 1) % n;
        let (a0, a1, b0, b1) = (2 * i, 2 * i + 1, 2 * j, 2 * j + 1);
        m.indices.extend([a0, b0, b1, a0, b1, a1]);
    }
    m.face_ranges.push(0..2 * n);
    // Caps (fans).
    for (z, nz) in [(0.0, -1.0_f32), (height, 1.0)] {
        let start = (m.indices.len() / 3) as u32;
        let center = m.positions.len() as u32;
        m.positions.push([0.0, 0.0, z]);
        m.normals.push([0.0, 0.0, nz]);
        for &(cx, cy) in &ring {
            m.positions.push([cx * radius, cy * radius, z]);
            m.normals.push([0.0, 0.0, nz]);
        }
        for i in 0..n {
            let a = center + 1 + i;
            let b = center + 1 + (i + 1) % n;
            if nz > 0.0 {
                m.indices.extend([center, a, b]);
            } else {
                m.indices.extend([center, b, a]);
            }
        }
        m.face_ranges.push(start..(m.indices.len() / 3) as u32);
        let mut circle: Vec<[f32; 3]> = ring.iter().map(|&(cx, cy)| [cx * radius, cy * radius, z]).collect();
        if let Some(&first) = circle.first() {
            circle.push(first);
        }
        m.edges.push(circle);
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_are_consistent() {
        let b = box_mesh([1.0, 2.0, 3.0]);
        assert_eq!(b.triangle_count(), 12);
        assert_eq!(b.face_ranges.len(), 6);
        assert_eq!(b.edges.len(), 12);
        let c = cylinder_mesh(1.0, 2.0, 16);
        assert_eq!(c.face_ranges.len(), 3);
        assert_eq!(c.face_ranges.last().map(|r| r.end), Some(c.triangle_count() as u32));
        assert_eq!(c.edges.len(), 2);
        assert_eq!(c.edges[0].len(), 17);
    }
}

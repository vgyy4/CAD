//! A generated SAT file for tests elsewhere (the OCCT import test).
#![allow(clippy::indexing_slicing)]

/// A SAT text of an axis-aligned box `w x d x h` with one corner at the origin, in the
/// version 6 or 7 layout (records built the way the sample files are).
pub fn box_sat(version: u32, size: [f64; 3]) -> String {
    let [w, d, h] = size;
    let v7 = version >= 700;
    let corners: Vec<[f64; 3]> = (0..8)
        .map(|i| [if i & 1 > 0 { w } else { 0.0 }, if i & 2 > 0 { d } else { 0.0 }, if i & 4 > 0 { h } else { 0.0 }])
        .collect();
    // Faces as corner loops, counter-clockwise seen from outside, with their outward normal.
    let faces: [([usize; 4], [f64; 3]); 6] = [
        ([0, 2, 3, 1], [0.0, 0.0, -1.0]),
        ([4, 5, 7, 6], [0.0, 0.0, 1.0]),
        ([0, 1, 5, 4], [0.0, -1.0, 0.0]),
        ([2, 6, 7, 3], [0.0, 1.0, 0.0]),
        ([0, 4, 6, 2], [-1.0, 0.0, 0.0]),
        ([1, 3, 7, 5], [1.0, 0.0, 0.0]),
    ];
    // Record numbers: body 0, lump 1, shell 2, faces 3.., loops, surfaces, coedges,
    // edges, curves, vertices, points.
    let (body, lump, shell) = (0usize, 1usize, 2usize);
    let face0 = 3;
    let loop0 = face0 + 6;
    let surf0 = loop0 + 6;
    let co0 = surf0 + 6;
    let mut edges: Vec<(usize, usize)> = Vec::new();
    let mut co_edge = Vec::new();
    for (f, _) in &faces {
        for k in 0..4 {
            let (a, b) = (f[k], f[(k + 1) % 4]);
            let (lo, hi) = (a.min(b), a.max(b));
            let e = match edges.iter().position(|x| *x == (lo, hi)) {
                Some(e) => e,
                None => {
                    edges.push((lo, hi));
                    edges.len() - 1
                }
            };
            co_edge.push((e, a == lo));
        }
    }
    let edge0 = co0 + 24;
    let curve0 = edge0 + edges.len();
    let vert0 = curve0 + edges.len();
    let point0 = vert0 + 8;
    let mut recs: Vec<String> = Vec::new();
    let p = |i: usize| format!("${i}");
    let x = if v7 { " -1 $-1" } else { "" };
    let num = |v: f64| format!("{v}");
    recs.push(format!("body $-1{x} {} $-1 $-1 #", p(lump)));
    recs.push(format!("lump $-1{x} $-1 {} {} #", p(shell), p(body)));
    recs.push(format!("shell $-1{x} $-1 $-1 {} $-1 {} #", p(face0), p(lump)));
    for i in 0..6 {
        let next = if i < 5 { p(face0 + i + 1) } else { "$-1".into() };
        recs.push(format!("face $-1{x} {next} {} {} $-1 {} forward single #", p(loop0 + i), p(shell), p(surf0 + i)));
    }
    for i in 0..6 {
        recs.push(format!("loop $-1{x} $-1 {} {} #", p(co0 + 4 * i), p(face0 + i)));
    }
    for (f, n) in &faces {
        let c = corners[f[0]];
        let u = [corners[f[1]][0] - c[0], corners[f[1]][1] - c[1], corners[f[1]][2] - c[2]];
        let l = (u[0] * u[0] + u[1] * u[1] + u[2] * u[2]).sqrt();
        recs.push(format!(
            "plane-surface $-1{x} {} {} {} {} {} {} {} {} {} forward_v I I I I #",
            num(c[0]),
            num(c[1]),
            num(c[2]),
            num(n[0]),
            num(n[1]),
            num(n[2]),
            num(u[0] / l),
            num(u[1] / l),
            num(u[2] / l)
        ));
    }
    for (i, (e, fwd)) in co_edge.iter().enumerate() {
        let (fi, k) = (i / 4, i % 4);
        let next = co0 + 4 * fi + (k + 1) % 4;
        let prev = co0 + 4 * fi + (k + 3) % 4;
        let partner = co_edge.iter().enumerate().find(|(j, (e2, _))| *j != i && e2 == e).map_or(i, |(j, _)| j);
        let sense = if *fwd { "forward" } else { "reversed" };
        recs.push(format!(
            "coedge $-1{x} {} {} {} {} {sense} {} $-1 #",
            p(next),
            p(prev),
            p(co0 + partner),
            p(edge0 + e),
            p(loop0 + fi)
        ));
    }
    for (i, (a, b)) in edges.iter().enumerate() {
        let first = co_edge.iter().position(|(e, _)| *e == i).unwrap_or(0);
        let l = {
            let (pa, pb) = (corners[*a], corners[*b]);
            ((pb[0] - pa[0]).powi(2) + (pb[1] - pa[1]).powi(2) + (pb[2] - pa[2]).powi(2)).sqrt()
        };
        let conv = if v7 { "@7 unknown" } else { "7 unknown" };
        recs.push(format!(
            "edge $-1{x} {} 0 {} {} {} {} forward {conv} #",
            p(vert0 + a),
            p(vert0 + b),
            num(l),
            p(co0 + first),
            p(curve0 + i)
        ));
    }
    for (a, b) in &edges {
        let (pa, pb) = (corners[*a], corners[*b]);
        let d = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        recs.push(format!(
            "straight-curve $-1{x} {} {} {} {} {} {} I I #",
            num(pa[0]),
            num(pa[1]),
            num(pa[2]),
            num(d[0] / l),
            num(d[1] / l),
            num(d[2] / l)
        ));
    }
    for v in 0..8 {
        let e = edges.iter().position(|(a, b)| *a == v || *b == v).unwrap_or(0);
        recs.push(format!("vertex $-1{x} {} {} #", p(edge0 + e), p(point0 + v)));
    }
    for c in &corners {
        recs.push(format!("point $-1{x} {} {} {} #", num(c[0]), num(c[1]), num(c[2])));
    }
    let product = if v7 { "@4 test @9 ACIS 7.0 @4 date" } else { "4 test 9 ACIS 6.0 4 date" };
    format!("{version} 0 1 0\n{product}\n1 1e-06 1e-10\n{}\nEnd-of-ACIS-data\n", recs.join("\n"))
}

/// A SAT text (version 7 layout) of 3722's globe base as `Revolve2` makes it: a cylinder of
/// radius 33.5 and height 19 under a dome, the arc of radius 54 about (0.658311, `zc`) that
/// meets the cylinder's top edge and the axis. The dome is a face of a spindle torus (major
/// radius less than minor) bounded by the top circle and a loop that is the point on the axis.
/// Returns the text and the volume by Pappus.
pub fn spindle_dome_sat() -> (String, f64) {
    let (r, h, big, small) = (33.5f64, 19.0f64, 0.658311f64, 54.0f64);
    let zc = h + (small * small - (r - big) * (r - big)).sqrt();
    let apex = zc + (small * small - big * big).sqrt();
    // Pappus: 2 pi times the first moment of the half section about the axis, in closed form.
    // The section is the rectangle under the rim and, above it, the disc of the arc from the
    // axis out to x = big + s (s = sqrt(small^2 - t^2), t = z - zc), less the notch above the
    // apex where the disc starts at x = big - s: the integrals of x^2 / 2 over t.
    let int = |t: f64, sign: f64| {
        let s = (small * small - t * t).max(0.0).sqrt();
        let asin = libm::asin((t / small).clamp(-1.0, 1.0));
        (big * big * t + sign * big * (t * s + small * small * asin) + small * small * t - t * t * t / 3.0) / 2.0
    };
    let moment = r * r / 2.0 * h + (int(small, 1.0) - int(h - zc, 1.0)) - (int(small, -1.0) - int(apex - zc, -1.0));
    let volume = 2.0 * std::f64::consts::PI * moment;
    let x = " -1 $-1";
    let tau = 2.0 * std::f64::consts::PI;
    let recs = [
        format!("body $-1{x} $1 $-1 $-1 #"),
        format!("lump $-1{x} $-1 $2 $0 #"),
        format!("shell $-1{x} $-1 $-1 $3 $-1 $1 #"),
        // Faces 3 (bottom), 4 (side), 5 (dome).
        format!("face $-1{x} $4 $6 $2 $-1 $11 forward single #"),
        format!("face $-1{x} $5 $7 $2 $-1 $12 forward single #"),
        format!("face $-1{x} $-1 $9 $2 $-1 $13 forward single #"),
        // Loops 6 (bottom), 7 and 8 (side), 9 and 10 (dome: the top circle and the apex).
        format!("loop $-1{x} $-1 $14 $3 #"),
        format!("loop $-1{x} $8 $15 $4 #"),
        format!("loop $-1{x} $-1 $16 $4 #"),
        format!("loop $-1{x} $10 $17 $5 #"),
        format!("loop $-1{x} $-1 $18 $5 #"),
        // Surfaces 11 (plane), 12 (cylinder), 13 (torus).
        format!("plane-surface $-1{x} 0 0 0 0 0 -1 1 0 0 forward_v I I I I #"),
        format!("cone-surface $-1{x} 0 0 0 0 0 1 {r} 0 0 1 I I 0 1 {r} forward I I I I #"),
        format!("torus-surface $-1{x} 0 0 {zc} 0 0 1 {big} {small} 1 0 0 forward_v I I I I #"),
        // Coedges 14 to 18.
        format!("coedge $-1{x} $14 $14 $15 $19 reversed $6 $-1 #"),
        format!("coedge $-1{x} $15 $15 $14 $19 forward $7 $-1 #"),
        format!("coedge $-1{x} $16 $16 $17 $20 reversed $8 $-1 #"),
        format!("coedge $-1{x} $17 $17 $16 $20 forward $9 $-1 #"),
        format!("coedge $-1{x} $18 $18 $18 $21 forward $10 $-1 #"),
        // Edges 19 (bottom circle), 20 (top circle), 21 (the apex: no curve).
        format!("edge $-1{x} $24 0 $24 {tau} $14 $22 forward @7 unknown #"),
        format!("edge $-1{x} $25 0 $25 {tau} $16 $23 forward @7 unknown #"),
        format!("edge $-1{x} $26 0 $26 0 $18 $-1 forward @7 unknown #"),
        format!("ellipse-curve $-1{x} 0 0 0 0 0 1 {r} 0 0 1 I I #"),
        format!("ellipse-curve $-1{x} 0 0 {h} 0 0 1 {r} 0 0 1 I I #"),
        format!("vertex $-1{x} $19 $27 #"),
        format!("vertex $-1{x} $20 $28 #"),
        format!("vertex $-1{x} $21 $29 #"),
        format!("point $-1{x} {r} 0 0 #"),
        format!("point $-1{x} {r} 0 {h} #"),
        format!("point $-1{x} 0 0 {apex} #"),
    ];
    let text =
        format!("700 0 1 0\n@4 test @9 ACIS 7.0 @4 date\n1 1e-06 1e-10\n{}\nEnd-of-ACIS-data\n", recs.join("\n"));
    (text, volume)
}

/// An elliptical cylinder (semi-axes 2 and 1 mm along x and y, 3 mm high from z = 0) as ACIS
/// writes it: a cone surface of ratio 0.5 without slope between two planes (6126's logo petals),
/// and its volume (pi a b h).
pub fn elliptic_cylinder_sat() -> (String, f64) {
    let (a, ratio, h) = (2.0f64, 0.5f64, 3.0f64);
    let x = " -1 $-1";
    let tau = 2.0 * std::f64::consts::PI;
    let recs = [
        format!("body $-1{x} $1 $-1 $-1 #"),
        format!("lump $-1{x} $-1 $2 $0 #"),
        format!("shell $-1{x} $-1 $-1 $3 $-1 $1 #"),
        // Faces 3 (bottom), 4 (side), 5 (top).
        format!("face $-1{x} $4 $6 $2 $-1 $10 forward single #"),
        format!("face $-1{x} $5 $7 $2 $-1 $11 forward single #"),
        format!("face $-1{x} $-1 $9 $2 $-1 $12 forward single #"),
        // Loops 6 (bottom), 7 and 8 (side), 9 (top).
        format!("loop $-1{x} $-1 $13 $3 #"),
        format!("loop $-1{x} $8 $14 $4 #"),
        format!("loop $-1{x} $-1 $15 $4 #"),
        format!("loop $-1{x} $-1 $16 $5 #"),
        // Surfaces 10 (bottom plane), 11 (the elliptical cylinder), 12 (top plane).
        format!("plane-surface $-1{x} 0 0 0 0 0 -1 1 0 0 forward_v I I I I #"),
        format!("cone-surface $-1{x} 0 0 0 0 0 1 {a} 0 0 {ratio} I I 0 1 {a} forward I I I I #"),
        format!("plane-surface $-1{x} 0 0 {h} 0 0 1 1 0 0 forward_v I I I I #"),
        // Coedges 13 to 16.
        format!("coedge $-1{x} $13 $13 $14 $17 reversed $6 $-1 #"),
        format!("coedge $-1{x} $14 $14 $13 $17 forward $7 $-1 #"),
        format!("coedge $-1{x} $15 $15 $16 $18 reversed $8 $-1 #"),
        format!("coedge $-1{x} $16 $16 $15 $18 forward $9 $-1 #"),
        // Edges 17 (bottom ellipse), 18 (top ellipse).
        format!("edge $-1{x} $21 0 $21 {tau} $13 $19 forward @7 unknown #"),
        format!("edge $-1{x} $22 0 $22 {tau} $15 $20 forward @7 unknown #"),
        format!("ellipse-curve $-1{x} 0 0 0 0 0 1 {a} 0 0 {ratio} I I #"),
        format!("ellipse-curve $-1{x} 0 0 {h} 0 0 1 {a} 0 0 {ratio} I I #"),
        format!("vertex $-1{x} $17 $23 #"),
        format!("vertex $-1{x} $18 $24 #"),
        format!("point $-1{x} {a} 0 0 #"),
        format!("point $-1{x} {a} 0 {h} #"),
    ];
    let text =
        format!("700 0 1 0\n@4 test @9 ACIS 7.0 @4 date\n1 1e-06 1e-10\n{}\nEnd-of-ACIS-data\n", recs.join("\n"));
    (text, std::f64::consts::PI * a * a * ratio * h)
}

/// The binary (SAB) form of a version 7 text file, as Fusion writes it: for tests.
pub fn to_binary(text: &str) -> Vec<u8> {
    let mut out = b"ASM BinaryFile8".to_vec();
    for v in [22500i64, 0, 1, 0] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    let s = |out: &mut Vec<u8>, tag: u8, v: &str| {
        out.push(tag);
        out.push(u8::try_from(v.len()).unwrap_or(255));
        out.extend_from_slice(v.as_bytes().get(..v.len().min(255)).unwrap_or_default());
    };
    s(&mut out, 0x07, "gusset test");
    s(&mut out, 0x07, "ASM 225");
    s(&mut out, 0x07, "today");
    for d in [1.0f64, 1e-6, 1e-10] {
        out.push(0x06);
        out.extend_from_slice(&d.to_le_bytes());
    }
    let body = text.lines().skip(3).collect::<Vec<_>>().join("\n");
    for rec in body.split(" #") {
        let mut words = rec.split_whitespace();
        let Some(ty) = words.next() else { continue };
        if ty.starts_with("End-of") {
            break;
        }
        let parts: Vec<&str> = ty.split('-').collect();
        for (i, p) in parts.iter().enumerate() {
            s(&mut out, if i + 1 == parts.len() { 0x0d } else { 0x0e }, p);
        }
        let mut it = words.peekable();
        while let Some(w) = it.next() {
            if let Some(p) = w.strip_prefix('$').and_then(|p| p.parse::<i64>().ok()) {
                out.push(0x0c);
                out.extend_from_slice(&p.to_le_bytes());
            } else if let Some(n) = w.strip_prefix('@').and_then(|n| n.parse::<usize>().ok()) {
                let t: String = it.next().unwrap_or("").chars().take(n).collect();
                s(&mut out, 0x07, &t);
            } else if let Ok(i) = w.parse::<i64>() {
                out.push(0x04);
                out.extend_from_slice(&i.to_le_bytes());
            } else if let Ok(d) = w.parse::<f64>() {
                out.push(0x06);
                out.extend_from_slice(&d.to_le_bytes());
            } else {
                match w {
                    "reversed" => out.push(0x0a),
                    "forward" | "forward_v" | "I" | "single" => out.push(0x0b),
                    other => s(&mut out, 0x0d, other),
                }
            }
        }
        out.push(0x11);
    }
    s(&mut out, 0x0d, "End-of-ASM-data");
    out.push(0x11);
    out
}

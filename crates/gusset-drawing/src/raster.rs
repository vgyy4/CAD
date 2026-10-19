//! A raster image of a [`Scene`] (ADR 0032 phase 3: what agents look at). Every
//! primitive becomes filled polygons in pixels (strokes as quads with square caps,
//! dashes cut along the path, text from `gusset-text` outlines), filled with the
//! nonzero rule, anti-aliased with exact horizontal coverage and 4 sub-rows per pixel.

use crate::scene::{Anchor, Prim, Scene, Style};
use std::f64::consts::TAU;

/// Largest image side in pixels.
pub const MAX_SIDE: u32 = 4096;
const SUBROWS: usize = 4;

/// RGBA8 pixels, rows top to bottom.
#[derive(Clone, PartialEq, Eq)]
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl std::fmt::Debug for Raster {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Raster({}x{})", self.width, self.height)
    }
}

impl Raster {
    /// PNG (8-bit RGBA, sRGB).
    ///
    /// # Errors
    /// The encoder failed.
    pub fn to_png(&self) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, self.width, self.height);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
            enc.set_compression(png::Compression::High);
            let mut w = enc.write_header().map_err(|e| e.to_string())?;
            w.write_image_data(&self.rgba).map_err(|e| e.to_string())?;
            w.finish().map_err(|e| e.to_string())?;
        }
        Ok(out)
    }
}

type P = [f64; 2];

/// Most dashes cut per path.
const MAX_DASHES: usize = 100_000;

/// The part (parameters 0..=1) of segment `a`-`b` inside the box (Liang-Barsky).
fn clip_segment(a: P, b: P, (lo, hi): (P, P)) -> Option<(f64, f64)> {
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    let d = [b[0] - a[0], b[1] - a[1]];
    for k in 0..2 {
        let (p, q0, q1) =
            (d.get(k).copied()?, a.get(k).copied()? - lo.get(k).copied()?, hi.get(k).copied()? - a.get(k).copied()?);
        for (p, q) in [(-p, q0), (p, q1)] {
            if p.abs() < 1e-300 {
                if q < 0.0 {
                    return None;
                }
            } else {
                let r = q / p;
                if p < 0.0 {
                    t0 = t0.max(r);
                } else {
                    t1 = t1.min(r);
                }
            }
        }
    }
    (t0 <= t1).then_some((t0, t1))
}

/// Polygons of one colour, as directed edges.
#[derive(Default)]
struct Layer {
    edges: Vec<(P, P)>,
}

impl Layer {
    fn polygon(&mut self, pts: &[P]) {
        if pts.len() < 3 {
            return;
        }
        // Counterclockwise in pixel space (any consistent orientation unions with
        // the nonzero rule).
        let area: f64 = pts.iter().zip(pts.iter().cycle().skip(1)).map(|(a, b)| a[0] * b[1] - b[0] * a[1]).sum();
        let flip = area < 0.0;
        for i in 0..pts.len() {
            let (Some(&a), Some(&b)) = (pts.get(i), pts.get((i + 1) % pts.len())) else { continue };
            self.edges.push(if flip { (b, a) } else { (a, b) });
        }
    }

    /// Contours of one glyph region set (orientations kept).
    fn contours(&mut self, cs: &[Vec<P>]) {
        for pts in cs {
            for i in 0..pts.len() {
                let (Some(&a), Some(&b)) = (pts.get(i), pts.get((i + 1) % pts.len())) else { continue };
                self.edges.push((a, b));
            }
        }
    }

    /// A stroke of width `w` from `a` to `b` with square caps.
    fn segment(&mut self, a: P, b: P, w: f64) {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l = libm::sqrt(dx * dx + dy * dy);
        let h = w / 2.0;
        let (ux, uy) = if l > 1e-12 { (dx / l, dy / l) } else { (1.0, 0.0) };
        let (nx, ny) = (-uy * h, ux * h);
        let (a, b) = ([a[0] - ux * h, a[1] - uy * h], [b[0] + ux * h, b[1] + uy * h]);
        self.polygon(&[[a[0] + nx, a[1] + ny], [b[0] + nx, b[1] + ny], [b[0] - nx, b[1] - ny], [a[0] - nx, a[1] - ny]]);
    }

    /// A path stroked with a dash pattern (pixels; empty: continuous). Each segment is
    /// clipped to `clip` first, so the work is bounded by the image, not the line's length
    /// (review D1); non-finite segments are skipped.
    fn path(&mut self, pts: &[P], w: f64, dashes: &[f64], clip: ([f64; 2], [f64; 2])) {
        let period: f64 = dashes.iter().sum();
        let dashed = !dashes.is_empty() && period > 0.0;
        let mut dist = 0.0;
        let mut quads = 0usize;
        for s in pts.windows(2) {
            let [a, b] = s else { continue };
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let l = libm::sqrt(dx * dx + dy * dy);
            if !l.is_finite() || !a[0].is_finite() || !a[1].is_finite() {
                continue;
            }
            let p = |u: f64| [a[0] + dx * u, a[1] + dy * u];
            let Some((u0, u1)) = clip_segment(*a, *b, clip) else {
                dist += l;
                continue;
            };
            if !dashed || l < 1e-12 {
                self.segment(p(u0), p(u1), w);
                dist += l;
                continue;
            }
            // The dash pattern continues along the whole path, also where it was clipped.
            let (s0, s1) = (u0 * l, u1 * l);
            let mut phase = (dist + s0) % period;
            let mut k = 0usize;
            let mut left = dashes.first().copied().unwrap_or(1.0);
            while phase >= left && left > 0.0 {
                phase -= left;
                k = (k + 1) % dashes.len();
                left = dashes.get(k).copied().unwrap_or(1.0);
            }
            left -= phase;
            let mut t = s0;
            while t < s1 {
                quads += 1;
                if quads > MAX_DASHES {
                    // Beyond any real drawing: the rest is drawn solid.
                    self.segment(p(t / l), p(u1), w);
                    break;
                }
                let step = left.min(s1 - t);
                if k.is_multiple_of(2) {
                    self.segment(p(t / l), p((t + step) / l), w);
                }
                t += step;
                left -= step;
                if left <= 1e-12 {
                    k = (k + 1) % dashes.len();
                    left = dashes.get(k).copied().unwrap_or(1.0);
                }
            }
            dist += l;
        }
    }

    /// Coverage (0..=1) per pixel, nonzero rule.
    fn coverage(&self, w: usize, h: usize) -> Vec<f32> {
        let mut cov = vec![0f32; w * h];
        let rows = h * SUBROWS;
        // Edges by first sub-row.
        let mut starts: Vec<Vec<usize>> = vec![Vec::new(); rows];
        for (i, (a, b)) in self.edges.iter().enumerate() {
            let (y0, y1) = (a[1].min(b[1]), a[1].max(b[1]));
            if y1 - y0 < 1e-12 || y1 < 0.0 {
                continue;
            }
            let first = libm::ceil(y0 * SUBROWS as f64 - 0.5).max(0.0) as usize;
            if let Some(slot) = starts.get_mut(first) {
                slot.push(i);
            }
        }
        let mut active: Vec<usize> = Vec::new();
        let mut xs: Vec<(f64, i32)> = Vec::new();
        for (r, start) in starts.iter().enumerate() {
            let y = (r as f64 + 0.5) / SUBROWS as f64;
            active.extend(start);
            active.retain(|&i| self.edges.get(i).is_some_and(|(a, b)| a[1].max(b[1]) > y));
            xs.clear();
            for &i in &active {
                let Some((a, b)) = self.edges.get(i) else { continue };
                if (a[1] <= y) == (b[1] <= y) {
                    continue;
                }
                let x = a[0] + (y - a[1]) * (b[0] - a[0]) / (b[1] - a[1]);
                xs.push((x, if b[1] > a[1] { 1 } else { -1 }));
            }
            xs.sort_by(|p, q| p.0.total_cmp(&q.0));
            let row = r / SUBROWS;
            let mut wind = 0;
            for pair in xs.windows(2) {
                let [(x0, d), (x1, _)] = pair else { continue };
                wind += d;
                if wind != 0 {
                    span(&mut cov, w, row, *x0, *x1);
                }
            }
        }
        cov
    }
}

/// Adds `1 / SUBROWS` times the covered fraction of each pixel of `row` in `x0..x1`.
fn span(cov: &mut [f32], w: usize, row: usize, x0: f64, x1: f64) {
    let (x0, x1) = (x0.max(0.0), x1.min(w as f64));
    if x1 <= x0 {
        return;
    }
    let (p0, p1) = (libm::floor(x0) as usize, (libm::ceil(x1) as usize).min(w));
    let unit = 1.0 / SUBROWS as f32;
    for px in p0..p1 {
        let lo = x0.max(px as f64);
        let hi = x1.min(px as f64 + 1.0);
        if let Some(c) = cov.get_mut(row * w + px) {
            *c += unit * (hi - lo).max(0.0) as f32;
        }
    }
}

/// Flattened arc points (pixels), counterclockwise in sheet space.
fn arc_points(c: P, r: f64, s: f64, e: f64) -> Vec<P> {
    let sweep = (e - s).min(TAU);
    // Chord error under 0.05 px.
    let step = 2.0 * libm::acos((1.0 - 0.05 / r.max(0.05)).clamp(-1.0, 1.0));
    let n = ((sweep / step.max(1e-3)).ceil() as usize).clamp(8, 4096);
    (0..=n)
        .map(|i| {
            let a = s + sweep * i as f64 / n as f64;
            [c[0] + r * libm::cos(a), c[1] + r * libm::sin(a)]
        })
        .collect()
}

/// Renders `scene` `width` pixels wide (at most [`MAX_SIDE`] on either side), white
/// paper, black lines, dangling dimensions in magenta.
pub fn render(scene: &Scene, width: u32) -> Raster {
    let [sw, sh] = scene.size;
    let mut wpx = width.clamp(16, MAX_SIDE);
    let mut s = f64::from(wpx) / sw.max(1.0);
    let mut hpx = (sh * s).round().max(1.0) as u32;
    if hpx > MAX_SIDE {
        hpx = MAX_SIDE;
        s = f64::from(hpx) / sh.max(1.0);
        wpx = ((sw * s).round() as u32).clamp(1, MAX_SIDE);
    }
    // Sheet mm (Y up) to pixels (Y down).
    let to = |p: P| [p[0] * s, (sh - p[1]) * s];
    let (mut ink, mut warn) = (Layer::default(), Layer::default());
    // The image with a margin for stroke widths (pixels).
    let clip = ([-8.0, -8.0], [f64::from(wpx) + 8.0, f64::from(hpx) + 8.0]);
    for item in &scene.items {
        let layer = if item.style == Style::Dangling { &mut warn } else { &mut ink };
        // At least one pixel wide, so thin lines stay visible at small sizes.
        let w = (item.style.width() * s).max(1.0);
        let dashes: Vec<f64> = item.style.dashes().iter().map(|d| d * s).collect();
        match &item.prim {
            Prim::Line { a, b } => layer.path(&[to(*a), to(*b)], w, &dashes, clip),
            Prim::Arc { center, radius, start, end } => {
                let pts: Vec<P> = arc_points(*center, *radius, *start, *end).into_iter().map(to).collect();
                layer.path(&pts, w, &dashes, clip);
            }
            Prim::Polyline { points, closed } => {
                let mut pts: Vec<P> = points.iter().map(|p| to(*p)).collect();
                if *closed && let Some(&f) = pts.first() {
                    pts.push(f);
                }
                layer.path(&pts, w, &dashes, clip);
            }
            Prim::Fill { points } => {
                let pts: Vec<P> = points.iter().map(|p| to(*p)).collect();
                layer.polygon(&pts);
            }
            Prim::Text { at, height, text, anchor, angle } => {
                let align = match anchor {
                    Anchor::Left => gusset_text::Align::Left,
                    Anchor::Center => gusset_text::Align::Center,
                    Anchor::Right => gusset_text::Align::Right,
                };
                // Characters the font lacks are left out rather than losing the line.
                let shown: String = text
                    .chars()
                    .filter(|c| {
                        gusset_text::outline(&c.to_string(), gusset_text::Font::Sans, 1.0, align).is_ok() || *c == ' '
                    })
                    .collect();
                let Ok(regions) = gusset_text::outline(shown.trim(), gusset_text::Font::Sans, *height, align) else {
                    continue;
                };
                let (sa, ca) = (libm::sin(*angle), libm::cos(*angle));
                let place = |q: P| to([at[0] + q[0] * ca - q[1] * sa, at[1] + q[0] * sa + q[1] * ca]);
                let mut cs: Vec<Vec<P>> = Vec::new();
                for r in &regions {
                    for c in std::iter::once(&r.outer).chain(&r.holes) {
                        let mut pts = Vec::new();
                        for seg in &c.segs {
                            match *seg {
                                gusset_text::Seg::Line { a, .. } => pts.push(place(a)),
                                gusset_text::Seg::Cubic { p: [a, b, c2, d] } => {
                                    for i in 0..8 {
                                        let t = f64::from(i) / 8.0;
                                        let u = 1.0 - t;
                                        let wts = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
                                        pts.push(place([
                                            wts[0] * a[0] + wts[1] * b[0] + wts[2] * c2[0] + wts[3] * d[0],
                                            wts[0] * a[1] + wts[1] * b[1] + wts[2] * c2[1] + wts[3] * d[1],
                                        ]));
                                    }
                                }
                            }
                        }
                        cs.push(pts);
                    }
                }
                layer.contours(&cs);
            }
        }
    }
    let (w, h) = (wpx as usize, hpx as usize);
    let (ink, warn) = (ink.coverage(w, h), warn.coverage(w, h));
    let mut rgba = vec![255u8; w * h * 4];
    for (i, px) in rgba.chunks_exact_mut(4).enumerate() {
        let k = ink.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0);
        let m = warn.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0);
        // Black ink over white, then magenta (176, 48, 96).
        let mix = |paper: f32, colour: f32| paper * (1.0 - m) + colour * m;
        let base = 255.0 * (1.0 - k);
        let rgb = [mix(base, 176.0), mix(base, 48.0), mix(base, 96.0)];
        for (o, v) in px.iter_mut().zip(rgb) {
            *o = v.round().clamp(0.0, 255.0) as u8;
        }
    }
    Raster { width: wpx, height: hpx, rgba }
}

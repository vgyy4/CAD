//! GD&T on the sheet: feature control frames and datum feature symbols (ISO 1101 / ASME
//! Y14.5). The characteristic symbols are drawn as lines and arcs, not font glyphs, so every
//! export shows them the same.

use crate::dims::arrow;
use crate::scene::{Anchor, Prim, Scene, Style};
use gusset_doc::drawing::GdtSymbol;
use std::f64::consts::{PI, TAU};

/// Cell height and the symbol and datum cell width, sheet mm.
pub(crate) const CELL: f64 = 8.0;
const TEXT_H: f64 = 3.5;

fn line(scene: &mut Scene, c: [f64; 2], a: [f64; 2], b: [f64; 2]) {
    scene.line(Style::Thin, [c[0] + a[0], c[1] + a[1]], [c[0] + b[0], c[1] + b[1]]);
}

fn circle(scene: &mut Scene, c: [f64; 2], r: f64) {
    scene.push(Style::Thin, Prim::Arc { center: c, radius: r, start: 0.0, end: TAU });
}

/// Draws `sym` centred at `c`, within a box of half-size `h`.
pub(crate) fn symbol(scene: &mut Scene, sym: GdtSymbol, c: [f64; 2]) {
    let h = 2.5;
    match sym {
        GdtSymbol::Straightness => line(scene, c, [-h, 0.0], [h, 0.0]),
        GdtSymbol::Flatness => {
            let p = [[-h, -0.6 * h], [0.4 * h, -0.6 * h], [h, 0.6 * h], [-0.4 * h, 0.6 * h]];
            for (a, b) in p.iter().zip(p.iter().cycle().skip(1)) {
                line(scene, c, *a, *b);
            }
        }
        GdtSymbol::Circularity => circle(scene, c, 0.8 * h),
        GdtSymbol::Cylindricity => {
            // A circle between two parallel slanted lines tangent to it.
            let r = 0.55 * h;
            circle(scene, c, r);
            let (dx, dy) = (0.8, 2.0);
            let l = libm::hypot(dx, dy);
            let (ux, uy) = (dx / l, dy / l);
            for side in [-1.0, 1.0] {
                let o = [side * r * uy, -side * r * ux];
                let t = h / uy;
                line(scene, c, [o[0] - ux * t, o[1] - uy * t], [o[0] + ux * t, o[1] + uy * t]);
            }
        }
        GdtSymbol::ProfileLine => {
            scene.push(Style::Thin, Prim::Arc { center: [c[0], c[1] - 0.4 * h], radius: 0.9 * h, start: 0.0, end: PI });
        }
        GdtSymbol::ProfileSurface => {
            scene.push(Style::Thin, Prim::Arc { center: [c[0], c[1] - 0.4 * h], radius: 0.9 * h, start: 0.0, end: PI });
            line(scene, c, [-0.9 * h, -0.4 * h], [0.9 * h, -0.4 * h]);
        }
        GdtSymbol::Perpendicularity => {
            line(scene, c, [-h, -h], [h, -h]);
            line(scene, c, [0.0, -h], [0.0, h]);
        }
        GdtSymbol::Angularity => {
            line(scene, c, [-h, -h], [h, -h]);
            line(scene, c, [-h, -h], [h, 0.3 * h]);
        }
        GdtSymbol::Parallelism => {
            line(scene, c, [-0.7 * h, -h], [0.0, h]);
            line(scene, c, [0.0, -h], [0.7 * h, h]);
        }
        GdtSymbol::Position => {
            circle(scene, c, 0.6 * h);
            line(scene, c, [-h, 0.0], [h, 0.0]);
            line(scene, c, [0.0, -h], [0.0, h]);
        }
        GdtSymbol::Concentricity => {
            circle(scene, c, 0.4 * h);
            circle(scene, c, 0.85 * h);
        }
        GdtSymbol::Symmetry => {
            line(scene, c, [-h, 0.0], [h, 0.0]);
            line(scene, c, [-0.6 * h, 0.55 * h], [0.6 * h, 0.55 * h]);
            line(scene, c, [-0.6 * h, -0.55 * h], [0.6 * h, -0.55 * h]);
        }
        GdtSymbol::Runout => {
            line(scene, c, [-h, -h], [0.6 * h, 0.6 * h]);
            let d = std::f64::consts::FRAC_1_SQRT_2;
            arrow(scene, Style::Thin, [c[0] + h, c[1] + h], [d, d]);
        }
        GdtSymbol::TotalRunout => {
            line(scene, c, [-h, -h], [h, -h]);
            for x in [-0.6 * h, 0.2 * h] {
                line(scene, c, [x, -h], [x + 0.5 * h, 0.3 * h]);
                let d = [0.5, 1.3];
                let l = libm::hypot(d[0], d[1]);
                arrow(scene, Style::Thin, [c[0] + x + 0.75 * h, c[1] + h], [d[0] / l, d[1] / l]);
            }
        }
    }
}

/// A feature control frame with its lower-left corner at `at` and a leader to `target`.
pub(crate) fn frame(
    scene: &mut Scene,
    at: [f64; 2],
    sym: GdtSymbol,
    tolerance: &str,
    datums: &[String],
    target: [f64; 2],
) {
    let tol_w = (tolerance.chars().count() as f64 * TEXT_H * 0.75 + 3.0).max(12.0);
    let widths: Vec<f64> = [CELL, tol_w].into_iter().chain(datums.iter().map(|_| CELL)).collect();
    let w: f64 = widths.iter().sum();
    scene.rect(Style::Thin, at, [at[0] + w, at[1] + CELL]);
    let mut x = at[0];
    for cw in widths.iter().take(widths.len().saturating_sub(1)) {
        x += cw;
        scene.line(Style::Thin, [x, at[1]], [x, at[1] + CELL]);
    }
    symbol(scene, sym, [at[0] + CELL / 2.0, at[1] + CELL / 2.0]);
    let ty = at[1] + (CELL - TEXT_H) / 2.0;
    scene.text([at[0] + CELL + tol_w / 2.0, ty], TEXT_H, tolerance, Anchor::Center);
    for (i, d) in datums.iter().enumerate() {
        scene.text([at[0] + CELL + tol_w + CELL * (i as f64 + 0.5), ty], TEXT_H, d, Anchor::Center);
    }
    leader(scene, [at[0], at[1] + CELL / 2.0], target);
}

/// A leader from `from` to `target` with an arrow at the target.
fn leader(scene: &mut Scene, from: [f64; 2], target: [f64; 2]) {
    let (dx, dy) = (target[0] - from[0], target[1] - from[1]);
    let l = libm::hypot(dx, dy);
    if l > 1e-9 {
        scene.line(Style::Thin, from, target);
        arrow(scene, Style::Thin, target, [dx / l, dy / l]);
    }
}

/// A datum feature symbol: the boxed `letter` centred at `at`, joined to `target` by a line
/// ending in a filled triangle whose base lies on the feature.
pub(crate) fn datum(scene: &mut Scene, at: [f64; 2], letter: &str, target: [f64; 2]) {
    let hw = CELL / 2.0;
    scene.rect(Style::Thin, [at[0] - hw, at[1] - hw], [at[0] + hw, at[1] + hw]);
    scene.text([at[0], at[1] - TEXT_H / 2.0], TEXT_H, letter, Anchor::Center);
    let (dx, dy) = (at[0] - target[0], at[1] - target[1]);
    let l = libm::hypot(dx, dy);
    if l > hw + 3.0 {
        let u = [dx / l, dy / l];
        let n = [-u[1], u[0]];
        let apex = [target[0] + 2.6 * u[0], target[1] + 2.6 * u[1]];
        let base = |s: f64| [target[0] + s * 1.5 * n[0], target[1] + s * 1.5 * n[1]];
        scene.push(Style::Thin, Prim::Fill { points: vec![base(1.0), apex, base(-1.0)] });
        // To the box's edge along the line.
        let t = hw / u[0].abs().max(u[1].abs());
        scene.line(Style::Thin, apex, [at[0] - u[0] * t, at[1] - u[1] * t]);
    }
}

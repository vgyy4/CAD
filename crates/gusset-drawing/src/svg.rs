//! SVG output of a [`Scene`]: one user unit is 1 mm (`viewBox` in mm, `width`/`height`
//! in mm), Y flipped in the coordinates (no transforms, so laser software reads it as
//! drawn), one group per line style.

use crate::scene::{Anchor, Prim, Scene, Style};
use std::f64::consts::TAU;
use std::fmt::Write as _;

/// Coordinates to 4 decimals (0.1 µm), without trailing zeros.
fn n(v: f64) -> String {
    let r = libm::round(v * 10_000.0) / 10_000.0;
    let s = format!("{:.4}", if r == 0.0 { 0.0 } else { r });
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.to_string()
}

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            c if c.is_control() => {}
            c => o.push(c),
        }
    }
    o
}

/// The scene as an SVG document.
pub fn to_svg(scene: &Scene) -> String {
    let [w, h] = scene.size;
    let y = |v: f64| h - v;
    let mut s = String::new();
    let _ = writeln!(s, r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    let _ = writeln!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}mm" height="{}mm" viewBox="0 0 {} {}">"#,
        n(w),
        n(h),
        n(w),
        n(h)
    );
    let _ = writeln!(s, r#"<rect width="{}" height="{}" fill="white"/>"#, n(w), n(h));
    for style in Style::ALL {
        let items: Vec<&Prim> = scene.items.iter().filter(|i| i.style == style).map(|i| &i.prim).collect();
        if items.is_empty() {
            continue;
        }
        let dash = style.dashes();
        let dash = if dash.is_empty() {
            String::new()
        } else {
            format!(r#" stroke-dasharray="{}""#, dash.iter().map(|d| n(*d)).collect::<Vec<_>>().join(" "))
        };
        let color = if style == Style::Dangling { "#b03060" } else { "black" };
        if style == Style::Text {
            let _ = writeln!(s, r#"<g class="{}" fill="black" font-family="sans-serif">"#, style.layer());
        } else {
            let _ = writeln!(
                s,
                r#"<g class="{}" fill="none" stroke="{color}" stroke-width="{}" stroke-linecap="round" stroke-linejoin="round"{dash}>"#,
                style.layer(),
                n(style.width())
            );
        }
        for p in items {
            match p {
                Prim::Line { a, b } => {
                    let _ = writeln!(
                        s,
                        r#"<line x1="{}" y1="{}" x2="{}" y2="{}"/>"#,
                        n(a[0]),
                        n(y(a[1])),
                        n(b[0]),
                        n(y(b[1]))
                    );
                }
                Prim::Arc { center, radius, start, end } => {
                    if end - start >= TAU - 1e-12 {
                        let _ = writeln!(
                            s,
                            r#"<circle cx="{}" cy="{}" r="{}"/>"#,
                            n(center[0]),
                            n(y(center[1])),
                            n(*radius)
                        );
                    } else {
                        let p = |a: f64| [center[0] + radius * libm::cos(a), center[1] + radius * libm::sin(a)];
                        let (a, b) = (p(*start), p(*end));
                        let large = u8::from(end - start > std::f64::consts::PI);
                        // Counterclockwise with Y up is a negative sweep with Y down.
                        let _ = writeln!(
                            s,
                            r#"<path d="M{} {} A{} {} 0 {large} 0 {} {}"/>"#,
                            n(a[0]),
                            n(y(a[1])),
                            n(*radius),
                            n(*radius),
                            n(b[0]),
                            n(y(b[1]))
                        );
                    }
                }
                Prim::Polyline { points, closed } => {
                    let pts: Vec<String> = points.iter().map(|q| format!("{},{}", n(q[0]), n(y(q[1])))).collect();
                    let tag = if *closed { "polygon" } else { "polyline" };
                    let _ = writeln!(s, r#"<{tag} points="{}"/>"#, pts.join(" "));
                }
                Prim::Fill { points } => {
                    let pts: Vec<String> = points.iter().map(|q| format!("{},{}", n(q[0]), n(y(q[1])))).collect();
                    let _ = writeln!(s, r#"<polygon points="{}" fill="{color}" stroke="none"/>"#, pts.join(" "));
                }
                Prim::Text { at, height, text, anchor, angle } => {
                    let a = match anchor {
                        Anchor::Left => "start",
                        Anchor::Center => "middle",
                        Anchor::Right => "end",
                    };
                    // The font size is the em height; caps are about 0.7 of it.
                    let size = height / 0.7;
                    let rot = if *angle == 0.0 {
                        String::new()
                    } else {
                        format!(r#" transform="rotate({} {} {})""#, n(-angle.to_degrees()), n(at[0]), n(y(at[1])))
                    };
                    let _ = writeln!(
                        s,
                        r#"<text x="{}" y="{}" font-size="{}" text-anchor="{a}"{rot}>{}</text>"#,
                        n(at[0]),
                        n(y(at[1])),
                        n(size),
                        esc(text)
                    );
                }
            }
        }
        s.push_str("</g>\n");
    }
    s.push_str("</svg>\n");
    s
}

//! PDF output of a [`Scene`] (ADR 0032 phase 4): one vector page the size of the sheet,
//! drawn in millimetres (one `cm` scale), line widths and ISO dash patterns per style,
//! arcs as cubic Béziers, text in the standard Helvetica font (no embedding; WinAnsi
//! encoding covers Ø, °, ±, × and the en dash). Written by our own code, no dependency.

use crate::scene::{Anchor, Prim, Scene, Style};
use std::f64::consts::{FRAC_PI_2, TAU};
use std::fmt::Write as _;

/// Points per millimetre.
const PT: f64 = 72.0 / 25.4;

fn n(v: f64) -> String {
    let r = libm::round(v * 10_000.0) / 10_000.0;
    let s = format!("{:.4}", if r == 0.0 { 0.0 } else { r });
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// A character in WinAnsiEncoding, or `?`.
fn win_ansi(c: char) -> u8 {
    match c as u32 {
        0x20..=0x7e | 0xa0..=0xff => c as u32 as u8,
        0x2013 => 0x96,
        0x2014 => 0x97,
        0x2022 => 0x95,
        0x20ac => 0x80,
        0x2018 => 0x91,
        0x2019 => 0x92,
        0x201c => 0x93,
        0x201d => 0x94,
        _ => b'?',
    }
}

/// Helvetica advance widths (1/1000 em) of the printable ASCII characters.
const HELVETICA: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, // space../
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, // 0..9
    278, 278, 584, 584, 584, 556, 1015, // :..@
    667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944,
    667, 667, 611, // A..Z
    278, 278, 278, 469, 556, 333, // [..`
    556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722,
    500, 500, 500, // a..z
    334, 260, 334, 584, // {..~
];

fn width(b: u8) -> f64 {
    let w = match b {
        0x20..=0x7e => HELVETICA.get(usize::from(b - 0x20)).copied().unwrap_or(556),
        0xb0 => 400,
        0xb1 | 0xd7 => 584,
        0xd8 => 778,
        0x96 => 556,
        0x97 => 1000,
        _ => 556,
    };
    f64::from(w) / 1000.0
}

/// A PDF string literal of WinAnsi bytes.
fn literal(bytes: &[u8]) -> String {
    let mut s = String::from("(");
    for &b in bytes {
        match b {
            b'(' | b')' | b'\\' => {
                s.push('\\');
                s.push(char::from(b));
            }
            0x20..=0x7e => s.push(char::from(b)),
            _ => {
                let _ = write!(s, "\\{b:03o}");
            }
        }
    }
    s.push(')');
    s
}

/// Cubic Bézier segments (each at most a quarter turn) of an arc, as path operators.
fn arc_path(out: &mut String, c: [f64; 2], r: f64, s: f64, e: f64) {
    let sweep = (e - s).clamp(0.0, TAU);
    let k = (sweep / FRAC_PI_2).ceil().max(1.0) as usize;
    let step = sweep / k as f64;
    let h = 4.0 / 3.0 * libm::tan(step / 4.0);
    let p = |a: f64| [c[0] + r * libm::cos(a), c[1] + r * libm::sin(a)];
    let p0 = p(s);
    let _ = writeln!(out, "{} {} m", n(p0[0]), n(p0[1]));
    for i in 0..k {
        let a0 = s + step * i as f64;
        let a1 = a0 + step;
        let (q0, q3) = (p(a0), p(a1));
        let q1 = [q0[0] - h * r * libm::sin(a0), q0[1] + h * r * libm::cos(a0)];
        let q2 = [q3[0] + h * r * libm::sin(a1), q3[1] - h * r * libm::cos(a1)];
        let _ = writeln!(out, "{} {} {} {} {} {} c", n(q1[0]), n(q1[1]), n(q2[0]), n(q2[1]), n(q3[0]), n(q3[1]));
    }
}

/// The scene as a one-page PDF document.
pub fn to_pdf(scene: &Scene, title: &str) -> Vec<u8> {
    let [w, h] = scene.size;
    let mut c = String::new();
    let _ = writeln!(c, "{} 0 0 {} 0 0 cm", n(PT), n(PT));
    let _ = writeln!(c, "1 J 1 j");
    for style in Style::ALL {
        let items: Vec<&Prim> = scene.items.iter().filter(|i| i.style == style).map(|i| &i.prim).collect();
        if items.is_empty() {
            continue;
        }
        let rgb = if style == Style::Dangling { "0.69 0.19 0.38" } else { "0 0 0" };
        let _ = writeln!(c, "{rgb} RG {rgb} rg {} w", n(style.width()));
        let dash: Vec<String> = style.dashes().iter().map(|d| n(*d)).collect();
        let _ = writeln!(c, "[{}] 0 d", dash.join(" "));
        for p in items {
            match p {
                Prim::Line { a, b } => {
                    let _ = writeln!(c, "{} {} m {} {} l S", n(a[0]), n(a[1]), n(b[0]), n(b[1]));
                }
                Prim::Arc { center, radius, start, end } => {
                    arc_path(&mut c, *center, *radius, *start, *end);
                    c.push_str("S\n");
                }
                Prim::Polyline { points, closed } => {
                    for (i, q) in points.iter().enumerate() {
                        let _ = writeln!(c, "{} {} {}", n(q[0]), n(q[1]), if i == 0 { "m" } else { "l" });
                    }
                    c.push_str(if *closed { "s\n" } else { "S\n" });
                }
                Prim::Fill { points } => {
                    for (i, q) in points.iter().enumerate() {
                        let _ = writeln!(c, "{} {} {}", n(q[0]), n(q[1]), if i == 0 { "m" } else { "l" });
                    }
                    c.push_str("h f\n");
                }
                Prim::Text { at, height, text, anchor, angle } => {
                    let bytes: Vec<u8> = text.chars().filter(|ch| !ch.is_control()).map(win_ansi).collect();
                    // The font size is the em; capitals are about 0.7 of it (as in SVG).
                    let size = height / 0.7;
                    let tw = bytes.iter().map(|b| width(*b)).sum::<f64>() * size;
                    let dx = match anchor {
                        Anchor::Left => 0.0,
                        Anchor::Center => -tw / 2.0,
                        Anchor::Right => -tw,
                    };
                    let (s, co) = (libm::sin(*angle), libm::cos(*angle));
                    let x = at[0] + dx * co;
                    let y = at[1] + dx * s;
                    let _ = writeln!(
                        c,
                        "BT /F1 {} Tf {} {} {} {} {} {} Tm {} Tj ET",
                        n(size),
                        n(co),
                        n(s),
                        n(-s),
                        n(co),
                        n(x),
                        n(y),
                        literal(&bytes)
                    );
                }
            }
        }
    }

    // Objects: catalog, pages, page, font, contents, info.
    let title: Vec<u8> = title.chars().filter(|ch| !ch.is_control()).map(win_ansi).collect();
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>",
            n(w * PT),
            n(h * PT)
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_string(),
        format!("<< /Length {} >>\nstream\n{c}endstream", c.len()),
        format!("<< /Title {} /Producer (Gusset) >>", literal(&title)),
    ];
    let mut out: Vec<u8> = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    let mut tail = format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
    for off in offsets {
        let _ = writeln!(tail, "{off:010} 00000 n ");
    }
    let _ =
        write!(tail, "trailer\n<< /Size {} /Root 1 0 R /Info 6 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1);
    out.extend_from_slice(tail.as_bytes());
    out
}

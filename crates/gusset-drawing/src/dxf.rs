//! DXF output of a [`Scene`] (ADR 0032 §4): one layer per line style with its linetype,
//! dimensions exploded into lines, arrows and text, millimetres.
//!
//! [`Profile::R12`] (AC1009) is the plainest form, read by laser cutters and old
//! software: LINE, ARC, CIRCLE, POLYLINE, SOLID and TEXT. Text that is not ASCII is
//! written as `\U+XXXX`.

use crate::scene::{Anchor, Prim, Scene, Style};
use std::f64::consts::TAU;
use std::fmt::Write as _;

/// Which DXF version to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Profile {
    /// AutoCAD R12 (AC1009).
    #[default]
    R12,
}

/// Values to 6 decimals (1 nm), without trailing zeros.
fn n(v: f64) -> String {
    let r = libm::round(v * 1e6) / 1e6;
    let s = format!("{:.6}", if r == 0.0 { 0.0 } else { r });
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" { "0".into() } else { s.to_string() }
}

/// Text in the DXF's 8-bit encoding: ASCII as is, everything else as `\U+XXXX`.
fn text(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii() && !c.is_ascii_control() {
            o.push(c);
        } else if !c.is_control() {
            let mut buf = [0u16; 2];
            for u in c.encode_utf16(&mut buf) {
                let _ = write!(o, "\\U+{u:04X}");
            }
        }
    }
    o
}

struct W {
    s: String,
}

impl W {
    fn g(&mut self, code: i32, v: impl std::fmt::Display) {
        let _ = write!(self.s, "{code:>3}\n{v}\n");
    }
    fn f(&mut self, code: i32, v: f64) {
        self.g(code, n(v));
    }
    fn xy(&mut self, base: i32, p: [f64; 2]) {
        self.f(base, p[0]);
        self.f(base + 10, p[1]);
        self.f(base + 20, 0.0);
    }
    fn entity(&mut self, kind: &str, style: Style) {
        self.g(0, kind);
        self.g(8, style.layer());
    }
}

/// The linetype a style is drawn with.
fn linetype(style: Style) -> &'static str {
    match style {
        Style::Hidden => "HIDDEN",
        Style::Center => "CENTER",
        Style::Dangling => "DASHED",
        _ => "CONTINUOUS",
    }
}

/// The scene as DXF text.
pub fn to_dxf(scene: &Scene, profile: Profile) -> String {
    let Profile::R12 = profile;
    let mut w = W { s: String::new() };
    let (lo, hi) = extents(scene);
    // Header.
    w.g(0, "SECTION");
    w.g(2, "HEADER");
    w.g(9, "$ACADVER");
    w.g(1, "AC1009");
    w.g(9, "$EXTMIN");
    w.xy(10, lo);
    w.g(9, "$EXTMAX");
    w.xy(10, hi);
    w.g(9, "$LIMMIN");
    w.f(10, 0.0);
    w.f(20, 0.0);
    w.g(9, "$LIMMAX");
    w.f(10, scene.size[0]);
    w.f(20, scene.size[1]);
    w.g(0, "ENDSEC");
    // Tables: linetypes, layers, the text style.
    w.g(0, "SECTION");
    w.g(2, "TABLES");
    let ltypes: [(&str, &str, &[f64]); 4] = [
        ("CONTINUOUS", "Solid line", &[]),
        ("HIDDEN", "Hidden __ __ __", &[3.0, -0.75]),
        ("CENTER", "Center ____ _ ____", &[6.0, -0.75, 0.0, -0.75]),
        ("DASHED", "Dashed _ _ _", &[1.5, -0.75]),
    ];
    w.g(0, "TABLE");
    w.g(2, "LTYPE");
    w.g(70, ltypes.len());
    for (name, desc, pat) in ltypes {
        w.g(0, "LTYPE");
        w.g(2, name);
        w.g(70, 0);
        w.g(3, desc);
        w.g(72, 65);
        w.g(73, pat.len());
        w.f(40, pat.iter().map(|v| v.abs()).sum());
        for v in pat {
            w.f(49, *v);
        }
    }
    w.g(0, "ENDTAB");
    w.g(0, "TABLE");
    w.g(2, "LAYER");
    w.g(70, Style::ALL.len() + 1);
    w.g(0, "LAYER");
    w.g(2, "0");
    w.g(70, 0);
    w.g(62, 7);
    w.g(6, "CONTINUOUS");
    for style in Style::ALL {
        w.g(0, "LAYER");
        w.g(2, style.layer());
        w.g(70, 0);
        w.g(62, if style == Style::Dangling { 6 } else { 7 });
        w.g(6, linetype(style));
    }
    w.g(0, "ENDTAB");
    w.g(0, "TABLE");
    w.g(2, "STYLE");
    w.g(70, 1);
    w.g(0, "STYLE");
    w.g(2, "STANDARD");
    w.g(70, 0);
    w.f(40, 0.0);
    w.f(41, 1.0);
    w.f(50, 0.0);
    w.g(71, 0);
    w.f(42, 2.5);
    w.g(3, "txt");
    w.g(4, "");
    w.g(0, "ENDTAB");
    w.g(0, "ENDSEC");
    // Entities.
    w.g(0, "SECTION");
    w.g(2, "ENTITIES");
    for item in &scene.items {
        let style = item.style;
        match &item.prim {
            Prim::Line { a, b } => {
                w.entity("LINE", style);
                w.xy(10, *a);
                w.xy(11, *b);
            }
            Prim::Arc { center, radius, start, end } => {
                if end - start >= TAU - 1e-12 {
                    w.entity("CIRCLE", style);
                    w.xy(10, *center);
                    w.f(40, *radius);
                } else {
                    w.entity("ARC", style);
                    w.xy(10, *center);
                    w.f(40, *radius);
                    w.f(50, start.to_degrees().rem_euclid(360.0));
                    w.f(51, end.to_degrees().rem_euclid(360.0));
                }
            }
            Prim::Polyline { points, closed } => {
                if points.len() < 2 {
                    continue;
                }
                w.entity("POLYLINE", style);
                w.g(66, 1);
                w.xy(10, [0.0, 0.0]);
                w.g(70, u8::from(*closed));
                for p in points {
                    w.entity("VERTEX", style);
                    w.xy(10, *p);
                }
                w.entity("SEQEND", style);
            }
            Prim::Fill { points } => {
                // SOLIDs are quadrilaterals with the corners in Z order: a fan of them.
                let Some(&first) = points.first() else { continue };
                for pair in points.get(1..).unwrap_or(&[]).windows(2) {
                    let (Some(&b), Some(&c)) = (pair.first(), pair.get(1)) else { continue };
                    w.entity("SOLID", style);
                    w.xy(10, first);
                    w.xy(11, b);
                    w.xy(12, c);
                    w.xy(13, c);
                }
            }
            Prim::Text { at, height, text: t, anchor, angle } => {
                w.entity("TEXT", style);
                w.xy(10, *at);
                w.f(40, *height);
                w.g(1, text(t));
                if *angle != 0.0 {
                    w.f(50, angle.to_degrees());
                }
                let h = match anchor {
                    Anchor::Left => 0,
                    Anchor::Center => 1,
                    Anchor::Right => 2,
                };
                if h != 0 {
                    w.g(72, h);
                    w.xy(11, *at);
                }
            }
        }
    }
    w.g(0, "ENDSEC");
    w.g(0, "EOF");
    w.s
}

fn extents(scene: &Scene) -> ([f64; 2], [f64; 2]) {
    ([0.0, 0.0], scene.size)
}

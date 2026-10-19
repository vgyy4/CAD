//! DXF output of a [`Scene`] (ADR 0032 §4): one layer per line style with its linetype,
//! dimensions exploded into lines, arrows and text, millimetres.
//!
//! [`Profile::R2000`] (AC1015, the default) carries the units (`$INSUNITS` = mm) and
//! line weights, with the handles, owners and objects AutoCAD expects.
//! [`Profile::R12`] (AC1009) is the plainest form, read by laser cutters and old
//! software: LINE, ARC, CIRCLE, POLYLINE, SOLID and TEXT. Text that is not ASCII is
//! written as `\U+XXXX` in both.

use crate::scene::{Anchor, Prim, Scene, Style};
use std::f64::consts::TAU;
use std::fmt::Write as _;

/// Which DXF version to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Profile {
    /// AutoCAD 2000 (AC1015).
    #[default]
    R2000,
    /// AutoCAD R12 (AC1009).
    R12,
}

impl Profile {
    pub fn from_keyword(s: &str) -> Option<Profile> {
        match s.to_ascii_lowercase().as_str() {
            "r2000" | "2000" | "ac1015" => Some(Profile::R2000),
            "r12" | "12" | "ac1009" => Some(Profile::R12),
            _ => None,
        }
    }
}

/// Values to 6 decimals (1 nm), without trailing zeros.
fn n(v: f64) -> String {
    let r = libm::round(v * 1e6) / 1e6;
    let s = format!("{:.6}", if r == 0.0 { 0.0 } else { r });
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" { "0".into() } else { s.to_string() }
}

/// Text in the DXF's 8-bit encoding: ASCII as is, `%` as `%%%` (`%%` starts a control
/// code), other BMP characters as `\U+XXXX`, and `?` beyond the BMP (review N1: readers do
/// not join surrogates).
fn text(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        if c == '%' {
            o.push_str("%%%");
        } else if c.is_ascii() && !c.is_ascii_control() {
            o.push(c);
        } else if u32::from(c) > 0xFFFF {
            o.push('?');
        } else if !c.is_control() {
            let _ = write!(o, "\\U+{:04X}", u32::from(c));
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
    /// A 2D point (no Z group).
    fn xy2(&mut self, base: i32, p: [f64; 2]) {
        self.f(base, p[0]);
        self.f(base + 10, p[1]);
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
    match profile {
        Profile::R2000 => r2000(scene),
        Profile::R12 => r12(scene),
    }
}

fn r12(scene: &Scene) -> String {
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
    let ltypes = LTYPES;
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

/// Line weight in 1/100 mm (group 370).
fn lineweight(style: Style) -> i32 {
    libm::round(style.width() * 100.0) as i32
}

/// The linetypes both profiles define: name, description, pattern (negative = gap).
const LTYPES: [(&str, &str, &[f64]); 4] = [
    ("CONTINUOUS", "Solid line", &[]),
    ("HIDDEN", "Hidden __ __ __", &[3.0, -0.75]),
    ("CENTER", "Center ____ _ ____", &[6.0, -0.75, 0.0, -0.75]),
    ("DASHED", "Dashed _ _ _", &[1.5, -0.75]),
];

/// Handles in allocation order, as upper-case hex.
struct Handles(u32);

impl Handles {
    fn next(&mut self) -> String {
        self.0 += 1;
        format!("{:X}", self.0)
    }
}

/// An R2000 table header.
fn table(w: &mut W, name: &str, handle: &str, count: usize) {
    w.g(0, "TABLE");
    w.g(2, name);
    w.g(5, handle);
    w.g(330, "0");
    w.g(100, "AcDbSymbolTable");
    w.g(70, count);
}

/// An R2000 table entry header.
fn record(w: &mut W, kind: &str, handle: &str, owner: &str, subclass: &str) {
    w.g(0, kind);
    w.g(if kind == "DIMSTYLE" { 105 } else { 5 }, handle);
    w.g(330, owner);
    w.g(100, "AcDbSymbolTableRecord");
    w.g(100, subclass);
}

/// A LAYOUT object: paper `size`, owned by the layout dictionary, for a block record.
fn layout(w: &mut W, h: &str, dict: &str, name: &str, tab: i32, size: [f64; 2], block: &str) {
    w.g(0, "LAYOUT");
    w.g(5, h);
    w.g(102, "{ACAD_REACTORS");
    w.g(330, dict);
    w.g(102, "}");
    w.g(330, dict);
    w.g(100, "AcDbPlotSettings");
    w.g(1, "");
    w.g(4, "");
    w.g(6, "");
    for (c, v) in [(40, 0.0), (41, 0.0), (42, 0.0), (43, 0.0), (44, size[0]), (45, size[1])] {
        w.f(c, v);
    }
    for c in [46, 47, 48, 49, 140, 141] {
        w.f(c, 0.0);
    }
    w.f(142, 1.0);
    w.f(143, 1.0);
    w.g(70, if tab == 0 { 1024 } else { 0 });
    w.g(72, 1);
    w.g(73, 0);
    w.g(74, 5);
    w.g(7, "");
    w.g(75, 16);
    w.f(147, 1.0);
    w.f(148, 0.0);
    w.f(149, 0.0);
    w.g(100, "AcDbLayout");
    w.g(1, name);
    w.g(70, 1);
    w.g(71, tab);
    w.xy2(10, [0.0, 0.0]);
    w.xy2(11, size);
    w.xy(12, [0.0, 0.0]);
    for (c, v) in [(14, 1e20), (15, -1e20)] {
        w.f(c, v);
        w.f(c + 10, v);
        w.f(c + 20, v);
    }
    w.f(146, 0.0);
    w.xy(13, [0.0, 0.0]);
    for (c, v) in [(16, [1.0, 0.0, 0.0]), (17, [0.0, 1.0, 0.0])] {
        w.f(c, v[0]);
        w.f(c + 10, v[1]);
        w.f(c + 20, v[2]);
    }
    w.g(76, 1);
    w.g(330, block);
}

fn r2000(scene: &Scene) -> String {
    let mut h = Handles(0);
    // Fixed objects first, so the tables can point at them.
    let (t_vport, t_ltype, t_layer, t_style, t_view, t_ucs, t_appid, t_dimstyle, t_brec) =
        (h.next(), h.next(), h.next(), h.next(), h.next(), h.next(), h.next(), h.next(), h.next());
    let (d_root, d_group, d_layout, d_plotstyle, placeholder) = (h.next(), h.next(), h.next(), h.next(), h.next());
    let (br_model, br_paper, lo_model, lo_paper) = (h.next(), h.next(), h.next(), h.next());
    let (blk_model, end_model, blk_paper, end_paper) = (h.next(), h.next(), h.next(), h.next());

    let mut w = W { s: String::new() };
    // Tables.
    w.g(0, "SECTION");
    w.g(2, "TABLES");
    table(&mut w, "VPORT", &t_vport, 1);
    let vp = h.next();
    record(&mut w, "VPORT", &vp, &t_vport, "AcDbViewportTableRecord");
    w.g(2, "*Active");
    w.g(70, 0);
    w.xy2(10, [0.0, 0.0]);
    w.xy2(11, [1.0, 1.0]);
    w.xy2(12, [scene.size[0] / 2.0, scene.size[1] / 2.0]);
    w.xy2(13, [0.0, 0.0]);
    w.xy2(14, [10.0, 10.0]);
    w.xy2(15, [10.0, 10.0]);
    w.xy(16, [0.0, 0.0]);
    w.f(36, 1.0);
    w.xy(17, [0.0, 0.0]);
    w.f(40, scene.size[1] * 1.1);
    w.f(41, scene.size[0] / scene.size[1].max(1.0));
    w.f(42, 50.0);
    w.f(43, 0.0);
    w.f(44, 0.0);
    w.f(50, 0.0);
    w.f(51, 0.0);
    for (c, v) in [(71, 0), (72, 1000), (73, 1), (74, 3), (75, 0), (76, 0), (77, 0), (78, 0)] {
        w.g(c, v);
    }
    w.g(0, "ENDTAB");

    table(&mut w, "LTYPE", &t_ltype, LTYPES.len() + 2);
    for (name, desc, pat) in [("ByBlock", "", &[][..]), ("ByLayer", "", &[][..])].into_iter().chain(LTYPES) {
        let lh = h.next();
        record(&mut w, "LTYPE", &lh, &t_ltype, "AcDbLinetypeTableRecord");
        w.g(2, if name == "CONTINUOUS" { "Continuous" } else { name });
        w.g(70, 0);
        w.g(3, desc);
        w.g(72, 65);
        w.g(73, pat.len());
        w.f(40, pat.iter().map(|v| v.abs()).sum());
        for v in pat {
            w.f(49, *v);
            w.g(74, 0);
        }
    }
    w.g(0, "ENDTAB");

    table(&mut w, "LAYER", &t_layer, Style::ALL.len() + 1);
    let layers = std::iter::once(("0", 7, "Continuous", 25)).chain(Style::ALL.iter().map(|s| {
        let lt = match linetype(*s) {
            "CONTINUOUS" => "Continuous",
            other => other,
        };
        (s.layer(), if *s == Style::Dangling { 6 } else { 7 }, lt, lineweight(*s))
    }));
    for (name, color, lt, lw) in layers {
        let lh = h.next();
        record(&mut w, "LAYER", &lh, &t_layer, "AcDbLayerTableRecord");
        w.g(2, name);
        w.g(70, 0);
        w.g(62, color);
        w.g(6, lt);
        w.g(370, lw);
        w.g(390, &placeholder);
    }
    w.g(0, "ENDTAB");

    table(&mut w, "STYLE", &t_style, 1);
    let sh = h.next();
    record(&mut w, "STYLE", &sh, &t_style, "AcDbTextStyleTableRecord");
    w.g(2, "Standard");
    w.g(70, 0);
    w.f(40, 0.0);
    w.f(41, 1.0);
    w.f(50, 0.0);
    w.g(71, 0);
    w.f(42, 2.5);
    w.g(3, "txt");
    w.g(4, "");
    w.g(0, "ENDTAB");

    for (name, th) in [("VIEW", &t_view), ("UCS", &t_ucs)] {
        table(&mut w, name, th, 0);
        w.g(0, "ENDTAB");
    }

    table(&mut w, "APPID", &t_appid, 1);
    let ah = h.next();
    record(&mut w, "APPID", &ah, &t_appid, "AcDbRegAppTableRecord");
    w.g(2, "ACAD");
    w.g(70, 0);
    w.g(0, "ENDTAB");

    table(&mut w, "DIMSTYLE", &t_dimstyle, 1);
    w.g(100, "AcDbDimStyleTable");
    let dh = h.next();
    record(&mut w, "DIMSTYLE", &dh, &t_dimstyle, "AcDbDimStyleTableRecord");
    w.g(2, "Standard");
    w.g(70, 0);
    w.g(0, "ENDTAB");

    table(&mut w, "BLOCK_RECORD", &t_brec, 2);
    for (bh, name, lo) in [(&br_model, "*Model_Space", &lo_model), (&br_paper, "*Paper_Space", &lo_paper)] {
        record(&mut w, "BLOCK_RECORD", bh, &t_brec, "AcDbBlockTableRecord");
        w.g(2, name);
        w.g(340, lo);
    }
    w.g(0, "ENDTAB");
    w.g(0, "ENDSEC");

    // Blocks.
    w.g(0, "SECTION");
    w.g(2, "BLOCKS");
    for (bh, eh, owner, name) in
        [(&blk_model, &end_model, &br_model, "*Model_Space"), (&blk_paper, &end_paper, &br_paper, "*Paper_Space")]
    {
        w.g(0, "BLOCK");
        w.g(5, bh);
        w.g(330, owner);
        w.g(100, "AcDbEntity");
        w.g(8, "0");
        w.g(100, "AcDbBlockBegin");
        w.g(2, name);
        w.g(70, 0);
        w.xy(10, [0.0, 0.0]);
        w.g(3, name);
        w.g(1, "");
        w.g(0, "ENDBLK");
        w.g(5, eh);
        w.g(330, owner);
        w.g(100, "AcDbEntity");
        w.g(8, "0");
        w.g(100, "AcDbBlockEnd");
    }
    w.g(0, "ENDSEC");

    // Entities, in model space.
    w.g(0, "SECTION");
    w.g(2, "ENTITIES");
    let entity = |w: &mut W, h: &mut Handles, kind: &str, style: Style, subclass: &str| {
        w.g(0, kind);
        w.g(5, h.next());
        w.g(330, &br_model);
        w.g(100, "AcDbEntity");
        w.g(8, style.layer());
        w.g(100, subclass);
    };
    for item in &scene.items {
        let style = item.style;
        match &item.prim {
            Prim::Line { a, b } => {
                entity(&mut w, &mut h, "LINE", style, "AcDbLine");
                w.xy(10, *a);
                w.xy(11, *b);
            }
            Prim::Arc { center, radius, start, end } => {
                let full = end - start >= TAU - 1e-12;
                entity(&mut w, &mut h, if full { "CIRCLE" } else { "ARC" }, style, "AcDbCircle");
                w.xy(10, *center);
                w.f(40, *radius);
                if !full {
                    w.g(100, "AcDbArc");
                    w.f(50, start.to_degrees().rem_euclid(360.0));
                    w.f(51, end.to_degrees().rem_euclid(360.0));
                }
            }
            Prim::Polyline { points, closed } => {
                if points.len() < 2 {
                    continue;
                }
                entity(&mut w, &mut h, "LWPOLYLINE", style, "AcDbPolyline");
                w.g(90, points.len());
                w.g(70, u8::from(*closed));
                w.f(43, 0.0);
                for p in points {
                    w.xy2(10, *p);
                }
            }
            Prim::Fill { points } => {
                let Some(&first) = points.first() else { continue };
                for pair in points.get(1..).unwrap_or(&[]).windows(2) {
                    let (Some(&b), Some(&c)) = (pair.first(), pair.get(1)) else { continue };
                    entity(&mut w, &mut h, "SOLID", style, "AcDbTrace");
                    w.xy(10, first);
                    w.xy(11, b);
                    w.xy(12, c);
                    w.xy(13, c);
                }
            }
            Prim::Text { at, height, text: t, anchor, angle } => {
                entity(&mut w, &mut h, "TEXT", style, "AcDbText");
                w.xy(10, *at);
                w.f(40, *height);
                w.g(1, text(t));
                if *angle != 0.0 {
                    w.f(50, angle.to_degrees());
                }
                w.g(7, "Standard");
                let ha = match anchor {
                    Anchor::Left => 0,
                    Anchor::Center => 1,
                    Anchor::Right => 2,
                };
                if ha != 0 {
                    w.g(72, ha);
                    w.xy(11, *at);
                }
                w.g(100, "AcDbText");
            }
        }
    }
    w.g(0, "ENDSEC");

    // Objects: the root dictionary, groups, layouts and the plot style placeholder.
    w.g(0, "SECTION");
    w.g(2, "OBJECTS");
    let dict = |w: &mut W, hh: &str, owner: &str, entries: &[(&str, &str)]| {
        w.g(0, "DICTIONARY");
        w.g(5, hh);
        w.g(330, owner);
        w.g(100, "AcDbDictionary");
        w.g(281, 1);
        for (k, v) in entries {
            w.g(3, k);
            w.g(350, v);
        }
    };
    dict(
        &mut w,
        &d_root,
        "0",
        &[("ACAD_GROUP", &d_group), ("ACAD_LAYOUT", &d_layout), ("ACAD_PLOTSTYLENAME", &d_plotstyle)],
    );
    dict(&mut w, &d_group, &d_root, &[]);
    dict(&mut w, &d_layout, &d_root, &[("Model", &lo_model), ("Layout1", &lo_paper)]);
    w.g(0, "ACDBDICTIONARYWDFLT");
    w.g(5, &d_plotstyle);
    w.g(330, &d_root);
    w.g(100, "AcDbDictionary");
    w.g(281, 1);
    w.g(3, "Normal");
    w.g(350, &placeholder);
    w.g(100, "AcDbDictionaryWithDefault");
    w.g(340, &placeholder);
    w.g(0, "ACDBPLACEHOLDER");
    w.g(5, &placeholder);
    w.g(330, &d_plotstyle);
    layout(&mut w, &lo_model, &d_layout, "Model", 0, scene.size, &br_model);
    layout(&mut w, &lo_paper, &d_layout, "Layout1", 1, scene.size, &br_paper);
    w.g(0, "ENDSEC");
    w.g(0, "EOF");

    // The header last: $HANDSEED must exceed every handle.
    let mut hd = W { s: String::new() };
    let (lo, hi) = extents(scene);
    hd.g(0, "SECTION");
    hd.g(2, "HEADER");
    hd.g(9, "$ACADVER");
    hd.g(1, "AC1015");
    hd.g(9, "$DWGCODEPAGE");
    hd.g(3, "ANSI_1252");
    hd.g(9, "$INSBASE");
    hd.xy(10, [0.0, 0.0]);
    hd.g(9, "$EXTMIN");
    hd.xy(10, lo);
    hd.g(9, "$EXTMAX");
    hd.xy(10, hi);
    hd.g(9, "$LIMMIN");
    hd.xy2(10, [0.0, 0.0]);
    hd.g(9, "$LIMMAX");
    hd.xy2(10, scene.size);
    hd.g(9, "$INSUNITS");
    hd.g(70, 4);
    hd.g(9, "$MEASUREMENT");
    hd.g(70, 1);
    hd.g(9, "$LUNITS");
    hd.g(70, 2);
    hd.g(9, "$LWDISPLAY");
    hd.g(290, 1);
    hd.g(9, "$HANDSEED");
    hd.g(5, h.next());
    hd.g(0, "ENDSEC");
    hd.g(0, "SECTION");
    hd.g(2, "CLASSES");
    hd.g(0, "ENDSEC");
    hd.s.push_str(&w.s);
    hd.s
}

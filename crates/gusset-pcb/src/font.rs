//! Gusset's own stroke font (ADR 0047 phase 3) for plotting board texts into Gerbers and
//! as copper obstacles: printable ASCII as polylines on a small grid, drawn with a round
//! pen of the text's thickness. KiCad's stroke font is not used (its licence was not
//! settled here); this one was drawn for Gusset and is MIT OR Apache-2.0 like the rest.
//!
//! Grid: x to the right from 0, y down from 0 (cap top) to 9 (baseline); lowercase from
//! 3; descenders to 12. A glyph's advance is its widest x plus 3.

use crate::board::{Pt, Text};

/// Glyphs for ' ' (32) to '~' (126): strokes split by `;`, points `x,y` split by spaces.
const GLYPHS: [&str; 95] = [
    "",                                                                    // ' '
    "0,0 0,6;0,9 0,9",                                                     // !
    "0,0 0,2;2,0 2,2",                                                     // "
    "1,1 1,8;4,1 4,8;0,3 5,3;0,6 5,6",                                     // #
    "6,2 5,1 1,1 0,2 0,3 1,4 5,4 6,5 6,7 5,8 1,8 0,7;3,0 3,9",             // $
    "0,9 6,0;0,0 1,0 1,1 0,1 0,0;5,8 6,8 6,9 5,9 5,8",                     // %
    "6,9 1,4 1,1 2,0 3,0 4,1 4,2 0,6 0,8 1,9 3,9 6,6",                     // &
    "0,0 0,2",                                                             // '
    "2,0 0,2 0,7 2,9",                                                     // (
    "0,0 2,2 2,7 0,9",                                                     // )
    "2,1 2,7;0,2 4,6;4,2 0,6",                                             // *
    "3,2 3,8;0,5 6,5",                                                     // +
    "1,8 1,9 0,10",                                                        // ,
    "0,5 5,5",                                                             // -
    "0,9 0,9",                                                             // .
    "0,9 5,0",                                                             // /
    "1,0 5,0 6,1 6,8 5,9 1,9 0,8 0,1 1,0",                                 // 0
    "1,2 3,0 3,9;1,9 5,9",                                                 // 1
    "0,1 1,0 5,0 6,1 6,3 0,9 6,9",                                         // 2
    "0,1 1,0 5,0 6,1 6,3 5,4 2,4;5,4 6,5 6,8 5,9 1,9 0,8",                 // 3
    "5,9 5,0 0,6 6,6",                                                     // 4
    "6,0 0,0 0,4 5,4 6,5 6,8 5,9 1,9 0,8",                                 // 5
    "5,0 2,0 0,2 0,8 1,9 5,9 6,8 6,5 5,4 0,4",                             // 6
    "0,0 6,0 2,9",                                                         // 7
    "1,0 5,0 6,1 6,3 5,4 1,4 0,3 0,1 1,0;1,4 0,5 0,8 1,9 5,9 6,8 6,5 5,4", // 8
    "6,5 1,5 0,4 0,1 1,0 5,0 6,1 6,7 4,9 1,9",                             // 9
    "0,3 0,3;0,9 0,9",                                                     // :
    "1,3 1,3;1,8 1,9 0,10",                                                // ;
    "6,1 0,5 6,9",                                                         // <
    "0,3 6,3;0,7 6,7",                                                     // =
    "0,1 6,5 0,9",                                                         // >
    "0,1 1,0 5,0 6,1 6,3 3,5 3,6;3,9 3,9",                                 // ?
    "5,6 5,3 2,3 1,4 1,5 2,6 5,6 6,5 6,1 5,0 1,0 0,1 0,8 1,9 6,9",         // @
    "0,9 3,0 6,9;1,6 5,6",                                                 // A
    "0,0 0,9 5,9 6,8 6,5 5,4 0,4;0,0 4,0 5,1 5,3 4,4",                     // B
    "6,1 5,0 1,0 0,1 0,8 1,9 5,9 6,8",                                     // C
    "0,0 0,9 4,9 6,7 6,2 4,0 0,0",                                         // D
    "6,0 0,0 0,9 6,9;0,4 4,4",                                             // E
    "6,0 0,0 0,9;0,4 4,4",                                                 // F
    "6,1 5,0 1,0 0,1 0,8 1,9 5,9 6,8 6,5 3,5",                             // G
    "0,0 0,9;6,0 6,9;0,4 6,4",                                             // H
    "1,0 5,0;3,0 3,9;1,9 5,9",                                             // I
    "6,0 6,8 5,9 1,9 0,8 0,7",                                             // J
    "0,0 0,9;6,0 0,6;2,4 6,9",                                             // K
    "0,0 0,9 6,9",                                                         // L
    "0,9 0,0 3,5 6,0 6,9",                                                 // M
    "0,9 0,0 6,9 6,0",                                                     // N
    "1,0 5,0 6,1 6,8 5,9 1,9 0,8 0,1 1,0",                                 // O
    "0,9 0,0 5,0 6,1 6,4 5,5 0,5",                                         // P
    "1,0 5,0 6,1 6,8 5,9 1,9 0,8 0,1 1,0;4,7 6,9",                         // Q
    "0,9 0,0 5,0 6,1 6,4 5,5 0,5;3,5 6,9",                                 // R
    "6,1 5,0 1,0 0,1 0,3 1,4 5,4 6,5 6,8 5,9 1,9 0,8",                     // S
    "0,0 6,0;3,0 3,9",                                                     // T
    "0,0 0,8 1,9 5,9 6,8 6,0",                                             // U
    "0,0 3,9 6,0",                                                         // V
    "0,0 1,9 3,4 5,9 6,0",                                                 // W
    "0,0 6,9;6,0 0,9",                                                     // X
    "0,0 3,4 6,0;3,4 3,9",                                                 // Y
    "0,0 6,0 0,9 6,9",                                                     // Z
    "2,0 0,0 0,9 2,9",                                                     // [
    "0,0 5,9",                                                             // backslash
    "0,0 2,0 2,9 0,9",                                                     // ]
    "0,3 3,0 6,3",                                                         // ^
    "0,11 6,11",                                                           // _
    "0,0 1,1",                                                             // `
    "0,3 5,3 6,4 6,9;6,6 1,6 0,7 0,8 1,9 6,9",                             // a
    "0,0 0,9;0,4 1,3 5,3 6,4 6,8 5,9 1,9 0,8",                             // b
    "6,3 1,3 0,4 0,8 1,9 6,9",                                             // c
    "6,0 6,9;6,4 5,3 1,3 0,4 0,8 1,9 5,9 6,8",                             // d
    "0,6 6,6 6,4 5,3 1,3 0,4 0,8 1,9 6,9",                                 // e
    "5,0 3,0 2,1 2,9;0,3 5,3",                                             // f
    "6,3 6,11 5,12 1,12;6,4 5,3 1,3 0,4 0,8 1,9 5,9 6,8",                  // g
    "0,0 0,9;0,4 1,3 5,3 6,4 6,9",                                         // h
    "0,3 0,9;0,1 0,1",                                                     // i
    "2,3 2,11 1,12 0,12;2,1 2,1",                                          // j
    "0,0 0,9;5,3 0,7;2,6 5,9",                                             // k
    "0,0 0,8 1,9",                                                         // l
    "0,9 0,3;0,4 1,3 2,3 3,4 3,9;3,4 4,3 5,3 6,4 6,9",                     // m
    "0,9 0,3;0,4 1,3 5,3 6,4 6,9",                                         // n
    "1,3 5,3 6,4 6,8 5,9 1,9 0,8 0,4 1,3",                                 // o
    "0,3 0,12;0,4 1,3 5,3 6,4 6,8 5,9 1,9 0,8",                            // p
    "6,3 6,12;6,4 5,3 1,3 0,4 0,8 1,9 5,9 6,8",                            // q
    "0,3 0,9;0,5 2,3 5,3",                                                 // r
    "6,3 1,3 0,4 0,5 1,6 5,6 6,7 6,8 5,9 0,9",                             // s
    "2,0 2,8 3,9 5,9;0,3 5,3",                                             // t
    "0,3 0,8 1,9 5,9 6,8;6,3 6,9",                                         // u
    "0,3 3,9 6,3",                                                         // v
    "0,3 1,9 3,5 5,9 6,3",                                                 // w
    "0,3 6,9;6,3 0,9",                                                     // x
    "0,3 3,9;6,3 3,9 2,11 1,12 0,12",                                      // y
    "0,3 6,3 0,9 6,9",                                                     // z
    "3,0 2,0 1,1 1,3 0,4 1,5 1,8 2,9 3,9",                                 // {
    "0,0 0,11",                                                            // |
    "0,0 1,0 2,1 2,3 3,4 2,5 2,8 1,9 0,9",                                 // }
    "0,5 1,4 2,4 4,6 5,6 6,5",                                             // ~
];

/// A character the font lacks: a box.
const MISSING: &str = "0,0 6,0 6,9 0,9 0,0";
/// Grid units from cap top to baseline.
const CAP: f64 = 9.0;
/// Most characters plotted from one text.
const MAX_CHARS: usize = 2000;

/// The strokes of one character on the grid, and its advance in grid units.
fn glyph(c: char) -> (Vec<Vec<[f64; 2]>>, f64) {
    let src = match c {
        ' '..='~' => GLYPHS.get(c as usize - 32).copied().unwrap_or(MISSING),
        _ => MISSING,
    };
    let strokes: Vec<Vec<[f64; 2]>> = src
        .split(';')
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.split(' ')
                .filter_map(|p| p.split_once(','))
                .filter_map(|(x, y)| Some([x.parse::<f64>().ok()?, y.parse::<f64>().ok()?]))
                .collect()
        })
        .collect();
    let wide = strokes.iter().flatten().fold(0.0f64, |m, p| m.max(p[0]));
    let advance = if c == ' ' { 5.0 } else { wide + 3.0 };
    (strokes, advance)
}

/// The text a board text shows: KiCad's `${REFERENCE}`/`${VALUE}` (and KiCad 5's `%R`,
/// `%V`) replaced by the footprint's.
pub fn shown(t: &str, reference: &str, value: &str) -> String {
    t.replace("${REFERENCE}", reference).replace("${VALUE}", value).replace("%R", reference).replace("%V", value)
}

/// The pen thickness a text is plotted with: its own, else an eighth of its height.
pub fn thickness(t: &Text) -> f64 {
    if t.thickness > 0.0 { t.thickness } else { t.size / 8.0 }
}

/// The polylines of `text` placed as `t` says (its place, height, width, angle,
/// justification, mirroring), in board coordinates (mm, Y down). Lines split at `\n`.
pub fn strokes(t: &Text, text: &str) -> Vec<Vec<Pt>> {
    let h = t.size / CAP;
    let w = if t.width > 0.0 { t.width } else { t.size } / CAP;
    let lines: Vec<Vec<char>> = text.split('\n').map(|l| l.chars().filter(|c| !c.is_control()).collect()).collect();
    let pitch = 1.6 * t.size;
    let n = lines.len().max(1) as f64;
    // Vertical: the block's top (cap of the first line) relative to the anchor.
    let block = t.size + (n - 1.0) * pitch;
    let top = match t.justify[1] {
        -1 => 0.0,
        1 => -block,
        _ => -block / 2.0,
    };
    let a = t.angle.to_radians();
    let (ca, sa) = (libm::cos(a), libm::sin(a));
    let mirror = if t.mirrored { -1.0 } else { 1.0 };
    // Counter-clockwise on screen with Y down.
    let place = |x: f64, y: f64| {
        let x = x * mirror;
        [t.at[0] + x * ca + y * sa, t.at[1] - x * sa + y * ca]
    };
    let mut out = Vec::new();
    let mut count = 0usize;
    for (k, line) in lines.iter().enumerate() {
        let glyphs: Vec<(Vec<Vec<[f64; 2]>>, f64)> = line.iter().map(|&c| glyph(c)).collect();
        // The last glyph's trailing space is not part of the line's width.
        let width = (glyphs.iter().map(|g| g.1).sum::<f64>() - 3.0).max(0.0) * w;
        let left = match t.justify[0] {
            -1 => 0.0,
            1 => -width,
            _ => -width / 2.0,
        };
        let y0 = top + k as f64 * pitch;
        let mut x0 = left;
        for (strokes, adv) in glyphs {
            count += 1;
            if count > MAX_CHARS {
                return out;
            }
            for s in strokes {
                out.push(s.iter().map(|p| place(x0 + p[0] * w, y0 + p[1] * h)).collect());
            }
            x0 += adv * w;
        }
    }
    out
}

/// The pen strokes of every shown text on `layer`: (start, end, pen radius), board
/// coordinates. Copper texts are obstacles to fills and routes.
pub fn layer_strokes(board: &crate::board::Board, layer: &str) -> Vec<(Pt, Pt, f64)> {
    let mut out = Vec::new();
    for t in board.texts.iter().filter(|t| t.layer == layer && !t.hidden) {
        let fp = t.footprint.and_then(|i| board.footprints.get(i));
        let text = shown(&t.text, fp.map_or("", |f| &f.reference), fp.map_or("", |f| &f.value));
        let r = thickness(t) / 2.0;
        for line in strokes(t, &text) {
            match line.as_slice() {
                [p] => out.push((*p, *p, r)),
                l => out.extend(l.windows(2).filter_map(|w| match w {
                    [a, b] => Some((*a, *b, r)),
                    _ => None,
                })),
            }
        }
    }
    out
}

/// The width and height of a one-line text without its pen (mm), for tests and layout.
pub fn extent(text: &str, size: f64) -> [f64; 2] {
    let w = size / CAP;
    let adv: f64 = text.chars().map(|c| glyph(c).1).sum();
    [(adv - 3.0).max(0.0) * w, size]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_glyph_is_on_its_grid() {
        for c in ' '..='~' {
            let (s, adv) = glyph(c);
            assert!(adv >= 3.0, "{c}");
            for p in s.iter().flatten() {
                assert!((0.0..=6.0).contains(&p[0]) && (0.0..=12.0).contains(&p[1]), "{c}: {p:?}");
            }
            assert!(c == ' ' || !s.is_empty(), "{c} has no strokes");
            assert!(s.iter().all(|x| !x.is_empty()), "{c}");
        }
    }
}

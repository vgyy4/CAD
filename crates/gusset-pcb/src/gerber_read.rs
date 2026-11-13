#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::print_stderr)]
//! A permissive Gerber re-reader for tests: it parses what [`crate::gerber`] writes (format
//! and unit statements, aperture definitions and macros, D01/D02/D03, regions, arcs under
//! G75, attributes) from the public Ucamco format specification, and checks the file
//! against the board it came from. Not a viewer: it keeps objects, not pixels.

use crate::board::{Board, PadKind};
use std::collections::{BTreeMap, BTreeSet, HashMap};

type R<T> = Result<T, String>;

/// An aperture definition.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Ap {
    Circle(f64),
    Rect(f64, f64),
    Obround(f64, f64),
    Polygon,
    /// A macro aperture by name.
    Macro(String),
}

/// What an object is.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Kind {
    Draw {
        ap: i32,
        a: [i64; 2],
        b: [i64; 2],
    },
    Arc {
        ap: i32,
        a: [i64; 2],
        b: [i64; 2],
        off: [i64; 2],
        ccw: bool,
    },
    Flash {
        ap: i32,
        at: [i64; 2],
    },
    /// Contours of a region, as points (arcs keep their end points).
    Region {
        contours: Vec<Vec<[i64; 2]>>,
    },
}

/// A graphical object with the attributes in force when it was made.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Obj {
    pub(crate) kind: Kind,
    /// Object attributes (`.N`, `.P`, `.C`), unescaped.
    pub(crate) attrs: BTreeMap<String, Vec<String>>,
    /// The `.AperFunction` value (first field) of its aperture, or in force for a region.
    pub(crate) func: Option<String>,
}

impl Obj {
    pub(crate) fn net(&self) -> Option<&str> {
        self.attrs.get(".N").and_then(|v| v.first()).map(String::as_str)
    }

    pub(crate) fn pin(&self) -> Option<(&str, &str)> {
        let v = self.attrs.get(".P")?;
        Some((v.first()?, v.get(1)?))
    }

    pub(crate) fn comp(&self) -> Option<&str> {
        self.attrs.get(".C").and_then(|v| v.first()).map(String::as_str)
    }
}

#[derive(Debug, Default)]
pub(crate) struct Parsed {
    /// Millimetres per coordinate unit.
    pub(crate) scale: f64,
    pub(crate) apertures: HashMap<i32, (Ap, Option<String>)>,
    pub(crate) file_attrs: BTreeMap<String, Vec<String>>,
    pub(crate) objects: Vec<Obj>,
    pub(crate) macros: Vec<String>,
    pub(crate) ended: bool,
}

/// Decodes a field: `\uXXXX` escapes (UTF-16 units) back to characters.
pub(crate) fn unescape(t: &str) -> R<String> {
    let mut units: Vec<u16> = Vec::new();
    let mut out = String::new();
    let flush = |units: &mut Vec<u16>, out: &mut String| -> R<()> {
        if !units.is_empty() {
            out.push_str(&String::from_utf16(units).map_err(|e| format!("bad surrogates in {t:?}: {e}"))?);
            units.clear();
        }
        Ok(())
    };
    let mut it = t.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            if it.next() != Some('u') {
                return Err(format!("bad escape in {t:?}"));
            }
            let hex: String = (0..4).filter_map(|_| it.next()).collect();
            units.push(u16::from_str_radix(&hex, 16).map_err(|_| format!("bad \\u in {t:?}"))?);
        } else {
            flush(&mut units, &mut out)?;
            if matches!(c, '%' | '*') || c.is_control() {
                return Err(format!("raw delimiter or control character in {t:?}"));
            }
            out.push(c);
        }
    }
    flush(&mut units, &mut out)?;
    Ok(out)
}

fn fields(s: &str) -> R<(String, Vec<String>)> {
    let mut it = s.split(',');
    let name = it.next().unwrap_or("").to_string();
    let vals = it.map(unescape).collect::<R<Vec<_>>>()?;
    Ok((name, vals))
}

fn number(s: &str) -> R<f64> {
    s.parse::<f64>().map_err(|_| format!("bad number {s:?}"))
}

/// Splits `X123Y-45D01` into letter/number pairs.
fn letters(w: &str) -> R<Vec<(char, i64)>> {
    let mut out = Vec::new();
    let b: Vec<char> = w.chars().collect();
    let mut i = 0;
    while i < b.len() {
        let l = b[i];
        if !l.is_ascii_alphabetic() {
            return Err(format!("unexpected {l:?} in {w:?}"));
        }
        let mut j = i + 1;
        while j < b.len() && (b[j].is_ascii_digit() || (j == i + 1 && matches!(b[j], '-' | '+'))) {
            j += 1;
        }
        let n: String = b[i + 1..j].iter().collect();
        out.push((l, n.parse::<i64>().map_err(|_| format!("bad number in {w:?}"))?));
        i = j;
    }
    Ok(out)
}

/// Parses a Gerber file.
pub(crate) fn parse(text: &str) -> R<Parsed> {
    let mut p = Parsed::default();
    // Statements: `%...%` blocks (several words for a macro) and plain `word*`.
    let mut blocks: Vec<(bool, Vec<String>)> = Vec::new();
    let mut rest = text;
    while let Some(c) = rest.trim_start().chars().next() {
        rest = rest.trim_start();
        if c == '%' {
            let end = rest[1..].find('%').ok_or("unterminated % block")? + 1;
            let words: Vec<String> = rest[1..end].split('*').map(|w| w.trim().to_string()).collect();
            if words.last().is_none_or(|w| !w.is_empty()) {
                return Err(format!("extended block without * before %: {:?}", &rest[..=end]));
            }
            blocks.push((true, words.into_iter().filter(|w| !w.is_empty()).collect()));
            rest = &rest[end + 1..];
        } else {
            let end = rest.find('*').ok_or("word without *")?;
            blocks.push((false, vec![rest[..end].trim().to_string()]));
            rest = &rest[end + 1..];
        }
    }
    let mut fmt_seen = false;
    let mut digits = (0usize, 0usize);
    let mut unit_mm = None;
    let mut ap_state: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut obj_state: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let (mut cur_ap, mut mode, mut quadrant, mut in_region) = (None::<i32>, 1u8, false, false);
    let mut pos = [0i64; 2];
    let mut region: Vec<Vec<[i64; 2]>> = Vec::new();
    let mut region_func = None;
    let mut started = false;
    for (ext, words) in blocks {
        if p.ended {
            return Err("content after M02".into());
        }
        if ext {
            let w = words.first().cloned().unwrap_or_default();
            if let Some(rest) = w.strip_prefix("FSLA") {
                let (xs, ys) = rest.split_once('Y').ok_or("bad FS")?;
                let xs = xs.strip_prefix('X').ok_or("bad FS")?;
                let d = |s: &str| -> R<(usize, usize)> {
                    let b = s.as_bytes();
                    if b.len() != 2 || !b.iter().all(u8::is_ascii_digit) {
                        return Err(format!("bad FS digits {s:?}"));
                    }
                    Ok(((b[0] - b'0') as usize, (b[1] - b'0') as usize))
                };
                let (dx, dy) = (d(xs)?, d(ys)?);
                if dx != dy || dx.0 < 1 || dx.1 < 1 || dx.0 > 6 || dx.1 > 6 {
                    return Err(format!("unsupported FS {w}"));
                }
                digits = dx;
                fmt_seen = true;
            } else if let Some(u) = w.strip_prefix("MO") {
                unit_mm = Some(match u {
                    "MM" => 1.0,
                    "IN" => 25.4,
                    _ => return Err(format!("bad unit {u}")),
                });
            } else if let Some(m) = w.strip_prefix("AM") {
                p.macros.push(m.to_string());
            } else if let Some(d) = w.strip_prefix("ADD") {
                let i = d.find(|c: char| !c.is_ascii_digit()).ok_or("bad AD")?;
                let code: i32 = d[..i].parse().map_err(|_| "bad D code")?;
                if code < 10 {
                    return Err(format!("aperture code {code} below 10"));
                }
                let (t, params) = d[i..].split_once(',').unwrap_or((&d[i..], ""));
                let nums: Vec<f64> =
                    if params.is_empty() { vec![] } else { params.split('X').map(number).collect::<R<_>>()? };
                let ap = match (t, nums.as_slice()) {
                    ("C", [d, ..]) => Ap::Circle(*d),
                    ("R", [w, h, ..]) => Ap::Rect(*w, *h),
                    ("O", [w, h, ..]) => Ap::Obround(*w, *h),
                    ("P", [_, _, ..]) => Ap::Polygon,
                    (name, _) if p.macros.iter().any(|m| m == name || m.starts_with(&format!("{name}*"))) => {
                        Ap::Macro(name.to_string())
                    }
                    _ => return Err(format!("bad aperture {w}")),
                };
                let func = ap_state.get(".AperFunction").and_then(|v| v.first().cloned());
                if p.apertures.insert(code, (ap, func)).is_some() {
                    return Err(format!("aperture D{code} defined twice"));
                }
            } else if let Some(a) = w.strip_prefix("TF") {
                if started {
                    return Err("file attribute after the first object".into());
                }
                let (n, v) = fields(a)?;
                p.file_attrs.insert(n, v);
            } else if let Some(a) = w.strip_prefix("TA") {
                let (n, v) = fields(a)?;
                ap_state.insert(n, v);
            } else if let Some(a) = w.strip_prefix("TO") {
                let (n, v) = fields(a)?;
                obj_state.insert(n, v);
            } else if let Some(a) = w.strip_prefix("TD") {
                if a.is_empty() {
                    ap_state.clear();
                    obj_state.clear();
                } else {
                    ap_state.remove(a);
                    obj_state.remove(a);
                }
            } else if w == "LPD" || w == "LPC" {
            } else {
                return Err(format!("unknown extended statement {w:?}"));
            }
            continue;
        }
        let w = &words[0];
        let (g, rest) = match w.strip_prefix('G') {
            Some(r) => {
                let n = r.find(|c: char| !c.is_ascii_digit()).unwrap_or(r.len());
                if r[..n].parse::<u32>().is_ok_and(|g| g == 4) {
                    continue; // comment
                }
                (r[..n].parse::<u32>().ok(), &r[n..])
            }
            None => (None, w.as_str()),
        };
        match g {
            Some(1) => mode = 1,
            Some(2) => mode = 2,
            Some(3) => mode = 3,
            Some(75) => quadrant = true,
            Some(74) => return Err("G74 single quadrant mode".into()),
            Some(36) => {
                in_region = true;
                region = Vec::new();
                region_func = ap_state.get(".AperFunction").and_then(|v| v.first().cloned());
            }
            Some(37) => {
                if !in_region {
                    return Err("G37 without G36".into());
                }
                in_region = false;
                started = true;
                p.objects.push(Obj {
                    kind: Kind::Region { contours: std::mem::take(&mut region) },
                    attrs: obj_state.clone(),
                    func: region_func.take(),
                });
            }
            Some(n) => return Err(format!("unsupported G{n}")),
            None => {}
        }
        if rest.is_empty() {
            continue;
        }
        if rest == "M02" {
            p.ended = true;
            continue;
        }
        let items = letters(rest)?;
        if let [('D', n)] = items.as_slice()
            && *n >= 10
        {
            let n = *n as i32;
            if !p.apertures.contains_key(&n) {
                return Err(format!("D{n} selected but not defined"));
            }
            cur_ap = Some(n);
            continue;
        }
        let (mut x, mut y, mut i, mut j, mut d) = (None, None, None, None, None);
        for (l, v) in items {
            match l {
                'X' => x = Some(v),
                'Y' => y = Some(v),
                'I' => i = Some(v),
                'J' => j = Some(v),
                'D' => d = Some(v),
                _ => return Err(format!("unknown word {w:?}")),
            }
        }
        let from = pos;
        pos = [x.unwrap_or(pos[0]), y.unwrap_or(pos[1])];
        let limit = 9999.0 * 10f64.powi(digits.1 as i32);
        if pos.iter().any(|v| (*v as f64).abs() > limit) {
            return Err(format!("coordinate out of range in {w:?}"));
        }
        let off = [i.unwrap_or(0), j.unwrap_or(0)];
        match d {
            Some(2) => {
                if in_region {
                    region.push(vec![pos]);
                }
            }
            Some(1) => {
                let arc = mode != 1;
                if arc && !quadrant {
                    return Err("arc without G75".into());
                }
                if in_region {
                    region.last_mut().ok_or("region draw before D02")?.push(pos);
                    started = true;
                } else {
                    let ap = cur_ap.ok_or("draw without aperture")?;
                    started = true;
                    let kind = if arc {
                        Kind::Arc { ap, a: from, b: pos, off, ccw: mode == 3 }
                    } else {
                        Kind::Draw { ap, a: from, b: pos }
                    };
                    p.objects.push(Obj { kind, attrs: obj_state.clone(), func: aper_func(&p, ap) });
                }
            }
            Some(3) => {
                if in_region {
                    return Err("flash in a region".into());
                }
                let ap = cur_ap.ok_or("flash without aperture")?;
                started = true;
                p.objects.push(Obj {
                    kind: Kind::Flash { ap, at: pos },
                    attrs: obj_state.clone(),
                    func: aper_func(&p, ap),
                });
            }
            _ => return Err(format!("operation without D01/D02/D03: {w:?}")),
        }
    }
    if !p.ended {
        return Err("no M02".into());
    }
    if !fmt_seen {
        return Err("no format statement".into());
    }
    let unit = unit_mm.ok_or("no unit statement")?;
    p.scale = unit / 10f64.powi(digits.1 as i32);
    Ok(p)
}

fn aper_func(p: &Parsed, ap: i32) -> Option<String> {
    p.apertures.get(&ap).and_then(|a| a.1.clone())
}

/// Parses and checks internal consistency: every object on a defined aperture, closed
/// region contours, arcs with matching radii, coordinates in range.
pub(crate) fn check_file(text: &str) -> R<Parsed> {
    let p = parse(text)?;
    for o in &p.objects {
        match &o.kind {
            Kind::Draw { ap, .. } | Kind::Arc { ap, .. } | Kind::Flash { ap, .. } => {
                if !p.apertures.contains_key(ap) {
                    return Err(format!("object on undefined aperture D{ap}"));
                }
            }
            Kind::Region { contours } => {
                if contours.is_empty() {
                    return Err("empty region".into());
                }
                for c in contours {
                    if c.len() < 3 || c.first() != c.last() {
                        return Err(format!("region contour not closed: {c:?}"));
                    }
                }
            }
        }
        if let Kind::Arc { a, b, off, .. } = &o.kind {
            let r0 = libm::hypot(off[0] as f64, off[1] as f64);
            let r1 = libm::hypot((b[0] - a[0] - off[0]) as f64, (b[1] - a[1] - off[1]) as f64);
            // Offsets are rounded to the coordinate grid.
            if (r0 - r1).abs() > 3.0 {
                return Err(format!("arc radii differ: {r0} vs {r1}"));
            }
        }
    }
    if !p.file_attrs.contains_key(".FileFunction") {
        return Err("no FileFunction".into());
    }
    Ok(p)
}

/// [`check_file`], then the objects against the board's items on `layer`.
pub(crate) fn verify(board: &Board, layer: &str, text: &str) -> R<()> {
    let p = check_file(text)?;
    let copper = board.copper_layers().iter().any(|l| l == layer);
    let refs: BTreeSet<&str> = board.footprints.iter().map(|f| f.reference.as_str()).collect();
    for o in &p.objects {
        if !copper && (o.net().is_some() || o.pin().is_some()) {
            return Err(format!("net or pad attribute on non-copper layer {layer}"));
        }
        if let Some(c) = o.comp()
            && !refs.contains(c)
        {
            return Err(format!(".C {c:?} is no footprint"));
        }
    }
    if !copper {
        return Ok(());
    }
    let sorted = |mut v: Vec<Option<String>>| {
        v.sort();
        v
    };
    let actual = |f: &dyn Fn(&Obj) -> bool| -> Vec<Option<String>> {
        sorted(p.objects.iter().filter(|o| f(o)).map(|o| o.net().map(str::to_string)).collect())
    };
    let tracks: Vec<_> = board.tracks.iter().filter(|t| t.layer == layer && t.width > 0.0).collect();
    let want = sorted(tracks.iter().map(|t| board.net_name(&t.net)).collect());
    let got = actual(&|o| o.func.as_deref() == Some("Conductor") && !matches!(o.kind, Kind::Region { .. }));
    if want != got {
        return Err(format!("track nets differ on {layer}: board {want:?}, file {got:?}"));
    }
    let want = sorted(
        board
            .vias
            .iter()
            .filter(|v| v.size > 0.0 && board.span(&v.layers[0], &v.layers[1]).iter().any(|l| l == layer))
            .map(|v| board.net_name(&v.net))
            .collect(),
    );
    let got = actual(&|o| o.func.as_deref() == Some("ViaPad"));
    if want != got {
        return Err(format!("via nets differ on {layer}: board {want:?}, file {got:?}"));
    }
    let want = sorted(
        board
            .zones
            .iter()
            .filter(|z| !z.keepout)
            .flat_map(|z| z.fills.iter().filter(|f| f.0 == layer && f.1.len() >= 3).map(|_| board.net_name(&z.net)))
            .collect(),
    );
    let got = actual(&|o| o.func.as_deref() == Some("Conductor") && matches!(o.kind, Kind::Region { .. }));
    if want != got {
        return Err(format!("zone fill nets differ on {layer}: board {want:?}, file {got:?}"));
    }
    // Pads: (reference, number, net) triples.
    let mut want = BTreeSet::new();
    for f in &board.footprints {
        for &pi in &f.pads {
            let Some(pad) = board.pads.get(pi) else { continue };
            if !board.on_layer(&pad.layers, layer) || pad.size[0] <= 0.0 || pad.size[1] <= 0.0 {
                continue;
            }
            if pad.kind == PadKind::Npth && pad.drill.is_some_and(|d| d.size[0] >= pad.size[0] - 1e-9) {
                continue;
            }
            if !f.reference.is_empty() && !pad.number.is_empty() {
                want.insert((
                    f.reference.clone(),
                    pad.number.clone(),
                    board.net_name(&pad.net).unwrap_or_else(|| "N/C".into()),
                ));
            }
        }
    }
    let mut got = BTreeSet::new();
    for o in p.objects.iter().filter(|o| matches!(o.func.as_deref(), Some("SMDPad" | "ComponentPad"))) {
        let Some(net) = o.net() else { return Err("a pad object without .N".into()) };
        if let Some((r, n)) = o.pin() {
            if o.comp() != Some(r) {
                return Err(format!(".P {r} without matching .C"));
            }
            got.insert((r.to_string(), n.to_string(), net.to_string()));
        }
    }
    if want != got {
        let miss: Vec<_> = want.difference(&got).take(5).collect();
        let extra: Vec<_> = got.difference(&want).take(5).collect();
        return Err(format!("pad attributes differ on {layer}: missing {miss:?}, extra {extra:?}"));
    }
    Ok(())
}

//! The canonical text form of a document: the code view and "Export as text" (ADR 0004).
//!
//! The language is line-oriented and declarative: one statement per line, with sketch
//! bodies in `{ ... }` blocks. [`print`] produces canonical text; [`parse`] accepts
//! canonical text plus harmless variations (extra spaces, blank lines, comments).
//! `parse(print(doc)) == doc` holds for every valid document.

use crate::cursor::{Cursor, ParseError, quote};
use crate::model::*;
use crate::refs::{EdgeRef, FaceRef, parse_edge, parse_face};
use gusset_units::LengthUnit;
use gusset_units::params::Param;
use gusset_units::quantity::format_exact;
use std::fmt::Write as _;

/// Hostile-input limits.
pub const MAX_TEXT_BYTES: usize = 64 << 20;
pub const MAX_LINES: usize = 2_000_000;
pub const MAX_FEATURES: usize = 100_000;
pub const MAX_SKETCH_ITEMS: usize = 100_000;
pub const MAX_LIST_ITEMS: usize = 10_000;

// ------------------------------------------------------------------ printing

/// Prints the canonical text of a document.
pub fn print(doc: &Document) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "gusset {SCHEMA_VERSION}");
    let _ = writeln!(
        out,
        "units {}",
        match doc.units {
            LengthUnit::Millimetre => "mm",
            LengthUnit::Inch => "in",
        }
    );
    if !doc.params.is_empty() {
        out.push('\n');
        for p in &doc.params {
            print_param(&mut out, p);
        }
    }
    for f in &doc.features {
        out.push('\n');
        print_feature(&mut out, f);
    }
    out
}

fn print_comment_lines(out: &mut String, text: &str) {
    for line in text.split('\n') {
        if line.is_empty() {
            out.push_str("#\n");
        } else {
            let _ = writeln!(out, "# {line}");
        }
    }
}

fn print_param(out: &mut String, p: &Param) {
    let single = !p.comment.is_empty() && !p.comment.contains('\n');
    if !p.comment.is_empty() && !single {
        print_comment_lines(out, &p.comment);
    }
    let _ = write!(out, "param {} = {}", p.name, p.expr);
    if single {
        let _ = write!(out, "  # {}", p.comment);
    }
    out.push('\n');
}

fn print_profile(p: &ProfileRef) -> String {
    if p.regions.is_empty() { p.sketch.clone() } else { format!("{}.regions({})", p.sketch, p.regions.join(", ")) }
}

fn print_common(f: &Feature, args: &mut Vec<String>) {
    if let Some(l) = &f.label {
        args.push(format!("label: {}", quote(l)));
    }
    if f.suppressed {
        args.push("suppressed: true".into());
    }
}

fn print_feature(out: &mut String, f: &Feature) {
    if let Some(n) = &f.note {
        print_comment_lines(out, n);
    }
    let mut args: Vec<String> = Vec::new();
    match &f.kind {
        FeatureKind::Sketch(s) => {
            let plane = match &s.plane {
                PlaneRef::XY => "XY".to_string(),
                PlaneRef::XZ => "XZ".to_string(),
                PlaneRef::YZ => "YZ".to_string(),
                PlaneRef::Face(r) => r.to_string(),
            };
            args.push(format!("plane: {plane}"));
            args.push(format!("next_id: {}", s.next_id));
            print_common(f, &mut args);
            let _ = writeln!(out, "sketch {} ({}) {{", f.name, args.join(", "));
            for e in &s.entities {
                let body = match &e.geom {
                    SkGeom::Point { x, y } => format!("point({}, {})", format_exact(*x), format_exact(*y)),
                    SkGeom::Line { a, b } => format!("line({a}, {b})"),
                    SkGeom::Circle { center, radius } => format!("circle({center}, {})", format_exact(*radius)),
                    SkGeom::Arc { center, start, end } => format!("arc({center}, {start}, {end})"),
                };
                let _ = writeln!(out, "  {} = {}{}", e.id, body, if e.construction { " construction" } else { "" });
            }
            for c in &s.constraints {
                let mut parts: Vec<String> = c.entities.clone();
                if let Some(v) = &c.value {
                    parts.push(v.to_string());
                }
                let _ = writeln!(out, "  {} = {}({})", c.id, c.kind.keyword(), parts.join(", "));
            }
            out.push_str("}\n");
            return;
        }
        FeatureKind::Extrude(e) => {
            args.push(format!("profile: {}", print_profile(&e.profile)));
            args.push(format!("distance: {}", e.distance));
            if e.symmetric {
                args.push("symmetric: true".into());
            }
            args.push(format!("op: {}", e.op.keyword()));
            if let Some(b) = &e.body {
                args.push(format!("body: {b}"));
            }
            if let Some(b) = &e.body_name {
                args.push(format!("body_name: {b}"));
            }
        }
        FeatureKind::Revolve(r) => {
            args.push(format!("profile: {}", print_profile(&r.profile)));
            let axis = match &r.axis {
                AxisRef::X => "X".to_string(),
                AxisRef::Y => "Y".to_string(),
                AxisRef::Z => "Z".to_string(),
                AxisRef::SketchLine { sketch, entity } => format!("{sketch}.{entity}"),
            };
            args.push(format!("axis: {axis}"));
            args.push(format!("angle: {}", r.angle));
            args.push(format!("op: {}", r.op.keyword()));
            if let Some(b) = &r.body {
                args.push(format!("body: {b}"));
            }
            if let Some(b) = &r.body_name {
                args.push(format!("body_name: {b}"));
            }
        }
        FeatureKind::Fillet(b) | FeatureKind::Chamfer(b) => {
            let edges: Vec<String> = b.edges.iter().map(EdgeRef::to_string).collect();
            args.push(format!("edges: [{}]", edges.join(", ")));
            let key = if matches!(f.kind, FeatureKind::Fillet(_)) { "radius" } else { "distance" };
            args.push(format!("{key}: {}", b.size));
        }
        FeatureKind::Import(i) => {
            args.push(format!("blob: {}", quote(&i.blob)));
            args.push(format!("format: {}", i.format.keyword()));
            args.push(format!("file: {}", quote(&i.file_name)));
        }
    }
    print_common(f, &mut args);
    let _ = writeln!(out, "{} {} ({})", f.kind.keyword(), f.name, args.join(", "));
}

// ------------------------------------------------------------------ parsing

/// Splits a line into (code, trailing comment). `#` inside strings and directly after
/// a reference's `)` (disambiguation index) is not a comment.
fn split_comment(line: &str) -> (&str, Option<&str>) {
    let mut in_str = false;
    let mut esc = false;
    let mut prev = ' ';
    for (i, c) in line.char_indices() {
        if in_str {
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
        } else if c == '"' {
            in_str = true;
        } else if c == '#' && prev != ')' {
            let code = line.get(..i).unwrap_or("");
            let comment = line.get(i + 1..).unwrap_or("");
            let comment = comment.strip_prefix(' ').unwrap_or(comment);
            // A trailing `\r` would print before the `\n` and then be read as part of a
            // CRLF line ending, so a comment never keeps one (found by fuzzing).
            return (code, Some(comment.trim_end_matches('\r')));
        }
        prev = c;
    }
    (line, None)
}

fn perr(line: usize, msg: impl Into<String>) -> ParseError {
    ParseError { line, column: 1, message: msg.into(), hint: None }
}

/// Parses document text. Only syntax is checked here; see [`crate::validate`].
pub fn parse(text: &str) -> Result<Document, ParseError> {
    if text.len() > MAX_TEXT_BYTES {
        return Err(perr(1, format!("document is larger than {} MiB", MAX_TEXT_BYTES >> 20)));
    }
    let lines: Vec<&str> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
    if lines.len() > MAX_LINES {
        return Err(perr(1, "document has too many lines"));
    }
    let mut doc = Document::default();
    let mut i = 0usize;
    let mut pending_note: Vec<String> = Vec::new();
    let mut seen_header = false;
    let mut seen_units = false;

    while i < lines.len() {
        let lineno = i + 1;
        let raw = lines.get(i).copied().unwrap_or("");
        i += 1;
        let (code, comment) = split_comment(raw);
        if code.trim().is_empty() {
            match comment {
                Some(c) => pending_note.push(c.to_string()),
                None => {
                    // A blank line detaches comments from the next statement only if the
                    // comment block is followed by a blank line. Keep them: notes are
                    // attached to the next statement in all cases.
                }
            }
            continue;
        }
        let mut c = Cursor::new(code, lineno);
        let kw = c.ident()?;
        if !seen_header {
            if kw != "gusset" {
                return Err(
                    c.err_hint("a Gusset document starts with 'gusset <version>'", "add 'gusset 1' as the first line")
                );
            }
            let v = c.uint()?;
            if v == 0 || v > SCHEMA_VERSION {
                return Err(c.err_hint(
                    format!("document schema {v} is not supported by this version (max {SCHEMA_VERSION})"),
                    "update Gusset to open this file",
                ));
            }
            if !c.at_end() {
                return Err(c.err("unexpected text after version"));
            }
            seen_header = true;
            pending_note.clear();
            continue;
        }
        match kw {
            "units" => {
                if seen_units {
                    return Err(c.err("'units' appears twice"));
                }
                doc.units = match c.ident()? {
                    "mm" => LengthUnit::Millimetre,
                    "in" => LengthUnit::Inch,
                    other => return Err(c.err_hint(format!("unknown document unit '{other}'"), "use 'mm' or 'in'")),
                };
                if !c.at_end() {
                    return Err(c.err("unexpected text after units"));
                }
                seen_units = true;
                pending_note.clear();
            }
            "param" => {
                let name = c.ident()?.to_string();
                c.expect('=')?;
                let expr = c.expr()?;
                if !c.at_end() {
                    return Err(c.err("unexpected text after parameter value"));
                }
                let mut notes = std::mem::take(&mut pending_note);
                if let Some(t) = comment {
                    notes.push(t.to_string());
                }
                doc.params.push(Param { name, expr, comment: notes.join("\n") });
            }
            "sketch" | "extrude" | "revolve" | "fillet" | "chamfer" | "import" => {
                if doc.features.len() >= MAX_FEATURES {
                    return Err(c.err(format!("more than {MAX_FEATURES} features")));
                }
                let mut feature = if kw == "sketch" {
                    let (f, consumed) = parse_sketch(&mut c, &lines, i)?;
                    i = consumed;
                    f
                } else {
                    parse_feature(kw, &mut c)?
                };
                let mut notes = std::mem::take(&mut pending_note);
                if let Some(t) = comment {
                    notes.push(t.to_string());
                }
                if !notes.is_empty() {
                    feature.note = Some(notes.join("\n"));
                }
                doc.features.push(feature);
            }
            other => {
                return Err(c.err_hint(
                    format!("unknown statement '{other}'"),
                    "statements are: param, sketch, extrude, revolve, fillet, chamfer, import",
                ));
            }
        }
    }
    if !seen_header {
        return Err(perr(1, "document is empty").clone_with_hint("a Gusset document starts with 'gusset 1'"));
    }
    Ok(doc)
}

trait WithHint {
    fn clone_with_hint(self, h: &str) -> Self;
}
impl WithHint for ParseError {
    fn clone_with_hint(mut self, h: &str) -> Self {
        self.hint = Some(h.to_string());
        self
    }
}

/// Parses `(key: value, ...)`, calling `on_arg` for each key with the cursor positioned
/// at the value.
fn parse_args(
    c: &mut Cursor<'_>,
    mut on_arg: impl FnMut(&str, &mut Cursor<'_>) -> Result<(), ParseError>,
) -> Result<(), ParseError> {
    c.expect('(')?;
    let mut seen: Vec<String> = Vec::new();
    if c.eat(')') {
        return Ok(());
    }
    loop {
        let key = c.ident()?.to_string();
        if seen.contains(&key) {
            return Err(c.err(format!("argument '{key}' is given twice")));
        }
        c.expect(':')?;
        on_arg(&key, c)?;
        seen.push(key);
        if c.eat(',') {
            continue;
        }
        c.expect(')')?;
        return Ok(());
    }
}

fn parse_bool(c: &mut Cursor<'_>) -> Result<bool, ParseError> {
    match c.ident()? {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(c.err(format!("expected true or false, found '{other}'"))),
    }
}

fn parse_profile(c: &mut Cursor<'_>) -> Result<ProfileRef, ParseError> {
    let sketch = c.ident()?.to_string();
    let mut regions = Vec::new();
    if c.eat('.') {
        if !c.eat_kw("regions") {
            return Err(c.err_hint("expected 'regions(...)' after '.'", "e.g. Sketch1.regions(l3)"));
        }
        c.expect('(')?;
        loop {
            regions.push(c.ident()?.to_string());
            if regions.len() > MAX_LIST_ITEMS {
                return Err(c.err("too many regions"));
            }
            if !c.eat(',') {
                break;
            }
        }
        c.expect(')')?;
    }
    Ok(ProfileRef { sketch, regions })
}

fn unknown_arg(c: &Cursor<'_>, kw: &str, key: &str, allowed: &str) -> ParseError {
    c.err_hint(format!("'{kw}' has no argument '{key}'"), format!("allowed: {allowed}"))
}

fn missing(c: &Cursor<'_>, kw: &str, key: &str) -> ParseError {
    c.err(format!("'{kw}' needs a '{key}' argument"))
}

fn parse_feature(kw: &str, c: &mut Cursor<'_>) -> Result<Feature, ParseError> {
    let name = c.ident()?.to_string();
    let mut label = None;
    let mut suppressed = false;
    let mut profile = None;
    let mut distance = None;
    let mut symmetric = false;
    let mut op = None;
    let mut body = None;
    let mut body_name = None;
    let mut axis = None;
    let mut angle = None;
    let mut edges: Option<Vec<EdgeRef>> = None;
    let mut size = None;
    let mut blob = None;
    let mut format = None;
    let mut file = None;
    let allowed = match kw {
        "extrude" => "profile, distance, symmetric, op, body, body_name, label, suppressed",
        "revolve" => "profile, axis, angle, op, body, body_name, label, suppressed",
        "fillet" => "edges, radius, label, suppressed",
        "chamfer" => "edges, distance, label, suppressed",
        _ => "blob, format, file, label, suppressed",
    };
    parse_args(c, |key, c| {
        match (kw, key) {
            (_, "label") => label = Some(c.string()?),
            (_, "suppressed") => suppressed = parse_bool(c)?,
            ("extrude" | "revolve", "profile") => profile = Some(parse_profile(c)?),
            ("extrude" | "revolve", "op") => {
                let w = c.ident()?;
                op =
                    Some(BodyOp::from_keyword(w).ok_or_else(|| {
                        c.err_hint(format!("unknown operation '{w}'"), "use new, add, cut or intersect")
                    })?);
            }
            ("extrude" | "revolve", "body") => body = Some(c.ident()?.to_string()),
            ("extrude" | "revolve", "body_name") => body_name = Some(c.ident()?.to_string()),
            ("extrude", "distance") | ("chamfer", "distance") => {
                let e = c.expr()?;
                if kw == "extrude" { distance = Some(e) } else { size = Some(e) }
            }
            ("extrude", "symmetric") => symmetric = parse_bool(c)?,
            ("revolve", "axis") => {
                let a = c.ident()?;
                axis = Some(match a {
                    "X" => AxisRef::X,
                    "Y" => AxisRef::Y,
                    "Z" => AxisRef::Z,
                    sketch => {
                        c.expect('.')?;
                        AxisRef::SketchLine { sketch: sketch.to_string(), entity: c.ident()?.to_string() }
                    }
                });
            }
            ("revolve", "angle") => angle = Some(c.expr()?),
            ("fillet" | "chamfer", "edges") => {
                c.expect('[')?;
                let mut list = Vec::new();
                if !c.eat(']') {
                    loop {
                        list.push(parse_edge(c)?);
                        if list.len() > MAX_LIST_ITEMS {
                            return Err(c.err("too many edges"));
                        }
                        if c.eat(',') {
                            continue;
                        }
                        c.expect(']')?;
                        break;
                    }
                }
                edges = Some(list);
            }
            ("fillet", "radius") => size = Some(c.expr()?),
            ("import", "blob") => blob = Some(c.string()?),
            ("import", "format") => {
                let w = c.ident_or_digits()?;
                format =
                    Some(ImportFormat::from_keyword(&w).ok_or_else(|| c.err(format!("unknown import format '{w}'")))?);
            }
            ("import", "file") => file = Some(c.string()?),
            (_, key) => return Err(unknown_arg(c, kw, key, allowed)),
        }
        Ok(())
    })?;
    if !c.at_end() {
        return Err(c.err("unexpected text after ')'"));
    }
    let kind = match kw {
        "extrude" => FeatureKind::Extrude(ExtrudeDef {
            profile: profile.ok_or_else(|| missing(c, kw, "profile"))?,
            distance: distance.ok_or_else(|| missing(c, kw, "distance"))?,
            symmetric,
            op: op.unwrap_or(BodyOp::New),
            body,
            body_name,
        }),
        "revolve" => FeatureKind::Revolve(RevolveDef {
            profile: profile.ok_or_else(|| missing(c, kw, "profile"))?,
            axis: axis.ok_or_else(|| missing(c, kw, "axis"))?,
            angle: angle.ok_or_else(|| missing(c, kw, "angle"))?,
            op: op.unwrap_or(BodyOp::New),
            body,
            body_name,
        }),
        "fillet" | "chamfer" => {
            let def = EdgeBlendDef {
                edges: edges.ok_or_else(|| missing(c, kw, "edges"))?,
                size: size.ok_or_else(|| missing(c, kw, if kw == "fillet" { "radius" } else { "distance" }))?,
            };
            if kw == "fillet" { FeatureKind::Fillet(def) } else { FeatureKind::Chamfer(def) }
        }
        _ => FeatureKind::Import(ImportDef {
            blob: blob.ok_or_else(|| missing(c, kw, "blob"))?,
            format: format.ok_or_else(|| missing(c, kw, "format"))?,
            file_name: file.unwrap_or_default(),
        }),
    };
    Ok(Feature { name, label, note: None, suppressed, kind })
}

impl Cursor<'_> {
    /// An identifier that may start with a digit (`3mf`).
    fn ident_or_digits(&mut self) -> Result<String, ParseError> {
        self.ws();
        let rest = self.rest();
        let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(rest.len());
        if end == 0 {
            return Err(self.err("expected a name"));
        }
        let s = rest.get(..end).unwrap_or("").to_string();
        self.pos += end;
        Ok(s)
    }
}

/// Parses `sketch Name (args) {` and the following block lines. Returns the feature and
/// the index of the line after the closing `}`.
fn parse_sketch(c: &mut Cursor<'_>, lines: &[&str], mut i: usize) -> Result<(Feature, usize), ParseError> {
    let name = c.ident()?.to_string();
    let mut label = None;
    let mut suppressed = false;
    let mut plane = None;
    let mut next_id = None;
    parse_args(c, |key, c| {
        match key {
            "label" => label = Some(c.string()?),
            "suppressed" => suppressed = parse_bool(c)?,
            "next_id" => next_id = Some(c.uint()?),
            "plane" => {
                let save = c.pos;
                let w = c.ident()?;
                let after = c.peek();
                plane = Some(match (w, after) {
                    ("XY", _) if after != Some('/') => PlaneRef::XY,
                    ("XZ", _) if after != Some('/') => PlaneRef::XZ,
                    ("YZ", _) if after != Some('/') => PlaneRef::YZ,
                    _ => {
                        c.pos = save;
                        PlaneRef::Face(parse_face(c)?)
                    }
                });
            }
            other => return Err(unknown_arg(c, "sketch", other, "plane, next_id (optional), label, suppressed")),
        }
        Ok(())
    })?;
    c.expect('{')?;
    if !c.at_end() {
        return Err(c.err("sketch contents start on the next line after '{'"));
    }
    let mut def = SketchDef {
        plane: plane.ok_or_else(|| missing(c, "sketch", "plane"))?,
        // Optional in hand-written text: derived from the ids once the block is read.
        next_id: next_id.unwrap_or(0),
        entities: Vec::new(),
        constraints: Vec::new(),
    };
    let derive_next_id = next_id.is_none();
    let open_line = c.line;
    loop {
        let Some(raw) = lines.get(i).copied() else {
            return Err(
                perr(open_line, format!("sketch '{name}' is not closed")).clone_with_hint("add a line with '}'")
            );
        };
        let lineno = i + 1;
        i += 1;
        let (code, _comment) = split_comment(raw);
        if code.trim().is_empty() {
            continue;
        }
        if code.trim() == "}" {
            break;
        }
        if def.entities.len() + def.constraints.len() >= MAX_SKETCH_ITEMS {
            return Err(perr(lineno, format!("sketch has more than {MAX_SKETCH_ITEMS} items")));
        }
        let mut lc = Cursor::new(code, lineno);
        let id = lc.ident()?.to_string();
        lc.expect('=')?;
        let kind = lc.ident()?;
        lc.expect('(')?;
        match kind {
            "point" | "line" | "circle" | "arc" => {
                let geom = match kind {
                    "point" => {
                        let x = lc.number()?;
                        lc.expect(',')?;
                        let y = lc.number()?;
                        SkGeom::Point { x, y }
                    }
                    "line" => {
                        let a = lc.ident()?.to_string();
                        lc.expect(',')?;
                        let b = lc.ident()?.to_string();
                        SkGeom::Line { a, b }
                    }
                    "circle" => {
                        let center = lc.ident()?.to_string();
                        lc.expect(',')?;
                        let radius = lc.number()?;
                        SkGeom::Circle { center, radius }
                    }
                    _ => {
                        let center = lc.ident()?.to_string();
                        lc.expect(',')?;
                        let start = lc.ident()?.to_string();
                        lc.expect(',')?;
                        let end = lc.ident()?.to_string();
                        SkGeom::Arc { center, start, end }
                    }
                };
                lc.expect(')')?;
                let construction = lc.eat_kw("construction");
                if !lc.at_end() {
                    return Err(lc.err("unexpected text after sketch entity"));
                }
                def.entities.push(SkEntity { id, geom, construction });
            }
            other => {
                let Some(ck) = SkConstraintKind::from_keyword(other) else {
                    return Err(lc.err_hint(
                        format!("unknown sketch item '{other}'"),
                        "entities: point, line, circle, arc; constraints: coincident, on_line, on_circle, midpoint, horizontal, vertical, parallel, perpendicular, tangent, equal, distance, angle, radius, diameter, fixed, symmetric",
                    ));
                };
                let (n, has_value) = ck.arity();
                let mut entities = Vec::with_capacity(n);
                for k in 0..n {
                    if k > 0 {
                        lc.expect(',')?;
                    }
                    entities.push(lc.ident()?.to_string());
                }
                let value = if has_value {
                    lc.expect(',')?;
                    Some(lc.expr()?)
                } else {
                    None
                };
                lc.expect(')')?;
                if !lc.at_end() {
                    return Err(lc.err("unexpected text after constraint"));
                }
                def.constraints.push(SkConstraint { id, kind: ck, entities, value });
            }
        }
    }
    if derive_next_id {
        // Largest numeric suffix of any id (`p3`, `k12`); ids that do not look like one
        // are left for the validator to report.
        let suffix = |id: &str| id.get(1..).and_then(|n| n.parse::<u32>().ok());
        let max = def.entities.iter().map(|e| e.id.as_str()).chain(def.constraints.iter().map(|k| k.id.as_str()));
        def.next_id = max.filter_map(suffix).max().map_or(1, |m| m.saturating_add(1));
    }
    Ok((Feature { name, label, note: None, suppressed, kind: FeatureKind::Sketch(def) }, i))
}

/// Convenience: parse a single face reference (for MCP/GUI input).
pub fn parse_face_ref(s: &str) -> Result<FaceRef, ParseError> {
    FaceRef::parse(s)
}

// ------------------------------------------------------------------ history objects

/// Canonical text of a single feature (a history object, ADR 0006).
pub fn print_feature_object(f: &Feature) -> String {
    let mut s = String::new();
    print_feature(&mut s, f);
    s
}

/// Parses a single-feature history object.
pub fn parse_feature_object(s: &str) -> Result<Feature, ParseError> {
    let doc = parse(&format!("gusset {SCHEMA_VERSION}\n{s}"))?;
    let mut it = doc.features.into_iter();
    match (it.next(), it.next(), doc.params.is_empty()) {
        (Some(f), None, true) => Ok(f),
        _ => Err(perr(1, "a feature object must contain exactly one feature")),
    }
}

/// Canonical text of a parameter table (a history object).
pub fn print_params_object(params: &[Param]) -> String {
    let mut s = String::new();
    for p in params {
        print_param(&mut s, p);
    }
    s
}

/// Parses a parameter-table history object.
pub fn parse_params_object(s: &str) -> Result<Vec<Param>, ParseError> {
    let doc = parse(&format!("gusset {SCHEMA_VERSION}\n{s}"))?;
    if !doc.features.is_empty() {
        return Err(perr(1, "a parameter object must not contain features"));
    }
    Ok(doc.params)
}

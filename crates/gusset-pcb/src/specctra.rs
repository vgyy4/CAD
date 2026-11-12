//! Specctra DSN out and SES in (ADR 0047 phase 2b), for routing a board with an external
//! autorouter the user installed (Freerouting is GPL: never bundled or linked).
//!
//! Written from the published format (the Specctra Design Language Reference and the DSN
//! and SES files autorouters exchange). The DSN is in micrometres at a 0.1 µm resolution
//! with Y up (KiCad's Y negated). Every footprint gets an image of its own, placed at its
//! origin unrotated on the front, with its pins where the board has them (bottom-side
//! pads keep their layers in their padstacks), so no rotation or mirroring convention of
//! the reader matters. The SES reader is bounded (size, depth, items) and fuzzed.

use crate::board::{Board, PadKind, PadShape, Pt};
use crate::geom::{self, Contour};
use crate::rules::Rules;
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// DSN coordinates per mm (µm at 0.1 µm resolution: `(resolution um 10)`).
const SCALE: f64 = 1000.0;

fn q(s: &str) -> String {
    let clean: String = s.chars().filter(|c| !c.is_control() && *c != '"').collect();
    if !clean.is_empty() && clean.chars().all(|c| c.is_ascii_alphanumeric() || "_.-+/[]:@#$%*!?=<>~^&|".contains(c)) {
        clean
    } else {
        format!("\"{clean}\"")
    }
}

fn c(v: f64) -> String {
    let r = (v * SCALE * 10.0).round() / 10.0;
    let mut s = format!("{r:.1}");
    if s.ends_with(".0") {
        s.truncate(s.len() - 2);
    }
    if s == "-0" { "0".into() } else { s }
}

fn xy(p: Pt) -> String {
    format!("{} {}", c(p[0]), c(-p[1]))
}

/// A pad's copper as a Specctra shape on one layer, relative to `origin` (DSN frame).
fn pad_shape(pad: &crate::board::Pad, layer: &str) -> String {
    let rel = |p: Pt| [p[0] - pad.at[0], p[1] - pad.at[1]];
    match geom::pad_contour(pad, 0.0) {
        Some(Contour::Circle { radius, .. }) => format!("(circle {} {})", q(layer), c(2.0 * radius)),
        Some(cont) => {
            let right = geom::right_angle(pad.angle);
            if matches!(pad.shape, PadShape::Rect) && right.is_some() {
                let (w, h) = if right.is_some_and(|r| r % 2 == 1) {
                    (pad.size[1], pad.size[0])
                } else {
                    (pad.size[0], pad.size[1])
                };
                format!("(rect {} {} {})", q(layer), xy([-w / 2.0, h / 2.0]), xy([w / 2.0, -h / 2.0]))
            } else {
                let pts: Vec<String> = cont.polygon(0.01).into_iter().map(|p| xy(rel(p))).collect();
                format!("(polygon {} 0 {})", q(layer), pts.join(" "))
            }
        }
        None => format!("(circle {} 0)", q(layer)),
    }
}

/// The via padstack name for a size and drill (mm), as KiCad names them.
pub fn via_name(size: f64, drill: f64) -> String {
    format!("Via[0-1]_{}:{}_um", (size * 1000.0).round(), (drill * 1000.0).round())
}

/// The board as a Specctra DSN design: layers, outline, rules and net classes, parts,
/// pins and padstacks, nets, and the tracks and vias already laid.
pub fn write_dsn(board: &Board, rules: &Rules, name: &str) -> String {
    let mut o = String::new();
    let cu = board.copper_layers();
    let _ = writeln!(o, "(pcb {}", q(name));
    o.push_str("  (parser\n    (string_quote \")\n    (space_in_quoted_tokens on)\n    (host_cad \"Gusset\")\n    (host_version \"1\")\n  )\n");
    o.push_str("  (resolution um 10)\n  (unit um)\n  (structure\n");
    for (i, l) in cu.iter().enumerate() {
        let _ = writeln!(o, "    (layer {} (type signal) (property (index {i})))", q(l));
    }
    // The outline: its largest loop as the boundary, the others as keep-outs.
    let (loops, _) = geom::outline_loops(board, crate::check::OUTLINE_TOL);
    let mut polys: Vec<Vec<Pt>> = loops.iter().map(|l| l.polygon(0.01)).collect();
    polys.sort_by(|a, b| geom::area(b).abs().total_cmp(&geom::area(a).abs()));
    if let Some(outer) = polys.first() {
        let mut pts: Vec<String> = outer.iter().map(|p| xy(*p)).collect();
        if let Some(f) = outer.first() {
            pts.push(xy(*f));
        }
        let _ = writeln!(o, "    (boundary (path pcb 0 {}))", pts.join(" "));
    }
    for cut in polys.iter().skip(1) {
        let pts: Vec<String> = cut.iter().map(|p| xy(*p)).collect();
        let _ = writeln!(o, "    (keepout \"\" (polygon signal 0 {}))", pts.join(" "));
    }
    for p in board.pads.iter().filter(|p| p.kind == PadKind::Npth) {
        if let Some(d) = p.drill {
            let _ = writeln!(o, "    (keepout \"\" (circle signal {} {}))", c(d.size[0].max(d.size[1])), xy(p.at));
        }
    }
    let (vs, vd) = rules.via_of(None);
    let mut vias: Vec<(f64, f64)> = vec![(vs, vd)];
    for cl in &rules.classes {
        let v = (cl.via_diameter.unwrap_or(vs), cl.via_drill.unwrap_or(vd));
        if !vias.contains(&v) {
            vias.push(v);
        }
    }
    let names: Vec<String> = vias.iter().map(|(s, d)| q(&via_name(*s, *d))).collect();
    let _ = writeln!(o, "    (via {})", names.join(" "));
    let _ = writeln!(o, "    (rule (width {}) (clearance {}))", c(rules.track_width), c(rules.clearance));
    o.push_str("  )\n  (placement\n");
    // One image per footprint, placed at its origin.
    let image = |i: usize, r: &str| format!("{}_{i}", if r.is_empty() { "FP" } else { r });
    let refs: Vec<String> =
        board
            .footprints
            .iter()
            .enumerate()
            .map(|(i, f)| {
                if f.reference.is_empty() || f.reference.contains('-') { format!("FP{i}") } else { f.reference.clone() }
            })
            .collect();
    for (i, f) in board.footprints.iter().enumerate() {
        let _ = writeln!(
            o,
            "    (component {}\n      (place {} {} front 0 (PN {}))\n    )",
            q(&image(i, &f.reference)),
            q(refs.get(i).map_or("", |s| s.as_str())),
            xy(f.at),
            q(&f.value)
        );
    }
    o.push_str("  )\n  (library\n");
    let mut stacks: BTreeMap<String, String> = BTreeMap::new();
    let mut pin_names: Vec<Option<String>> = vec![None; board.pads.len()];
    for (i, f) in board.footprints.iter().enumerate() {
        let _ = writeln!(o, "    (image {}", q(&image(i, &f.reference)));
        let mut used: BTreeMap<String, usize> = BTreeMap::new();
        for &k in &f.pads {
            let Some(pad) = board.pads.get(k) else { continue };
            if pad.kind == PadKind::Npth || pad.number.is_empty() {
                continue;
            }
            let on: Vec<&String> = cu.iter().filter(|l| board.on_layer(&pad.layers, l)).collect();
            if on.is_empty() {
                continue;
            }
            let shapes: Vec<String> = on.iter().map(|l| pad_shape(pad, l)).collect();
            let body = shapes.iter().map(|s| format!("      (shape {s})")).collect::<Vec<_>>().join("\n");
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for b in body.bytes() {
                h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
            }
            let stack = format!("Pad_{:012x}", h & 0xffff_ffff_ffff);
            stacks.entry(stack.clone()).or_insert(body);
            let n = used.entry(pad.number.clone()).or_insert(0);
            let pin = if *n == 0 { pad.number.clone() } else { format!("{}@{n}", pad.number) };
            *n += 1;
            let rel = [pad.at[0] - f.at[0], pad.at[1] - f.at[1]];
            let _ = writeln!(o, "      (pin {} {} {})", q(&stack), q(&pin), xy(rel));
            if let Some(slot) = pin_names.get_mut(k) {
                *slot = Some(format!("{}-{pin}", refs.get(i).map_or("", |s| s.as_str())));
            }
        }
        o.push_str("    )\n");
    }
    for (name, body) in &stacks {
        let _ = writeln!(o, "    (padstack {}\n{body}\n      (attach off)\n    )", q(name));
    }
    for (s, d) in &vias {
        let shapes: Vec<String> = cu.iter().map(|l| format!("      (shape (circle {} {}))", q(l), c(*s))).collect();
        let _ = writeln!(o, "    (padstack {}\n{}\n      (attach off)\n    )", q(&via_name(*s, *d)), shapes.join("\n"));
    }
    o.push_str("  )\n  (network\n");
    let mut nets: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (k, pad) in board.pads.iter().enumerate() {
        if let (Some(net), Some(Some(pin))) = (board.net_name(&pad.net), pin_names.get(k)) {
            nets.entry(net).or_default().push(q(pin));
        }
    }
    for (net, pins) in &nets {
        let _ = writeln!(o, "    (net {}\n      (pins {})\n    )", q(net), pins.join(" "));
    }
    // Net classes: each class's nets with its width, clearance and via.
    let mut classes: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for net in nets.keys() {
        let cl = rules.class_of(net).map_or("kicad_default".to_string(), |c| c.name.clone());
        classes.entry(cl).or_default().push(q(net));
    }
    for (cl, members) in &classes {
        let class = rules.classes.iter().find(|c| c.name == *cl);
        let w = class.and_then(|c| c.track_width).unwrap_or(rules.track_width);
        let cl_c = class.and_then(|c| c.clearance).unwrap_or(rules.clearance);
        let via =
            via_name(class.and_then(|c| c.via_diameter).unwrap_or(vs), class.and_then(|c| c.via_drill).unwrap_or(vd));
        let _ = writeln!(
            o,
            "    (class {} {}\n      (circuit (use_via {}))\n      (rule (width {}) (clearance {}))\n    )",
            q(cl),
            members.join(" "),
            q(&via),
            c(w),
            c(cl_c)
        );
    }
    o.push_str("  )\n  (wiring\n");
    for t in &board.tracks {
        let Some(net) = board.net_name(&t.net) else { continue };
        let pts = match t.mid {
            Some(m) => geom::arc_points(t.start, m, t.end, 0.01),
            None => vec![t.start, t.end],
        };
        let path: Vec<String> = pts.into_iter().map(xy).collect();
        let _ = writeln!(o, "    (wire (path {} {} {}) (net {}))", q(&t.layer), c(t.width), path.join(" "), q(&net));
    }
    for v in &board.vias {
        let Some(net) = board.net_name(&v.net) else { continue };
        let _ = writeln!(o, "    (via {} {} (net {}))", q(&via_name(v.size, v.drill)), xy(v.at), q(&net));
    }
    o.push_str("  )\n)\n");
    o
}

/// Limits of the SES reader.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_bytes: usize,
    pub max_depth: usize,
    pub max_nodes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { max_bytes: 64 << 20, max_depth: 64, max_nodes: 8_000_000 }
    }
}

/// A Specctra list or atom.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Atom(String),
    List(Vec<Node>),
}

impl Node {
    fn head(&self) -> Option<&str> {
        match self {
            Node::List(v) => match v.first() {
                Some(Node::Atom(a)) => Some(a.as_str()),
                _ => None,
            },
            Node::Atom(_) => None,
        }
    }
    fn items(&self) -> &[Node] {
        match self {
            Node::List(v) => v.get(1..).unwrap_or_default(),
            Node::Atom(_) => &[],
        }
    }
    fn atom(&self) -> Option<&str> {
        match self {
            Node::Atom(a) => Some(a),
            Node::List(_) => None,
        }
    }
    fn child(&self, h: &str) -> Option<&Node> {
        self.items().iter().find(|n| n.head() == Some(h))
    }
    fn children<'a>(&'a self, h: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.items().iter().filter(move |n| n.head() == Some(h))
    }
}

/// Why a Specctra file was not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    TooLarge,
    TooDeep,
    TooMany,
    Malformed(&'static str),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::TooLarge => write!(f, "the file is too large"),
            Error::TooDeep => write!(f, "the file nests too deeply"),
            Error::TooMany => write!(f, "the file has too many items"),
            Error::Malformed(m) => write!(f, "malformed Specctra file: {m}"),
        }
    }
}

/// Reads a Specctra file (DSN or SES) as lists and atoms, iteratively. The quote character
/// is `"` unless `(string_quote X)` sets another.
pub fn parse(src: &str, limits: &Limits) -> Result<Node, Error> {
    if src.len() > limits.max_bytes {
        return Err(Error::TooLarge);
    }
    let mut stack: Vec<Vec<Node>> = Vec::new();
    let mut done: Option<Node> = None;
    let mut quote = '"';
    let mut nodes = 0usize;
    let mut it = src.char_indices().peekable();
    let mut after_string_quote = false;
    while let Some((_, ch)) = it.next() {
        if ch.is_whitespace() {
            continue;
        }
        if done.is_some() {
            return Err(Error::Malformed("text after the end"));
        }
        nodes += 1;
        if nodes > limits.max_nodes {
            return Err(Error::TooMany);
        }
        if after_string_quote && ch != ')' {
            // `(string_quote ")`: the next character is the quote itself.
            quote = ch;
            after_string_quote = false;
            stack.last_mut().ok_or(Error::Malformed("atom outside a list"))?.push(Node::Atom(ch.to_string()));
            continue;
        }
        match ch {
            '(' => {
                if stack.len() >= limits.max_depth {
                    return Err(Error::TooDeep);
                }
                stack.push(Vec::new());
            }
            ')' => {
                after_string_quote = false;
                let list = stack.pop().ok_or(Error::Malformed("unbalanced ')'"))?;
                let n = Node::List(list);
                match stack.last_mut() {
                    Some(parent) => parent.push(n),
                    None => done = Some(n),
                }
            }
            c if c == quote => {
                let mut s = String::new();
                loop {
                    match it.next() {
                        Some((_, ch)) if ch == quote => break,
                        Some((_, ch)) => s.push(ch),
                        None => return Err(Error::Malformed("unterminated string")),
                    }
                }
                stack.last_mut().ok_or(Error::Malformed("atom outside a list"))?.push(Node::Atom(s));
            }
            c => {
                let mut s = String::from(c);
                while let Some(&(_, n)) = it.peek() {
                    if n.is_whitespace() || n == '(' || n == ')' {
                        break;
                    }
                    s.push(n);
                    it.next();
                }
                let list = stack.last_mut().ok_or(Error::Malformed("atom outside a list"))?;
                if list.is_empty() && s == "string_quote" {
                    after_string_quote = true;
                }
                list.push(Node::Atom(s));
            }
        }
    }
    done.ok_or(Error::Malformed("unbalanced '('"))
}

/// A wire of a session: layer, width (mm) and points (board mm, Y down).
#[derive(Debug, Clone, PartialEq)]
pub struct Wire {
    pub layer: String,
    pub width: f64,
    pub pts: Vec<Pt>,
}

/// A via of a session: padstack name, place, and the padstack's diameter when the session
/// lists it (mm).
#[derive(Debug, Clone, PartialEq)]
pub struct SesVia {
    pub padstack: String,
    pub at: Pt,
    pub size: Option<f64>,
}

/// The routes of one net.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SesNet {
    pub name: String,
    pub wires: Vec<Wire>,
    pub vias: Vec<SesVia>,
}

/// A Specctra session's routes.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Session {
    pub nets: Vec<SesNet>,
}

/// mm per unit of `(resolution unit n)`.
fn unit_mm(n: &Node) -> Option<f64> {
    let unit = n.items().first()?.atom()?.to_ascii_lowercase();
    let per: f64 = n.items().get(1)?.atom()?.parse().ok().filter(|v: &f64| v.is_finite() && *v > 0.0)?;
    let mm = match unit.as_str() {
        "inch" => 25.4,
        "mil" => 0.0254,
        "cm" => 10.0,
        "mm" => 1.0,
        "um" => 0.001,
        _ => return None,
    };
    Some(mm / per)
}

fn num(n: Option<&Node>) -> Option<f64> {
    n?.atom()?.parse::<f64>().ok().filter(|v| v.is_finite() && v.abs() < 1e12)
}

/// Reads the routes of a Specctra session file (`.ses`): wires and vias per net.
///
/// # Errors
/// Malformed files, or files past `limits`.
pub fn read_ses(src: &str, limits: &Limits) -> Result<Session, Error> {
    let root = parse(src, limits)?;
    if root.head() != Some("session") {
        return Err(Error::Malformed("not a session file"));
    }
    let routes = root.child("routes").ok_or(Error::Malformed("no routes"))?;
    let scale = routes.child("resolution").and_then(unit_mm).unwrap_or(0.0001);
    let pt = |x: Option<&Node>, y: Option<&Node>| -> Option<Pt> {
        let (x, y) = (num(x)? * scale, num(y)? * scale);
        (x.abs() <= 10_000.0 && y.abs() <= 10_000.0).then_some([x, -y])
    };
    let mut sizes: BTreeMap<String, f64> = BTreeMap::new();
    if let Some(lib) = routes.child("library_out") {
        for ps in lib.children("padstack") {
            let Some(name) = ps.items().first().and_then(Node::atom) else { continue };
            let d = ps
                .children("shape")
                .filter_map(|s| s.child("circle"))
                .filter_map(|c| num(c.items().get(1)))
                .fold(0.0f64, f64::max);
            if d > 0.0 {
                sizes.insert(name.to_string(), d * scale);
            }
        }
    }
    let mut out = Session::default();
    let mut items = 0usize;
    let Some(net_out) = routes.child("network_out") else { return Ok(out) };
    for net in net_out.children("net") {
        let name = net.items().first().and_then(Node::atom).unwrap_or_default().to_string();
        let mut sn = SesNet { name, ..SesNet::default() };
        for w in net.children("wire") {
            let Some(path) = w.child("path").or_else(|| w.child("polyline_path")) else { continue };
            let a = path.items();
            let layer = a.first().and_then(Node::atom).unwrap_or_default().to_string();
            let width = num(a.get(1)).map_or(0.0, |v| v * scale);
            let pts: Vec<Pt> =
                a.get(2..).unwrap_or_default().chunks(2).filter_map(|c| pt(c.first(), c.get(1))).collect();
            items += pts.len();
            if items > limits.max_nodes {
                return Err(Error::TooMany);
            }
            if pts.len() >= 2 && width > 0.0 && width < 100.0 {
                sn.wires.push(Wire { layer, width, pts });
            }
        }
        for v in net.children("via") {
            let a = v.items();
            let padstack = a.first().and_then(Node::atom).unwrap_or_default().to_string();
            if let Some(at) = pt(a.get(1), a.get(2)) {
                let size = sizes.get(&padstack).copied();
                sn.vias.push(SesVia { padstack, at, size });
                items += 1;
            }
        }
        out.nets.push(sn);
    }
    Ok(out)
}

/// Size and drill (mm) of a via padstack named as [`via_name`] writes them.
pub fn via_from_name(name: &str) -> Option<(f64, f64)> {
    let rest = name.rsplit_once('_')?.0;
    let dims = rest.rsplit_once('_')?.1;
    let (s, d) = dims.split_once(':')?;
    let (s, d): (f64, f64) = (s.parse().ok()?, d.parse().ok()?);
    (s.is_finite() && d.is_finite() && d > 0.0 && s > d && s < 50_000.0).then_some((s / 1000.0, d / 1000.0))
}

impl Board {
    /// Lays a session's routes: every track and via goes (when `replace`), then the
    /// session's wires and vias are added on their nets, in one edit. Vias take the
    /// padstack's size, its drill from the name, else the rules' via for the net.
    ///
    /// # Errors
    /// Layers this board does not have, or a failed re-read.
    pub fn apply_session(
        &mut self,
        ses: &Session,
        rules: &Rules,
        replace: bool,
        limits: &crate::board::Limits,
    ) -> Result<(usize, usize), crate::board::Error> {
        let cu = self.copper_layers();
        if replace {
            let tracks: Vec<usize> = (0..self.tracks.len()).collect();
            let vias: Vec<usize> = (0..self.vias.len()).collect();
            self.delete_items(&tracks, &vias, limits)?;
        }
        let mut routes: Vec<crate::edit::NewRoute> = Vec::new();
        let mut vias: Vec<crate::edit::NewVia> = Vec::new();
        let mut nt = 0usize;
        for net in &ses.nets {
            let name = (!net.name.is_empty()).then_some(net.name.clone());
            for w in &net.wires {
                if !cu.contains(&w.layer) {
                    return Err(crate::board::Error::Edit(format!(
                        "the session routes on '{}', which this board does not have",
                        w.layer.chars().take(40).collect::<String>()
                    )));
                }
                let segs: Vec<crate::route::Seg> = w
                    .pts
                    .windows(2)
                    .filter_map(|p| match p {
                        [a, b] if libm::hypot(b[0] - a[0], b[1] - a[1]) > 1e-6 => {
                            Some(crate::route::Seg { start: *a, end: *b, mid: None })
                        }
                        _ => None,
                    })
                    .collect();
                if !segs.is_empty() {
                    nt += segs.len();
                    routes.push((segs, w.width, w.layer.clone(), name.clone()));
                }
            }
            for v in &net.vias {
                let (rs, rd) = rules.via_of(name.as_deref());
                let (s, d) = via_from_name(&v.padstack).unwrap_or((rs, rd));
                let s = v.size.unwrap_or(s);
                let d = if d < s { d } else { rd.min(s * 0.5) };
                vias.push((v.at, s, d, name.clone()));
            }
        }
        let nv = vias.len();
        self.add_batch(&routes, &vias, limits)?;
        Ok((nt, nv))
    }
}

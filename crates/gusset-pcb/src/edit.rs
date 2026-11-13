//! Board edits (ADR 0047 phase 1b): each changes the S-expression tree, which stays the
//! source of truth, and re-reads the typed view, so everything an edit does not touch is
//! written back as it was read.

use crate::board::{Board, Error, Limits, Pt};
use crate::sexpr::{self, Node};

/// Largest coordinate an edit accepts (mm), as the reader's.
const MAX_COORD: f64 = 10_000.0;

fn bad(msg: impl Into<String>) -> Error {
    Error::Edit(msg.into())
}

fn check_pt(p: Pt) -> Result<(), Error> {
    if p.iter().all(|v| v.is_finite() && v.abs() <= MAX_COORD) {
        Ok(())
    } else {
        Err(bad("coordinates must be finite and within 10 m"))
    }
}

fn list(items: Vec<Node>) -> Node {
    Node::List(items)
}

fn xy(head: &str, p: Pt) -> Node {
    list(vec![Node::sym(head), Node::num(p[0]), Node::num(p[1])])
}

fn kv(head: &str, v: Node) -> Node {
    list(vec![Node::sym(head), v])
}

/// A route to add: pieces, width, layer, net.
pub type NewRoute = (Vec<crate::route::Seg>, f64, String, Option<String>);
/// A via to add: place, size, drill, net.
pub type NewVia = (Pt, f64, f64, Option<String>);

/// The F/B counterpart of a layer name (`F.SilkS` ↔ `B.SilkS`); others unchanged.
pub fn flip_layer(name: &str) -> String {
    if let Some(r) = name.strip_prefix("F.") {
        format!("B.{r}")
    } else if let Some(r) = name.strip_prefix("B.") {
        format!("F.{r}")
    } else {
        name.to_string()
    }
}

/// A 64-bit mix (splitmix64) for deterministic ids.
fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// A UUID (version 4 layout) derived from the item's text and the board's size, so the
/// same edit on the same board gives the same file.
pub(crate) fn uuid_for(seed_text: &str, salt: usize) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in seed_text.bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    let a = mix(h ^ salt as u64);
    let b = mix(a);
    format!(
        "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
        a >> 32,
        (a >> 16) & 0xffff,
        a & 0x0fff,
        0x8000 | ((b >> 48) & 0x3fff),
        b & 0xffff_ffff_ffff
    )
}

impl Board {
    /// Whether the file writes `uuid`s (KiCad 6 and later).
    pub(crate) fn writes_uuids(&self) -> bool {
        self.version.is_some_and(|v| v >= 20_210_000)
    }

    /// A new two-layer board: a `w` × `h` mm rectangular outline from the origin, a
    /// 1.6 mm FR4 stackup with 35 µm copper, and no parts.
    ///
    /// # Errors
    /// If the size is not in (0, 10 m].
    pub fn new_two_layer(w: f64, h: f64) -> Result<Board, Error> {
        if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0 && w <= MAX_COORD && h <= MAX_COORD) {
            return Err(bad("the board size must be above 0 and at most 10 m"));
        }
        let (w, h) = (sexpr::fmt_num(w), sexpr::fmt_num(h));
        let text = format!(
            r#"(kicad_pcb (version 20240108) (generator "gusset") (generator_version "1")
  (general (thickness 1.6) (legacy_teardrops no))
  (paper "A4")
  (layers
    (0 "F.Cu" signal)
    (31 "B.Cu" signal)
    (32 "B.Adhes" user "B.Adhesive")
    (33 "F.Adhes" user "F.Adhesive")
    (34 "B.Paste" user)
    (35 "F.Paste" user)
    (36 "B.SilkS" user "B.Silkscreen")
    (37 "F.SilkS" user "F.Silkscreen")
    (38 "B.Mask" user)
    (39 "F.Mask" user)
    (44 "Edge.Cuts" user)
    (45 "Margin" user)
    (46 "B.CrtYd" user "B.Courtyard")
    (47 "F.CrtYd" user "F.Courtyard")
    (48 "B.Fab" user)
    (49 "F.Fab" user)
  )
  (setup
    (stackup
      (layer "F.SilkS" (type "Top Silk Screen"))
      (layer "F.Paste" (type "Top Solder Paste"))
      (layer "F.Mask" (type "Top Solder Mask") (thickness 0.01))
      (layer "F.Cu" (type "copper") (thickness 0.035))
      (layer "dielectric 1" (type "core") (thickness 1.51) (material "FR4") (epsilon_r 4.5) (loss_tangent 0.02))
      (layer "B.Cu" (type "copper") (thickness 0.035))
      (layer "B.Mask" (type "Bottom Solder Mask") (thickness 0.01))
      (layer "B.Paste" (type "Bottom Solder Paste"))
      (layer "B.SilkS" (type "Bottom Silk Screen"))
      (copper_finish "None")
      (dielectric_constraints no)
    )
    (pad_to_mask_clearance 0)
  )
  (net 0 "")
  (gr_rect (start 0 0) (end {w} {h}) (stroke (width 0.05) (type default)) (fill none) (layer "Edge.Cuts") (uuid "{}"))
)
"#,
            uuid_for(&format!("outline {w} {h}"), 0)
        );
        Board::read(&text, &Limits::default())
    }

    /// Re-reads the typed view after the tree changed.
    pub(crate) fn reread(&mut self, limits: &Limits) -> Result<(), Error> {
        let tree = std::mem::replace(&mut self.tree, Node::List(Vec::new()));
        *self = Board::from_tree(tree, limits)?;
        Ok(())
    }

    /// The `(net ...)` node an item of `net` gets, adding the net to the board's list when
    /// the file numbers its nets and does not know it yet. `None`: no net.
    pub(crate) fn net_node(&mut self, net: Option<&str>) -> Result<Option<Node>, Error> {
        let net = net.filter(|n| !n.is_empty());
        if !self.numbered_nets {
            return Ok(net.map(|n| kv("net", Node::str(n))));
        }
        let Some(name) = net else { return Ok(Some(kv("net", Node::num(0.0)))) };
        if let Some(n) = self.nets.iter().find(|n| n.name == name) {
            return Ok(Some(kv("net", Node::sym(n.id.to_string()))));
        }
        if name.chars().count() > 200 || name.chars().any(char::is_control) {
            return Err(bad("a net name is at most 200 characters, without control characters"));
        }
        let id = self.nets.iter().map(|n| n.id).max().unwrap_or(0) + 1;
        let decl = list(vec![Node::sym("net"), Node::sym(id.to_string()), Node::str(name)]);
        let items = self.tree.items_mut().ok_or_else(|| bad("not a board"))?;
        // After the last net declaration (KiCad lists them before the items).
        let at = items.iter().rposition(|n| n.head() == Some("net")).map_or(items.len(), |i| i + 1);
        items.insert(at, decl);
        self.nets.push(crate::board::Net { id, name: name.to_string() });
        Ok(Some(kv("net", Node::sym(id.to_string()))))
    }

    pub(crate) fn push_item(&mut self, mut items: Vec<Node>, limits: &Limits) -> Result<(), Error> {
        if self.writes_uuids() {
            let n = self.tree.items().len();
            let text = sexpr::write(&list(items.clone()));
            items.push(kv("uuid", Node::str(uuid_for(&text, n))));
        }
        self.tree.items_mut().ok_or_else(|| bad("not a board"))?.push(list(items));
        self.reread(limits)
    }

    pub(crate) fn copper_layer(&self, layer: &str) -> Result<(), Error> {
        if self.copper_layers().iter().any(|l| l == layer) {
            Ok(())
        } else {
            Err(bad(format!("'{}' is not a copper layer of this board", layer.chars().take(40).collect::<String>())))
        }
    }

    /// Adds a straight track from `start` to `end`; returns its index in [`Board::tracks`].
    ///
    /// # Errors
    /// Bad coordinates or width, a layer that is not copper, or a failed re-read.
    pub fn add_track(
        &mut self,
        start: Pt,
        end: Pt,
        width: f64,
        layer: &str,
        net: Option<&str>,
        limits: &Limits,
    ) -> Result<usize, Error> {
        check_pt(start)?;
        check_pt(end)?;
        if !(width.is_finite() && width > 0.0 && width <= 100.0) {
            return Err(bad("the track width must be above 0 and at most 100 mm"));
        }
        if libm::hypot(end[0] - start[0], end[1] - start[1]) < 1e-6 {
            return Err(bad("the track has no length"));
        }
        self.copper_layer(layer)?;
        let mut items = vec![Node::sym("segment"), xy("start", start), xy("end", end), kv("width", Node::num(width))];
        items.push(kv("layer", Node::str(layer)));
        items.extend(self.net_node(net)?);
        self.push_item(items, limits)?;
        Ok(self.tracks.len().saturating_sub(1))
    }

    /// Adds a routed track (lines and arcs) on one layer in one edit; returns the index of
    /// its first piece in [`Board::tracks`].
    ///
    /// # Errors
    /// Bad coordinates or width, a layer that is not copper, an empty route, or a failed
    /// re-read.
    pub fn add_route(
        &mut self,
        segs: &[crate::route::Seg],
        width: f64,
        layer: &str,
        net: Option<&str>,
        limits: &Limits,
    ) -> Result<usize, Error> {
        let first = self.tracks.len();
        self.push_route(segs, width, layer, net)?;
        self.reread(limits)?;
        Ok(first)
    }

    /// Adds several routes (pieces, width, layer, net) and through vias (place, size,
    /// drill, net) in one edit.
    ///
    /// # Errors
    /// As [`Board::add_route`] and [`Board::add_via`].
    pub fn add_batch(&mut self, routes: &[NewRoute], vias: &[NewVia], limits: &Limits) -> Result<(), Error> {
        if routes.len() + vias.len() > self.limits_items(limits) {
            return Err(bad("too many items at once"));
        }
        for (segs, width, layer, net) in routes {
            self.push_route(segs, *width, layer, net.as_deref())?;
        }
        for (at, size, drill, net) in vias {
            self.push_via(*at, *size, *drill, net.as_deref())?;
        }
        self.reread(limits)
    }

    fn limits_items(&self, limits: &Limits) -> usize {
        limits.max_items.saturating_sub(self.tracks.len() + self.vias.len())
    }

    fn push_route(
        &mut self,
        segs: &[crate::route::Seg],
        width: f64,
        layer: &str,
        net: Option<&str>,
    ) -> Result<(), Error> {
        if !(width.is_finite() && width > 0.0 && width <= 100.0) {
            return Err(bad("the track width must be above 0 and at most 100 mm"));
        }
        if segs.is_empty() || segs.len() > 100_000 {
            return Err(bad("a route has 1 to 100000 pieces"));
        }
        self.copper_layer(layer)?;
        let net_node = self.net_node(net)?;
        for s in segs {
            check_pt(s.start)?;
            check_pt(s.end)?;
            if libm::hypot(s.end[0] - s.start[0], s.end[1] - s.start[1]) < 1e-6 {
                continue;
            }
            let mut items = match s.mid {
                Some(m) => {
                    check_pt(m)?;
                    vec![Node::sym("arc"), xy("start", s.start), xy("mid", m), xy("end", s.end)]
                }
                None => vec![Node::sym("segment"), xy("start", s.start), xy("end", s.end)],
            };
            items.push(kv("width", Node::num(width)));
            items.push(kv("layer", Node::str(layer)));
            items.extend(net_node.clone());
            self.push_raw(items)?;
        }
        Ok(())
    }

    fn push_via(&mut self, at: Pt, size: f64, drill: f64, net: Option<&str>) -> Result<(), Error> {
        check_pt(at)?;
        if !(size.is_finite() && drill.is_finite() && drill > 0.0 && size > drill && size <= 50.0) {
            return Err(bad("a via needs a drill above 0 and a size above the drill (at most 50 mm)"));
        }
        let cu = self.copper_layers();
        let (Some(a), Some(b)) = (cu.first(), cu.last()) else { return Err(bad("the board has no copper layers")) };
        let layers = list(vec![Node::sym("layers"), Node::str(a.clone()), Node::str(b.clone())]);
        let mut items =
            vec![Node::sym("via"), xy("at", at), kv("size", Node::num(size)), kv("drill", Node::num(drill))];
        items.push(layers);
        items.extend(self.net_node(net)?);
        self.push_raw(items)
    }

    /// Appends an item (with a uuid in KiCad 6+ files) without re-reading.
    fn push_raw(&mut self, mut items: Vec<Node>) -> Result<(), Error> {
        if self.writes_uuids() {
            let n = self.tree.items().len();
            let text = sexpr::write(&list(items.clone()));
            items.push(kv("uuid", Node::str(uuid_for(&text, n))));
        }
        self.tree.items_mut().ok_or_else(|| bad("not a board"))?.push(list(items));
        Ok(())
    }

    /// Adds a through via; returns its index in [`Board::vias`].
    ///
    /// # Errors
    /// Bad coordinates or sizes, or a failed re-read.
    pub fn add_via(
        &mut self,
        at: Pt,
        size: f64,
        drill: f64,
        net: Option<&str>,
        limits: &Limits,
    ) -> Result<usize, Error> {
        self.push_via(at, size, drill, net)?;
        self.reread(limits)?;
        Ok(self.vias.len().saturating_sub(1))
    }

    /// Deletes tracks and vias by their indices in [`Board::tracks`] and [`Board::vias`].
    ///
    /// # Errors
    /// An index past the lists, or a failed re-read.
    pub fn delete_items(&mut self, tracks: &[usize], vias: &[usize], limits: &Limits) -> Result<usize, Error> {
        let mut nodes = Vec::new();
        for &i in tracks {
            nodes.push(self.tracks.get(i).ok_or_else(|| bad(format!("there is no track {i}")))?.node);
        }
        for &i in vias {
            nodes.push(self.vias.get(i).ok_or_else(|| bad(format!("there is no via {i}")))?.node);
        }
        nodes.sort_unstable();
        nodes.dedup();
        let items = self.tree.items_mut().ok_or_else(|| bad("not a board"))?;
        for &n in nodes.iter().rev() {
            if n < items.len() && n > 0 {
                items.remove(n);
            }
        }
        self.reread(limits)?;
        Ok(nodes.len())
    }

    /// Replaces the board outline (the board's own `Edge.Cuts` drawings; footprint ones
    /// stay) with one closed polygon (at least 3 points).
    ///
    /// # Errors
    /// Too few or bad points, a degenerate polygon, or a failed re-read.
    pub fn set_outline(&mut self, pts: &[Pt], limits: &Limits) -> Result<(), Error> {
        if pts.len() < 3 || pts.len() > 100_000 {
            return Err(bad("an outline needs 3 to 100000 points"));
        }
        for p in pts {
            check_pt(*p)?;
        }
        if crate::geom::area(pts).abs() < 1e-6 {
            return Err(bad("the outline encloses no area"));
        }
        let items = self.tree.items_mut().ok_or_else(|| bad("not a board"))?;
        items.retain(|n| {
            let edge = n.child("layer").and_then(|l| l.arg(0)) == Some("Edge.Cuts");
            !(edge && n.head().is_some_and(|h| h.starts_with("gr_")))
        });
        let stroke = if self.version.is_some_and(|v| v >= 20_211_014) {
            list(vec![Node::sym("stroke"), kv("width", Node::num(0.05)), kv("type", Node::sym("default"))])
        } else {
            kv("width", Node::num(0.05))
        };
        let same = |u: f64, v: f64| (u - v).abs() < 1e-9;
        let rect = match pts {
            [a, b, c, d] => {
                (same(a[1], b[1]) && same(b[0], c[0]) && same(c[1], d[1]) && same(d[0], a[0]))
                    || (same(a[0], b[0]) && same(b[1], c[1]) && same(c[0], d[0]) && same(d[1], a[1]))
            }
            _ => false,
        };
        let new = if self.version.is_some_and(|v| v < 20_210_000) {
            // KiCad 5 boards: lines.
            let n = pts.len();
            (0..n)
                .filter_map(|i| Some((pts.get(i)?, pts.get((i + 1) % n)?)))
                .map(|(a, b)| {
                    vec![
                        Node::sym("gr_line"),
                        xy("start", *a),
                        xy("end", *b),
                        kv("layer", Node::str("Edge.Cuts")),
                        stroke.clone(),
                    ]
                })
                .collect::<Vec<_>>()
        } else if rect {
            let (lo, hi) = pts.iter().fold(([f64::MAX; 2], [f64::MIN; 2]), |(lo, hi), p| {
                ([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])])
            });
            vec![vec![
                Node::sym("gr_rect"),
                xy("start", lo),
                xy("end", hi),
                stroke,
                kv("fill", Node::sym("none")),
                kv("layer", Node::str("Edge.Cuts")),
            ]]
        } else {
            let mut p = vec![Node::sym("pts")];
            p.extend(pts.iter().map(|q| xy("xy", *q)));
            vec![vec![
                Node::sym("gr_poly"),
                list(p),
                stroke,
                kv("fill", Node::sym("none")),
                kv("layer", Node::str("Edge.Cuts")),
            ]]
        };
        for n in new {
            if self.writes_uuids() {
                let mut n = n;
                let salt = self.tree.items().len();
                n.push(kv("uuid", Node::str(uuid_for(&sexpr::write(&list(n.clone())), salt))));
                self.tree.items_mut().ok_or_else(|| bad("not a board"))?.push(list(n));
            } else {
                self.tree.items_mut().ok_or_else(|| bad("not a board"))?.push(list(n));
            }
        }
        self.reread(limits)
    }

    /// Moves a footprint to the other side: its layers swap front and back and it is
    /// mirrored about its own horizontal axis (KiCad's default flip), staying at its place.
    ///
    /// # Errors
    /// No such footprint, or a failed re-read.
    pub fn flip_footprint(&mut self, index: usize, limits: &Limits) -> Result<(), Error> {
        let node = self.footprints.get(index).ok_or_else(|| bad(format!("there is no footprint {index}")))?.node;
        let fp = if node == 0 {
            &mut self.tree
        } else {
            self.tree.items_mut().and_then(|v| v.get_mut(node)).ok_or_else(|| bad("not a board"))?
        };
        if let Some(items) = fp.items_mut() {
            for c in items.iter_mut().skip(1) {
                if c.head() == Some("at") {
                    // The footprint's own place stays; its angle mirrors.
                    negate_arg(c, 2);
                } else {
                    flip_node(c, 0);
                }
            }
        }
        self.reread(limits)
    }

    /// The net of the copper at `p` on `layer`: a pad, a track end or a via there.
    pub fn net_at(&self, p: Pt, layer: &str) -> Option<String> {
        let near = |q: Pt, r: f64| libm::hypot(q[0] - p[0], q[1] - p[1]) <= r;
        for pad in &self.pads {
            if !self.on_layer(&pad.layers, layer) || pad.net.is_none() {
                continue;
            }
            let hit = crate::geom::pad_contour(pad, 0.0)
                .map(|c| {
                    crate::geom::inside(&c.polygon(0.01), p)
                        || matches!(c, crate::geom::Contour::Circle { center, radius } if near(center, radius))
                })
                .unwrap_or(false);
            if hit {
                return self.net_name(&pad.net);
            }
        }
        for v in &self.vias {
            let [la, lb] = &v.layers;
            if near(v.at, v.size / 2.0) && !v.net.is_none() && self.span(la, lb).iter().any(|l| l == layer) {
                return self.net_name(&v.net);
            }
        }
        for t in &self.tracks {
            if t.layer == layer && !t.net.is_none() && (near(t.start, t.width / 2.0) || near(t.end, t.width / 2.0)) {
                return self.net_name(&t.net);
            }
        }
        None
    }

    /// The index of a footprint by reference (`R1`), or `#<index>`.
    pub fn footprint_index(&self, name: &str) -> Option<usize> {
        if let Some(i) = name.strip_prefix('#').and_then(|i| i.parse::<usize>().ok()) {
            return (i < self.footprints.len()).then_some(i);
        }
        self.footprints.iter().position(|f| f.reference == name)
    }
}

/// Negates the `i`-th argument of a list when it is a number.
fn negate_arg(n: &mut Node, i: usize) {
    let Some(items) = n.items_mut() else { return };
    let Some(slot) = items.get_mut(i + 1) else { return };
    if let Some(v) = slot.atom().and_then(sexpr::parse_num) {
        *slot = Node::num(-v);
    }
}

/// Mirrors one item of a footprint (local coordinates: y → −y, angles → −angle) and swaps
/// its layers front and back. Depth is bounded by the reader's limit.
fn flip_node(n: &mut Node, depth: usize) {
    if depth > 200 {
        return;
    }
    match n.head() {
        Some("at") => {
            negate_arg(n, 1);
            negate_arg(n, 2);
            return;
        }
        Some("start" | "end" | "center" | "mid" | "xy" | "rect_delta" | "offset") => {
            negate_arg(n, 1);
            return;
        }
        // KiCad 5 arcs: the sweep turns the other way.
        Some("angle") => {
            negate_arg(n, 0);
            return;
        }
        Some("layer" | "layers") => {
            if let Some(items) = n.items_mut() {
                for a in items.iter_mut().skip(1) {
                    if let Node::Atom(atom) = a {
                        atom.text = flip_layer(&atom.text);
                    }
                }
            }
            return;
        }
        // 3D models follow the side on their own.
        Some("model") => return,
        _ => {}
    }
    if let Some(items) = n.items_mut() {
        for c in items.iter_mut().skip(1) {
            if matches!(c, Node::List(_)) {
                flip_node(c, depth + 1);
            }
        }
    }
}

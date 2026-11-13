//! Board text edits (ADR 0049): adding, changing and deleting texts. Like every edit they
//! change the S-expression tree and re-read the typed view, so what an edit does not touch
//! is written back as it was read.
//!
//! A text is a board `gr_text` or a footprint's `fp_text`/`property`. Board texts can be
//! changed in every way; footprint texts keep their words when they are the reference or
//! the value (rename the part instead), and their place is given on the board, turned back
//! into the footprint's own coordinates.

use crate::board::{Board, Error, Limits, Pt, Text};
use crate::sexpr::Node;

/// Largest text (characters).
const MAX_TEXT: usize = 1000;
/// Largest character height (mm).
const MAX_SIZE: f64 = 100.0;

fn bad(msg: impl Into<String>) -> Error {
    Error::Edit(msg.into())
}

fn list(items: Vec<Node>) -> Node {
    Node::List(items)
}

fn kv(head: &str, v: Node) -> Node {
    list(vec![Node::sym(head), v])
}

/// A text to add: everything but the place and the words has a default.
#[derive(Debug, Clone, PartialEq)]
pub struct NewText {
    pub text: String,
    pub at: Pt,
    /// Default `F.SilkS`.
    pub layer: Option<String>,
    /// Character height, mm (default 1).
    pub size: Option<f64>,
    /// Stroke thickness, mm (default 0.15 of the height).
    pub thickness: Option<f64>,
    /// Degrees, counter-clockwise on screen.
    pub angle: Option<f64>,
    /// Default: mirrored on the back side.
    pub mirrored: Option<bool>,
    /// Horizontal -1 left, 0 centre, 1 right; vertical -1 top, 0 centre, 1 bottom.
    pub justify: Option<[i8; 2]>,
}

/// What changes in a text; `None` stays.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextPatch {
    pub text: Option<String>,
    pub at: Option<Pt>,
    pub layer: Option<String>,
    pub size: Option<f64>,
    pub thickness: Option<f64>,
    pub angle: Option<f64>,
    /// Moving to the other side without this turns the mirroring with it.
    pub mirrored: Option<bool>,
    pub justify: Option<[i8; 2]>,
}

/// Whether `layer` is a layer a text may be on: a copper layer of the board or one of
/// KiCad's technical and user layers.
fn text_layer(b: &Board, layer: &str) -> Result<(), Error> {
    let tech = ["SilkS", "Mask", "Paste", "Fab", "CrtYd", "Adhes"];
    let user = ["Cmts.User", "Dwgs.User", "Eco1.User", "Eco2.User", "Edge.Cuts", "Margin"];
    let ok = b.copper_layers().iter().any(|l| l == layer)
        || user.contains(&layer)
        || layer.strip_prefix("F.").or_else(|| layer.strip_prefix("B.")).is_some_and(|r| tech.contains(&r))
        || layer.strip_prefix("User.").is_some_and(|n| n.parse::<u8>().is_ok_and(|n| (1..=9).contains(&n)));
    if ok {
        Ok(())
    } else {
        Err(bad(format!("'{}' is not a layer for texts", layer.chars().take(40).collect::<String>())))
    }
}

fn check_words(t: &str) -> Result<(), Error> {
    if t.is_empty() || t.chars().count() > MAX_TEXT {
        return Err(bad("a text has 1 to 1000 characters"));
    }
    if t.chars().any(|c| c.is_control() && c != '\n') {
        return Err(bad("a text has no control characters but line breaks"));
    }
    Ok(())
}

fn check_look(
    size: Option<f64>,
    thickness: Option<f64>,
    angle: Option<f64>,
    justify: Option<[i8; 2]>,
) -> Result<(), Error> {
    if size.is_some_and(|v| !(v.is_finite() && v > 0.0 && v <= MAX_SIZE)) {
        return Err(bad("the text height must be above 0 and at most 100 mm"));
    }
    if thickness.is_some_and(|v| !(v.is_finite() && v > 0.0 && v <= MAX_SIZE)) {
        return Err(bad("the stroke thickness must be above 0 and at most 100 mm"));
    }
    if angle.is_some_and(|v| !v.is_finite()) {
        return Err(bad("the angle must be a number of degrees"));
    }
    if justify.is_some_and(|j| j.iter().any(|v| !(-1..=1).contains(v))) {
        return Err(bad("justification is -1, 0 or 1 in each direction"));
    }
    Ok(())
}

fn check_at(p: Pt) -> Result<(), Error> {
    if p.iter().all(|v| v.is_finite() && v.abs() <= 10_000.0) {
        Ok(())
    } else {
        Err(bad("coordinates must be finite and within 10 m"))
    }
}

/// `(justify left bottom mirror)`, or none when nothing is set.
fn justify_node(j: [i8; 2], mirrored: bool) -> Option<Node> {
    let mut v = vec![Node::sym("justify")];
    match j[0] {
        -1 => v.push(Node::sym("left")),
        1 => v.push(Node::sym("right")),
        _ => {}
    }
    match j[1] {
        -1 => v.push(Node::sym("top")),
        1 => v.push(Node::sym("bottom")),
        _ => {}
    }
    if mirrored {
        v.push(Node::sym("mirror"));
    }
    (v.len() > 1).then(|| list(v))
}

/// The `(effects ...)` of a new text.
fn effects_node(size: f64, thickness: f64, j: [i8; 2], mirrored: bool) -> Node {
    let font = list(vec![
        Node::sym("font"),
        list(vec![Node::sym("size"), Node::num(size), Node::num(size)]),
        kv("thickness", Node::num(thickness)),
    ]);
    let mut e = vec![Node::sym("effects"), font];
    e.extend(justify_node(j, mirrored));
    list(e)
}

/// Sets size, thickness and justification in a node's `effects`, keeping what else is in
/// it (bold, italic, hide ...).
fn set_effects(n: &mut Node, size: [f64; 2], thickness: f64, j: [i8; 2], mirrored: bool) {
    let Some(items) = n.items_mut() else { return };
    if !items.iter().any(|c| c.head() == Some("effects")) {
        items.push(list(vec![Node::sym("effects")]));
    }
    let Some(e) = items.iter_mut().find(|c| c.head() == Some("effects")).and_then(Node::items_mut) else { return };
    if !e.iter().any(|c| c.head() == Some("font")) {
        e.insert(1, list(vec![Node::sym("font")]));
    }
    if let Some(f) = e.iter_mut().find(|c| c.head() == Some("font")).and_then(Node::items_mut) {
        f.retain(|c| !matches!(c.head(), Some("size" | "thickness")));
        f.insert(1, list(vec![Node::sym("size"), Node::num(size[0]), Node::num(size[1])]));
        f.insert(2, kv("thickness", Node::num(thickness)));
    }
    e.retain(|c| c.head() != Some("justify"));
    e.extend(justify_node(j, mirrored));
}

/// Turns the mirroring of a node's `effects` over (a footprint flipped to the other side
/// reads from below).
pub(crate) fn toggle_mirror(effects: &mut Node) {
    let Some(items) = effects.items_mut() else { return };
    if let Some(j) = items.iter_mut().find(|c| c.head() == Some("justify")).and_then(Node::items_mut) {
        if let Some(k) = j.iter().position(|a| a.atom() == Some("mirror")) {
            j.remove(k);
        } else {
            j.push(Node::sym("mirror"));
        }
        if j.len() == 1 {
            items.retain(|c| c.head() != Some("justify"));
        }
    } else {
        items.push(list(vec![Node::sym("justify"), Node::sym("mirror")]));
    }
}

fn at_node(p: Pt, angle: f64, always_angle: bool) -> Node {
    let mut v = vec![Node::sym("at"), Node::num(p[0]), Node::num(p[1])];
    if angle.abs() > 1e-12 || always_angle {
        v.push(Node::num(angle));
    }
    list(v)
}

impl Board {
    /// Adds a board text (`gr_text`); returns its index in [`Board::texts`].
    ///
    /// # Errors
    /// Empty, long or odd text, bad numbers, a layer texts cannot be on, or a failed re-read.
    pub fn add_text(&mut self, t: &NewText, limits: &Limits) -> Result<usize, Error> {
        check_words(&t.text)?;
        check_at(t.at)?;
        check_look(t.size, t.thickness, t.angle, t.justify)?;
        let layer = t.layer.clone().unwrap_or_else(|| "F.SilkS".into());
        text_layer(self, &layer)?;
        let size = t.size.unwrap_or(1.0);
        let thickness = t.thickness.unwrap_or(((size * 0.15) * 1000.0).round() / 1000.0).min(size);
        let mirrored = t.mirrored.unwrap_or_else(|| layer.starts_with("B."));
        let old = self.version.is_some_and(|v| v < 20_210_000);
        let layer_node = if old { Node::sym(layer.as_str()) } else { Node::str(layer.as_str()) };
        let items = vec![
            Node::sym("gr_text"),
            Node::str(t.text.as_str()),
            at_node(t.at, t.angle.unwrap_or(0.0), old),
            list(vec![Node::sym("layer"), layer_node]),
            effects_node(size, thickness, t.justify.unwrap_or([0, 0]), mirrored),
        ];
        self.push_item(items, limits)?;
        Ok(self.texts.len().saturating_sub(1))
    }

    fn text_target(&self, index: usize) -> Result<Text, Error> {
        self.texts.get(index).cloned().ok_or_else(|| bad(format!("there is no text {index}")))
    }

    fn text_node_mut(&mut self, t: &Text) -> Result<&mut Node, Error> {
        let top = self.tree.items_mut().and_then(|v| v.get_mut(t.node)).ok_or_else(|| bad("not a board"))?;
        if t.sub == 0 {
            Ok(top)
        } else {
            top.items_mut().and_then(|v| v.get_mut(t.sub)).ok_or_else(|| bad("not a board"))
        }
    }

    /// Changes a text by its index in [`Board::texts`]. Footprint texts: the reference and
    /// the value keep their words, and the place is turned into the footprint's own.
    ///
    /// # Errors
    /// No such text, bad values, or a failed re-read.
    pub fn edit_text(&mut self, index: usize, p: &TextPatch, limits: &Limits) -> Result<(), Error> {
        let t = self.text_target(index)?;
        if let Some(w) = &p.text {
            check_words(w)?;
        }
        if let Some(a) = p.at {
            check_at(a)?;
        }
        check_look(p.size, p.thickness, p.angle, p.justify)?;
        if let Some(l) = &p.layer {
            text_layer(self, l)?;
        }
        let fp = t.footprint.and_then(|i| self.footprints.get(i)).map(|f| (f.at, f.angle));
        let size = p.size.unwrap_or(t.size);
        // A new height takes the character width with it.
        let width = if p.size.is_some() {
            size
        } else if t.width > 0.0 {
            t.width
        } else {
            t.size
        };
        let thickness = p.thickness.unwrap_or(if t.thickness > 0.0 { t.thickness } else { t.size / 8.0 }).min(size);
        let side_changed = p.layer.as_deref().is_some_and(|l| l.starts_with("B.") != t.layer.starts_with("B."));
        let mirrored = p.mirrored.unwrap_or(if side_changed { !t.mirrored } else { t.mirrored });
        let old = self.version.is_some_and(|v| v < 20_210_000);
        let n = self.text_node_mut(&t)?;
        if let Some(w) = &p.text {
            // `(gr_text "w" ...)`, `(fp_text user "w" ...)`, `(property "Name" "w" ...)`.
            let (kind, at) = if n.head() == Some("gr_text") { ("", 1) } else { (n.arg(0).unwrap_or_default(), 2) };
            if kind.eq_ignore_ascii_case("reference") || kind.eq_ignore_ascii_case("value") {
                return Err(bad("the reference and value of a footprint keep their words"));
            }
            if let Some(a) = n.items_mut().and_then(|v| v.get_mut(at)) {
                *a = Node::str(w.as_str());
            }
        }
        if p.at.is_some() || p.angle.is_some() {
            let angle = p.angle.unwrap_or(t.angle);
            let pos = p.at.unwrap_or(t.at);
            let local = match fp {
                Some((fat, fangle)) => {
                    let (s, c) = (libm::sin(fangle.to_radians()), libm::cos(fangle.to_radians()));
                    let (dx, dy) = (pos[0] - fat[0], pos[1] - fat[1]);
                    [dx * c - dy * s, dx * s + dy * c]
                }
                None => pos,
            };
            if let Some(a) = n.child_mut("at") {
                let keep = old || a.num_arg(2).is_some();
                *a = at_node(local, angle, keep);
            }
        }
        if let Some(l) = &p.layer {
            let node = if old { Node::sym(l.as_str()) } else { Node::str(l.as_str()) };
            if let Some(c) = n.child_mut("layer").and_then(Node::items_mut) {
                c.truncate(1);
                c.push(node);
            }
        }
        set_effects(n, [size, width], thickness, p.justify.unwrap_or(t.justify), mirrored);
        self.reread(limits)
    }

    /// Deletes board texts (`gr_text`) by their indices in [`Board::texts`]; returns how
    /// many. Footprint texts stay.
    ///
    /// # Errors
    /// An index past the list, a footprint's text, or a failed re-read.
    pub fn delete_texts(&mut self, indices: &[usize], limits: &Limits) -> Result<usize, Error> {
        let mut nodes = Vec::new();
        for &i in indices {
            let t = self.text_target(i)?;
            if t.footprint.is_some() {
                return Err(bad(format!("text {i} belongs to a footprint and cannot be deleted")));
            }
            nodes.push(t.node);
        }
        nodes.sort_unstable();
        nodes.dedup();
        let items = self.tree.items_mut().ok_or_else(|| bad("not a board"))?;
        for &n in nodes.iter().rev() {
            if n > 0 && n < items.len() {
                items.remove(n);
            }
        }
        self.reread(limits)?;
        Ok(nodes.len())
    }
}

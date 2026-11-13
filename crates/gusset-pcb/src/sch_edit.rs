//! Schematic edits (ADR 0047 phase 2a): each changes the S-expression tree, which stays
//! the source of truth, and re-reads the typed view, so everything an edit does not touch
//! is written back as it was read. New items get deterministic UUIDs.

use crate::board::{Limits, Pt};
use crate::edit::uuid_for;
use crate::sch::{Error, LabelKind, Mirror, Schematic};
use crate::sexpr::{self, Node};

const MAX_COORD: f64 = 10_000.0;
/// Longest label or reference an edit accepts (characters).
const MAX_NAME: usize = 200;

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

fn check_name(what: &str, t: &str) -> Result<(), Error> {
    if t.is_empty() || t.chars().count() > MAX_NAME || t.chars().any(char::is_control) {
        Err(bad(format!("a {what} is 1 to {MAX_NAME} characters, without control characters")))
    } else {
        Ok(())
    }
}

fn list(items: Vec<Node>) -> Node {
    Node::List(items)
}

fn kv(head: &str, v: Node) -> Node {
    list(vec![Node::sym(head), v])
}

fn at_node(p: Pt, angle: Option<f64>) -> Node {
    let mut v = vec![Node::sym("at"), Node::num(p[0]), Node::num(p[1])];
    if let Some(a) = angle {
        v.push(Node::num(a));
    }
    list(v)
}

fn effects(justify: Option<&str>, hide: bool) -> Node {
    let mut v = vec![
        Node::sym("effects"),
        list(vec![Node::sym("font"), list(vec![Node::sym("size"), Node::num(1.27), Node::num(1.27)])]),
    ];
    if let Some(j) = justify {
        let mut jl = vec![Node::sym("justify")];
        jl.extend(j.split(' ').map(Node::sym));
        v.push(list(jl));
    }
    if hide {
        v.push(kv("hide", Node::sym("yes")));
    }
    list(v)
}

fn yes(head: &str, on: bool) -> Node {
    kv(head, Node::sym(if on { "yes" } else { "no" }))
}

/// Sets every `(reference ...)` under an `instances` node.
fn set_references(n: &mut Node, value: &str, depth: usize) {
    if depth > 16 {
        return;
    }
    if n.head() == Some("reference")
        && let Some(v) = n.items_mut()
        && let Some(slot) = v.get_mut(1)
    {
        *slot = Node::str(value);
        return;
    }
    if let Some(v) = n.items_mut() {
        for c in v.iter_mut().skip(1) {
            set_references(c, value, depth + 1);
        }
    }
}

/// An angle as KiCad stores symbol and label angles: 0, 90, 180 or 270.
fn right_angle(a: f64) -> Result<f64, Error> {
    let a = a.rem_euclid(360.0);
    let r = (a / 90.0).round() * 90.0;
    if (a - r).abs() > 1e-6 {
        return Err(bad("schematic angles are multiples of 90 degrees"));
    }
    Ok(r.rem_euclid(360.0))
}

/// A schematic item by kind and index into its list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Item {
    Symbol(usize),
    Wire(usize),
    Bus(usize),
    Label(usize),
    Junction(usize),
    NoConnect(usize),
}

impl Item {
    /// `R1` (a symbol by reference), or `symbol#3`, `wire#2`, `bus#0`, `label#0`,
    /// `junction#1`, `no_connect#0`.
    pub fn parse(s: &Schematic, name: &str) -> Option<Item> {
        if let Some((kind, i)) = name.split_once('#') {
            let i: usize = i.parse().ok()?;
            let (item, n) = match kind {
                "symbol" | "" => (Item::Symbol(i), s.symbols.len()),
                "wire" => (Item::Wire(i), s.wires.len()),
                "bus" => (Item::Bus(i), s.buses.len()),
                "label" => (Item::Label(i), s.labels.len()),
                "junction" => (Item::Junction(i), s.junctions.len()),
                "no_connect" => (Item::NoConnect(i), s.no_connects.len()),
                _ => return None,
            };
            return (i < n).then_some(item);
        }
        s.symbols.iter().position(|x| x.reference() == name).map(Item::Symbol)
    }

    /// The item's name for [`Item::parse`].
    pub fn name(self) -> String {
        match self {
            Item::Symbol(i) => format!("symbol#{i}"),
            Item::Wire(i) => format!("wire#{i}"),
            Item::Bus(i) => format!("bus#{i}"),
            Item::Label(i) => format!("label#{i}"),
            Item::Junction(i) => format!("junction#{i}"),
            Item::NoConnect(i) => format!("no_connect#{i}"),
        }
    }
}

impl Schematic {
    /// An empty A4 sheet (KiCad 9 layout); `seed` makes its UUID.
    ///
    /// # Errors
    /// Never in practice (the text is fixed); kept for the reader's result.
    pub fn new_empty(seed: &str) -> Result<Schematic, Error> {
        let text = format!(
            "(kicad_sch (version 20250114) (generator \"gusset\") (generator_version \"1\") (uuid \"{}\") \
             (paper \"A4\") (lib_symbols) (sheet_instances (path \"/\" (page \"1\"))) (embedded_fonts no))",
            uuid_for(&format!("schematic {seed}"), 0)
        );
        Schematic::read(&text, &Limits::default())
    }

    fn reread(&mut self, limits: &Limits) -> Result<(), Error> {
        let tree = std::mem::take(&mut self.tree);
        *self = Schematic::from_tree(tree, limits)?;
        Ok(())
    }

    /// A UUID for a new item from its text and the sheet's size.
    fn new_uuid(&self, items: &[Node]) -> Node {
        let text = sexpr::write(&list(items.to_vec()));
        let seed = format!("{}{}", self.uuid.as_deref().unwrap_or(""), text);
        kv("uuid", Node::str(uuid_for(&seed, self.tree.items().len())))
    }

    /// Inserts a top-level item before the trailing `sheet_instances` / `embedded_fonts`.
    fn insert(&mut self, item: Node) -> Result<(), Error> {
        let items = self.tree.items_mut().ok_or_else(|| bad("not a schematic"))?;
        let at = items
            .iter()
            .position(|n| matches!(n.head(), Some("sheet_instances" | "symbol_instances" | "embedded_fonts")))
            .unwrap_or(items.len());
        items.insert(at, item);
        Ok(())
    }

    /// The next free reference with this prefix (`R3` when R1 and R2 exist).
    pub fn next_reference(&self, prefix: &str) -> String {
        let used: std::collections::HashSet<u64> = self
            .symbols
            .iter()
            .filter_map(|s| s.reference().strip_prefix(prefix).and_then(|n| n.parse().ok()))
            .collect();
        let n = (1..).find(|n| !used.contains(n)).unwrap_or(1);
        format!("{prefix}{n}")
    }

    /// Places a symbol: `lib_node` is its library node (see
    /// [`crate::sch::SymbolLib::embedded_node`]), added to `lib_symbols` under `lib_id`
    /// when missing. `project` names the instance (KiCad's project name). Returns the
    /// index in [`Schematic::symbols`].
    ///
    /// # Errors
    /// Bad place, angle, unit or reference, or a failed re-read.
    #[allow(clippy::too_many_arguments)]
    pub fn add_symbol(
        &mut self,
        lib_node: &Node,
        lib_id: &str,
        at: Pt,
        angle: f64,
        mirror: Mirror,
        unit: u32,
        reference: &str,
        project: &str,
        limits: &Limits,
    ) -> Result<usize, Error> {
        check_pt(at)?;
        check_name("reference", reference)?;
        check_name("library id", lib_id)?;
        let angle = right_angle(angle)?;
        let lib = crate::sch::LibSymbol::from_node(lib_node, limits.max_items)?
            .ok_or_else(|| bad("the library symbol has no name"))?;
        if unit == 0 || unit > lib.unit_count() {
            return Err(bad(format!("the symbol has {} unit(s)", lib.unit_count())));
        }
        if !self.lib_symbols.iter().any(|l| l.name == lib_id) {
            let mut node = lib_node.clone();
            if let Some(Node::Atom(a)) = node.items_mut().and_then(|v| v.get_mut(1)) {
                a.text = lib_id.to_string();
            }
            let items = self.tree.items_mut().ok_or_else(|| bad("not a schematic"))?;
            match items.iter_mut().find(|n| n.head() == Some("lib_symbols")).and_then(Node::items_mut) {
                Some(ls) => ls.push(node),
                None => {
                    let at = items.iter().position(|n| n.head() == Some("paper")).map_or(items.len().min(1), |i| i + 1);
                    items.insert(at, list(vec![Node::sym("lib_symbols"), node]));
                }
            }
        }
        // The new symbol, placed like the reader will place it.
        let placed = crate::sch::Symbol {
            lib_id: lib_id.to_string(),
            lib_name: None,
            at,
            angle,
            mirror,
            unit,
            style: 1,
            uuid: None,
            properties: Vec::new(),
            instances: Vec::new(),
            in_bom: true,
            on_board: true,
            dnp: false,
            node: 0,
        };
        let mut items = vec![Node::sym("symbol"), kv("lib_id", Node::str(lib_id)), at_node(at, Some(angle))];
        match mirror {
            Mirror::None => {}
            Mirror::X => items.push(kv("mirror", Node::sym("x"))),
            Mirror::Y => items.push(kv("mirror", Node::sym("y"))),
        }
        items.push(kv("unit", Node::num(f64::from(unit))));
        items.extend([yes("exclude_from_sim", false), yes("in_bom", true), yes("on_board", true), yes("dnp", false)]);
        let uuid = self.new_uuid(&items);
        let sym_uuid = uuid.arg(0).unwrap_or("").to_string();
        items.push(uuid);
        for p in &lib.properties {
            let value = if p.key == "Reference" { reference } else { p.value.as_str() };
            let mut prop = vec![Node::sym("property"), Node::str(&p.key), Node::str(value)];
            prop.push(at_node(placed.place(p.at), Some(p.angle)));
            // The library's text effects (size, justification, hidden) are kept.
            let lib_effects = lib_node
                .children("property")
                .find(|n| n.arg(0) == Some(p.key.as_str()))
                .and_then(|n| n.child("effects"));
            prop.push(lib_effects.cloned().unwrap_or_else(|| effects(None, p.hidden)));
            items.push(list(prop));
        }
        let mut numbers: Vec<&str> = lib.pins_of(unit, 1).map(|p| p.number.as_str()).collect();
        numbers.dedup();
        for (k, n) in numbers.iter().enumerate() {
            let id = uuid_for(&format!("{sym_uuid} pin {n}"), k);
            items.push(list(vec![Node::sym("pin"), Node::str(*n), kv("uuid", Node::str(id))]));
        }
        let root = format!("/{}", self.uuid.as_deref().unwrap_or(""));
        items.push(list(vec![
            Node::sym("instances"),
            list(vec![
                Node::sym("project"),
                Node::str(project),
                list(vec![
                    Node::sym("path"),
                    Node::str(root),
                    kv("reference", Node::str(reference)),
                    kv("unit", Node::num(f64::from(unit))),
                ]),
            ]),
        ]));
        self.insert(list(items))?;
        let n = self.symbols.len();
        self.reread(limits)?;
        if self.symbols.len() != n + 1 {
            return Err(bad("the new symbol did not read back"));
        }
        // Symbols are kept in file order; the new one sits before the trailing items.
        self.symbols.iter().position(|s| s.uuid.as_deref() == Some(sym_uuid.as_str())).ok_or_else(|| bad("lost"))
    }

    fn top_item_mut(&mut self, node: usize) -> Result<&mut Vec<Node>, Error> {
        self.tree
            .items_mut()
            .and_then(|v| v.get_mut(node))
            .and_then(Node::items_mut)
            .ok_or_else(|| bad("the item is not in the file"))
    }

    /// Moves, turns or mirrors a symbol; its fields move with it, and wire ends on its
    /// pins follow the pins: a wire that ran straight keeps running horizontally or
    /// vertically (a dogleg is added where the pin moved off its line), junctions stay and
    /// are added where wires now meet.
    ///
    /// # Errors
    /// No such symbol, a bad place or angle, or a failed re-read.
    pub fn move_symbol(
        &mut self,
        index: usize,
        at: Pt,
        angle: f64,
        mirror: Mirror,
        limits: &Limits,
    ) -> Result<(), Error> {
        check_pt(at)?;
        let angle = right_angle(angle)?;
        let old = self.symbols.get(index).cloned().ok_or_else(|| bad("no such symbol"))?;
        let mut new = old.clone();
        (new.at, new.angle, new.mirror) = (at, angle, mirror);
        let moves = self.pin_moves(&old, &new);
        self.edit_symbol_tree(&old, &new)?;
        let (fresh, spots) = self.reroute(&moves, &[])?;
        self.finish_reroute(fresh, spots, limits)
    }

    /// (before, after) places of the pins of a symbol that move.
    fn pin_moves(&self, old: &crate::sch::Symbol, new: &crate::sch::Symbol) -> Vec<(Pt, Pt)> {
        let before = self.pins(old);
        let after = self.pins(new);
        before
            .iter()
            .filter_map(|b| after.iter().find(|a| a.number == b.number).map(|a| (b.at, a.at)))
            .filter(|(b, a)| libm::hypot(a[0] - b[0], a[1] - b[1]) > 1e-9)
            .collect()
    }

    /// Writes a symbol's new place, angle, mirror and field places into the tree.
    fn edit_symbol_tree(&mut self, old: &crate::sch::Symbol, new: &crate::sch::Symbol) -> Result<(), Error> {
        let (at, angle, mirror) = (new.at, new.angle, new.mirror);
        let items = self.top_item_mut(old.node)?;
        items.retain(|n| n.head() != Some("mirror"));
        let at_pos = items.iter().position(|n| n.head() == Some("at")).ok_or_else(|| bad("the symbol has no place"))?;
        if let Some(slot) = items.get_mut(at_pos) {
            *slot = at_node(at, Some(angle));
        }
        match mirror {
            Mirror::None => {}
            Mirror::X => items.insert(at_pos + 1, kv("mirror", Node::sym("x"))),
            Mirror::Y => items.insert(at_pos + 1, kv("mirror", Node::sym("y"))),
        }
        // Fields keep their place relative to the symbol's body.
        for p in items.iter_mut().filter(|n| n.head() == Some("property")) {
            let Some(a) = p.child_mut("at") else { continue };
            let (Some(x), Some(y)) = (a.num_arg(0), a.num_arg(1)) else { continue };
            let local = old.orient_inverse([x - old.at[0], y - old.at[1]]);
            let o = new.orient(local);
            if let Some(v) = a.items_mut() {
                if let Some(s) = v.get_mut(1) {
                    *s = Node::num(at[0] + o[0]);
                }
                if let Some(s) = v.get_mut(2) {
                    *s = Node::num(at[1] + o[1]);
                }
            }
        }
        Ok(())
    }

    /// Follows connection points that moved (`moves`: before, after) with the ends of the
    /// wires not in `skip` (file positions of wires moved as a whole). A wire running
    /// horizontally or vertically keeps its direction: the end slides along it to the new
    /// column or row, and a segment to the new place is added (a dogleg); other wires, and
    /// wires with both ends moved, are stretched straight. Returns the wires to add (a, b)
    /// and the points to check for junctions. Edits the tree only; call
    /// [`Schematic::finish_reroute`] after.
    fn reroute(&mut self, moves: &[(Pt, Pt)], skip: &[usize]) -> Result<(Vec<(Pt, Pt)>, Vec<Pt>), Error> {
        let eps = 1e-6;
        let near = |a: Pt, b: Pt| libm::hypot(a[0] - b[0], a[1] - b[1]) < eps;
        let target = |p: Pt| moves.iter().find(|(from, _)| near(*from, p)).map(|(_, to)| *to);
        let mut fresh: Vec<(Pt, Pt)> = Vec::new();
        let mut spots: Vec<Pt> = moves.iter().flat_map(|(a, b)| [*a, *b]).collect();
        let mut current: Vec<(Pt, Pt)> = self.wires.iter().map(|w| (w.a, w.b)).collect();
        let wires: Vec<(usize, usize, Pt, Pt)> = self
            .wires
            .iter()
            .enumerate()
            .filter(|(_, w)| !skip.contains(&w.node))
            .map(|(i, w)| (i, w.node, w.a, w.b))
            .collect();
        for (idx, node, a, b) in wires {
            let (na, nb) = match (target(a), target(b)) {
                (None, None) => continue,
                (Some(qa), Some(qb)) => (qa, qb),
                (Some(q), None) => {
                    let (end, seg) = dogleg(b, a, q);
                    fresh.extend(seg);
                    (end, b)
                }
                (None, Some(q)) => {
                    let (end, seg) = dogleg(a, b, q);
                    fresh.extend(seg);
                    (a, end)
                }
            };
            self.set_wire_ends(node, na, nb)?;
            if let Some(c) = current.get_mut(idx) {
                *c = (na, nb);
            }
        }
        // No duplicates, and nothing a wire already covers.
        let eps_on = crate::netlist::EPS;
        let mut kept: Vec<(Pt, Pt)> = Vec::new();
        for (p, q) in fresh {
            let same = |x: &(Pt, Pt)| (near(x.0, p) && near(x.1, q)) || (near(x.0, q) && near(x.1, p));
            let covered = current.iter().any(|&(a, b)| {
                crate::netlist::point_segment(p, a, b) <= eps_on && crate::netlist::point_segment(q, a, b) <= eps_on
            });
            if !kept.iter().any(same) && !covered {
                kept.push((p, q));
            }
        }
        spots.extend(kept.iter().flat_map(|(a, b)| [*a, *b]));
        Ok((kept, spots))
    }

    fn set_wire_ends(&mut self, node: usize, a: Pt, b: Pt) -> Result<(), Error> {
        let items = self.top_item_mut(node)?;
        let Some(pts) = items.iter_mut().find(|n| n.head() == Some("pts")).and_then(Node::items_mut) else {
            return Ok(());
        };
        let mut ends = [a, b].into_iter();
        for xy in pts.iter_mut().skip(1).filter(|n| n.head() == Some("xy")) {
            let Some(p) = ends.next() else { break };
            *xy = list(vec![Node::sym("xy"), Node::num(p[0]), Node::num(p[1])]);
        }
        Ok(())
    }

    /// Whether a junction dot is needed at `p`: a wire end in a wire's middle, or three
    /// or more wire ends meeting, and none there yet.
    fn needs_junction(&self, p: Pt) -> bool {
        let eps = crate::netlist::EPS;
        let ends = self
            .wires
            .iter()
            .flat_map(|w| [w.a, w.b])
            .filter(|q| libm::hypot(q[0] - p[0], q[1] - p[1]) <= eps)
            .count();
        (ends >= 3 || (ends >= 1 && self.on_wire_middle(p)))
            && !self.junctions.iter().any(|j| libm::hypot(j.at[0] - p[0], j.at[1] - p[1]) <= eps)
    }

    /// Adds the doglegs from [`Schematic::reroute`], re-reads, and adds junction dots
    /// where wires now meet in `spots`.
    fn finish_reroute(&mut self, fresh: Vec<(Pt, Pt)>, spots: Vec<Pt>, limits: &Limits) -> Result<(), Error> {
        for (a, b) in fresh {
            let mut items = vec![
                Node::sym("wire"),
                list(vec![
                    Node::sym("pts"),
                    list(vec![Node::sym("xy"), Node::num(a[0]), Node::num(a[1])]),
                    list(vec![Node::sym("xy"), Node::num(b[0]), Node::num(b[1])]),
                ]),
                list(vec![Node::sym("stroke"), kv("width", Node::num(0.0)), kv("type", Node::sym("default"))]),
            ];
            items.push(self.new_uuid(&items));
            self.insert(list(items))?;
        }
        self.reread(limits)?;
        let mut dots: Vec<Pt> = Vec::new();
        for p in spots {
            if self.needs_junction(p) && !dots.iter().any(|d| libm::hypot(d[0] - p[0], d[1] - p[1]) < 1e-6) {
                dots.push(p);
            }
        }
        if !dots.is_empty() {
            for p in dots {
                self.push_junction(p)?;
            }
            self.reread(limits)?;
        }
        Ok(())
    }

    /// Moves items by `delta` (mm) as one edit: symbols, labels, wires, buses, junctions
    /// and no-connects translate; wires outside the selection that end on a pin, a wire
    /// end, a label, a junction or a no-connect that moved follow it with an orthogonal
    /// dogleg (see [`Schematic::move_symbol`]). Returns the number of items moved.
    ///
    /// # Errors
    /// An item past its list, a bad delta, or a failed re-read.
    pub fn move_items(&mut self, items: &[Item], delta: Pt, limits: &Limits) -> Result<usize, Error> {
        check_pt(delta)?;
        let mut sel: Vec<Item> = items.to_vec();
        sel.sort_unstable();
        sel.dedup();
        let shift = |p: Pt| [p[0] + delta[0], p[1] + delta[1]];
        let missing = |it: &Item| bad(format!("there is no {}", it.name()));
        let mut moves: Vec<(Pt, Pt)> = Vec::new();
        let mut whole: Vec<usize> = Vec::new();
        let mut symbols: Vec<(crate::sch::Symbol, crate::sch::Symbol)> = Vec::new();
        let mut shifts: Vec<usize> = Vec::new();
        for it in &sel {
            match *it {
                Item::Symbol(i) => {
                    let old = self.symbols.get(i).cloned().ok_or_else(|| missing(it))?;
                    let mut new = old.clone();
                    new.at = shift(old.at);
                    check_pt(new.at)?;
                    moves.extend(self.pin_moves(&old, &new));
                    symbols.push((old, new));
                }
                Item::Wire(i) => {
                    let w = self.wires.get(i).ok_or_else(|| missing(it))?;
                    moves.extend([(w.a, shift(w.a)), (w.b, shift(w.b))]);
                    whole.push(w.node);
                    shifts.push(w.node);
                }
                Item::Bus(i) => shifts.push(self.buses.get(i).ok_or_else(|| missing(it))?.node),
                Item::Label(i) => {
                    let l = self.labels.get(i).ok_or_else(|| missing(it))?;
                    moves.push((l.at, shift(l.at)));
                    shifts.push(l.node);
                }
                Item::Junction(i) => {
                    let m = self.junctions.get(i).ok_or_else(|| missing(it))?;
                    moves.push((m.at, shift(m.at)));
                    shifts.push(m.node);
                }
                Item::NoConnect(i) => {
                    let m = self.no_connects.get(i).ok_or_else(|| missing(it))?;
                    moves.push((m.at, shift(m.at)));
                    shifts.push(m.node);
                }
            }
        }
        for (old, new) in &symbols {
            self.edit_symbol_tree(old, new)?;
        }
        for node in shifts {
            shift_node(self.top_item_mut(node)?, delta);
        }
        // Wires outside the selection follow the points that moved.
        let (fresh, spots) = self.reroute(&moves, &whole)?;
        self.finish_reroute(fresh, spots, limits)?;
        Ok(sel.len())
    }

    /// Sets a field (property) of a symbol: `Value`, `Footprint` or any other; a field the
    /// symbol lacks is added, hidden, at the symbol's place.
    ///
    /// # Errors
    /// No such symbol, a bad field name or value, or a failed re-read.
    pub fn set_field(&mut self, index: usize, key: &str, value: &str, limits: &Limits) -> Result<(), Error> {
        check_name("a field name", key)?;
        if value.chars().count() > 1000 || value.chars().any(char::is_control) {
            return Err(bad("a field value is at most 1000 characters, without control characters"));
        }
        if key == "Reference" && value.trim().is_empty() {
            return Err(bad("a reference cannot be empty"));
        }
        let sym = self.symbols.get(index).cloned().ok_or_else(|| bad("no such symbol"))?;
        let items = self.top_item_mut(sym.node)?;
        let existing = items.iter_mut().find(|n| n.head() == Some("property") && n.arg(0) == Some(key));
        match existing.and_then(Node::items_mut) {
            Some(v) => {
                if let Some(slot) = v.get_mut(2) {
                    *slot = Node::str(value);
                }
            }
            None => {
                let at = at_node(sym.at, Some(0.0));
                let prop = list(vec![Node::sym("property"), Node::str(key), Node::str(value), at, effects(None, true)]);
                let pos = items.iter().rposition(|n| n.head() == Some("property")).map_or(items.len(), |i| i + 1);
                items.insert(pos, prop);
            }
        }
        // References also live in the per-sheet-use instances.
        if key == "Reference"
            && let Some(inst) = items.iter_mut().find(|n| n.head() == Some("instances"))
        {
            set_references(inst, value, 0);
        }
        self.reread(limits)
    }

    /// Moves or turns a label.
    ///
    /// # Errors
    /// No such label, a bad place or angle, or a failed re-read.
    pub fn move_label(&mut self, index: usize, at: Pt, angle: f64, limits: &Limits) -> Result<(), Error> {
        check_pt(at)?;
        let angle = right_angle(angle)?;
        let node = self.labels.get(index).ok_or_else(|| bad("no such label"))?.node;
        let items = self.top_item_mut(node)?;
        let slot = items.iter_mut().find(|n| n.head() == Some("at")).ok_or_else(|| bad("the label has no place"))?;
        *slot = at_node(at, Some(angle));
        self.reread(limits)
    }

    /// Whether `p` lies inside a wire (not at its ends).
    fn on_wire_middle(&self, p: Pt) -> bool {
        let eps = crate::netlist::EPS;
        self.wires.iter().any(|w| {
            let end = |q: Pt| libm::hypot(q[0] - p[0], q[1] - p[1]) <= eps;
            !end(w.a) && !end(w.b) && crate::netlist::point_segment(p, w.a, w.b) <= eps
        })
    }

    /// Adds wires along `pts` (a polyline, one wire per segment) and junctions where a
    /// new end lands inside a wire or three or more wire ends meet. Returns the number
    /// of wires added.
    ///
    /// # Errors
    /// Fewer than two points, bad coordinates, or a failed re-read.
    pub fn add_wires(&mut self, pts: &[Pt], limits: &Limits) -> Result<usize, Error> {
        if pts.len() < 2 || pts.len() > 10_000 {
            return Err(bad("a wire needs 2 to 10000 points"));
        }
        for p in pts {
            check_pt(*p)?;
        }
        let mut added = 0;
        for w in pts.windows(2) {
            let [a, b] = *w else { continue };
            if libm::hypot(b[0] - a[0], b[1] - a[1]) < 1e-6 {
                continue;
            }
            let mut items = vec![
                Node::sym("wire"),
                list(vec![
                    Node::sym("pts"),
                    list(vec![Node::sym("xy"), Node::num(a[0]), Node::num(a[1])]),
                    list(vec![Node::sym("xy"), Node::num(b[0]), Node::num(b[1])]),
                ]),
                list(vec![Node::sym("stroke"), kv("width", Node::num(0.0)), kv("type", Node::sym("default"))]),
            ];
            items.push(self.new_uuid(&items));
            self.insert(list(items))?;
            added += 1;
        }
        if added == 0 {
            return Err(bad("the wire has no length"));
        }
        self.reread(limits)?;
        // Junctions at the new wire's ends and corners where they join a wire's middle,
        // and where another wire's end lands on the new wire.
        let eps = crate::netlist::EPS;
        let mut spots: Vec<Pt> = Vec::new();
        let first_new = self.wires.len().saturating_sub(added);
        let new: Vec<(Pt, Pt)> = self.wires.iter().skip(first_new).map(|w| (w.a, w.b)).collect();
        for &p in pts {
            let ends = self
                .wires
                .iter()
                .flat_map(|w| [w.a, w.b])
                .filter(|q| libm::hypot(q[0] - p[0], q[1] - p[1]) <= eps)
                .count();
            if self.on_wire_middle(p) || ends >= 3 {
                spots.push(p);
            }
        }
        for w in self.wires.iter().take(first_new) {
            for e in [w.a, w.b] {
                let inside = new.iter().any(|&(a, b)| {
                    let end = |q: Pt| libm::hypot(q[0] - e[0], q[1] - e[1]) <= eps;
                    !end(a) && !end(b) && crate::netlist::point_segment(e, a, b) <= eps
                });
                if inside {
                    spots.push(e);
                }
            }
        }
        spots.retain(|p| !self.junctions.iter().any(|j| libm::hypot(j.at[0] - p[0], j.at[1] - p[1]) <= eps));
        spots.dedup_by(|a, b| libm::hypot(a[0] - b[0], a[1] - b[1]) <= eps);
        if !spots.is_empty() {
            for p in spots {
                self.push_junction(p)?;
            }
            self.reread(limits)?;
        }
        Ok(added)
    }

    fn push_junction(&mut self, p: Pt) -> Result<(), Error> {
        let mut items = vec![
            Node::sym("junction"),
            at_node(p, None),
            kv("diameter", Node::num(0.0)),
            list(vec![Node::sym("color"), Node::num(0.0), Node::num(0.0), Node::num(0.0), Node::num(0.0)]),
        ];
        items.push(self.new_uuid(&items));
        self.insert(list(items))
    }

    /// Adds a junction dot.
    ///
    /// # Errors
    /// Bad coordinates or a failed re-read.
    pub fn add_junction(&mut self, p: Pt, limits: &Limits) -> Result<(), Error> {
        check_pt(p)?;
        self.push_junction(p)?;
        self.reread(limits)
    }

    /// Adds a no-connect flag.
    ///
    /// # Errors
    /// Bad coordinates or a failed re-read.
    pub fn add_no_connect(&mut self, p: Pt, limits: &Limits) -> Result<(), Error> {
        check_pt(p)?;
        let mut items = vec![Node::sym("no_connect"), at_node(p, None)];
        items.push(self.new_uuid(&items));
        self.insert(list(items))?;
        self.reread(limits)
    }

    /// Adds a label; global and hierarchical labels get `shape` (default
    /// `bidirectional`). Returns its index in [`Schematic::labels`].
    ///
    /// # Errors
    /// A bad name, place, angle or shape, or a failed re-read.
    pub fn add_label(
        &mut self,
        kind: LabelKind,
        text: &str,
        at: Pt,
        angle: f64,
        shape: Option<&str>,
        limits: &Limits,
    ) -> Result<usize, Error> {
        check_pt(at)?;
        check_name("label", text)?;
        let angle = right_angle(angle)?;
        let shape = shape.unwrap_or("bidirectional");
        if !matches!(shape, "input" | "output" | "bidirectional" | "tri_state" | "passive") {
            return Err(bad("the shape is input, output, bidirectional, tri_state or passive"));
        }
        let justify = if angle > 135.0 { "right" } else { "left" };
        let mut items = vec![Node::sym(kind.head()), Node::str(text)];
        if kind != LabelKind::Local {
            items.push(kv("shape", Node::sym(shape)));
        }
        items.push(at_node(at, Some(angle)));
        items.push(yes("fields_autoplaced", true));
        let j = if kind == LabelKind::Local { format!("{justify} bottom") } else { justify.to_string() };
        items.push(effects(Some(&j), false));
        items.push(self.new_uuid(&items));
        if kind == LabelKind::Global {
            items.push(list(vec![
                Node::sym("property"),
                Node::str("Intersheetrefs"),
                Node::str("${INTERSHEET_REFS}"),
                at_node(at, Some(0.0)),
                effects(None, true),
            ]));
        }
        let n = self.labels.len();
        self.insert(list(items))?;
        self.reread(limits)?;
        if self.labels.len() != n + 1 {
            return Err(bad("the new label did not read back"));
        }
        // Labels are in file order; the new one is the last before the trailing items.
        Ok(self.labels.iter().enumerate().max_by_key(|(_, l)| l.node).map_or(0, |(i, _)| i))
    }

    /// Deletes items; a library symbol no symbol uses any more goes too. Returns the
    /// number of items deleted.
    ///
    /// # Errors
    /// An item past its list, or a failed re-read.
    pub fn delete(&mut self, items: &[Item], limits: &Limits) -> Result<usize, Error> {
        let mut nodes = Vec::with_capacity(items.len());
        let mut gone_libs = Vec::new();
        for it in items {
            let node = match *it {
                Item::Symbol(i) => self.symbols.get(i).map(|s| {
                    gone_libs.push(s.lib_key().to_string());
                    s.node
                }),
                Item::Wire(i) => self.wires.get(i).map(|w| w.node),
                Item::Bus(i) => self.buses.get(i).map(|w| w.node),
                Item::Label(i) => self.labels.get(i).map(|l| l.node),
                Item::Junction(i) => self.junctions.get(i).map(|j| j.node),
                Item::NoConnect(i) => self.no_connects.get(i).map(|j| j.node),
            };
            nodes.push(node.ok_or_else(|| bad(format!("there is no {}", it.name())))?);
        }
        nodes.sort_unstable();
        nodes.dedup();
        let tree = self.tree.items_mut().ok_or_else(|| bad("not a schematic"))?;
        for &n in nodes.iter().rev() {
            if n < tree.len() && n > 0 {
                tree.remove(n);
            }
        }
        // Library symbols left unused.
        let used: std::collections::HashSet<String> = tree
            .iter()
            .filter(|n| n.head() == Some("symbol"))
            .filter_map(|n| n.child("lib_name").or_else(|| n.child("lib_id")).and_then(|l| l.arg(0)).map(String::from))
            .collect();
        if let Some(ls) = tree.iter_mut().find(|n| n.head() == Some("lib_symbols")).and_then(Node::items_mut) {
            ls.retain(|l| {
                l.head() != Some("symbol")
                    || l.arg(0).is_none_or(|name| used.contains(name) || !gone_libs.iter().any(|g| g == name))
            });
        }
        self.reread(limits)?;
        Ok(nodes.len())
    }
}

/// Shifts a node's `at` or its `pts` by `d`.
fn shift_node(items: &mut [Node], d: Pt) {
    let shift = |n: &mut Node| {
        let (Some(x), Some(y)) = (n.num_arg(0), n.num_arg(1)) else { return };
        if let Some(v) = n.items_mut() {
            if let Some(s) = v.get_mut(1) {
                *s = Node::num(x + d[0]);
            }
            if let Some(s) = v.get_mut(2) {
                *s = Node::num(y + d[1]);
            }
        }
    };
    for n in items.iter_mut() {
        match n.head() {
            Some("at") => shift(n),
            Some("pts") => {
                if let Some(v) = n.items_mut() {
                    v.iter_mut().skip(1).filter(|c| c.head() == Some("xy")).for_each(shift);
                }
            }
            _ => {}
        }
    }
}

/// A wire's end `e` (the other end `o`) whose connection point moves to `q`: the end's
/// new place and a segment to add. A horizontal wire keeps its row up to `q`'s column
/// and a vertical segment goes to `q` (and the other way for a vertical wire); a wire
/// that is neither, or whose end moves along it, just stretches.
fn dogleg(o: Pt, e: Pt, q: Pt) -> (Pt, Option<(Pt, Pt)>) {
    let eps = 1e-6;
    let horizontal = (o[1] - e[1]).abs() < eps;
    let vertical = (o[0] - e[0]).abs() < eps;
    let mid = if horizontal && (q[1] - e[1]).abs() >= eps {
        [q[0], e[1]]
    } else if vertical && (q[0] - e[0]).abs() >= eps {
        [e[0], q[1]]
    } else {
        return (q, None);
    };
    let d = |a: Pt, b: Pt| libm::hypot(a[0] - b[0], a[1] - b[1]);
    if d(mid, e) < eps {
        // The pin is straight off the wire's end: the wire stays, a segment is added.
        (e, Some((e, q)))
    } else if d(mid, o) < eps {
        // The wire would shrink to nothing: it takes the other direction instead.
        (q, None)
    } else {
        (mid, Some((mid, q)))
    }
}

impl crate::sch::Symbol {
    /// The inverse of [`crate::sch::Symbol::orient`].
    pub fn orient_inverse(&self, v: Pt) -> Pt {
        // `orient` is a rotation then a mirror; both are orthogonal, so the inverse is
        // the mirror then the opposite rotation.
        let m = match self.mirror {
            Mirror::None => v,
            Mirror::X => [v[0], -v[1]],
            Mirror::Y => [-v[0], v[1]],
        };
        let (s, c) = crate::sch::sin_cos(-self.angle);
        [m[0] * c + m[1] * s, -m[0] * s + m[1] * c]
    }
}

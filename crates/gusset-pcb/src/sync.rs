//! Schematic to board (ADR 0047 phase 2a): pad nets from the schematic's netlist by
//! reference and pin number, footprints added for parts the board lacks (only from
//! footprint files the caller finds, never bundled ones), and a report of what differs.

use crate::board::{Board, Error, Limits, Pt};
use crate::netlist::Netlist;
use crate::sexpr::{self, Node};
use std::collections::{BTreeMap, HashMap};

/// What an update did and what still differs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    /// Pads whose net changed.
    pub pads_changed: usize,
    /// Footprints added (references).
    pub added: Vec<String>,
    /// Parts with a footprint name whose file was not found: (reference, footprint).
    pub not_found: Vec<(String, String)>,
    /// Parts for the board without a footprint name.
    pub no_footprint: Vec<String>,
    /// Footprints on the board with no part in the schematic.
    pub extra: Vec<String>,
    /// (reference, board value, schematic value).
    pub value_differs: Vec<(String, String, String)>,
    /// (reference, board footprint, schematic footprint).
    pub footprint_differs: Vec<(String, String, String)>,
    /// Pins with no pad of that number: (reference, pin).
    pub pins_without_pad: Vec<(String, String)>,
}

fn bad(msg: impl Into<String>) -> Error {
    Error::Edit(msg.into())
}

/// The board's outline (or pads') extent, for placing new footprints beside it.
fn extent(board: &Board) -> [Pt; 2] {
    let (loops, _) = crate::geom::outline_loops(board, 1e-3);
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for p in loops.iter().flat_map(|l| l.polygon(0.1)).chain(board.pads.iter().map(|p| p.at)) {
        lo = [lo[0].min(p[0]), lo[1].min(p[1])];
        hi = [hi[0].max(p[0]), hi[1].max(p[1])];
    }
    if lo[0] > hi[0] { [[0.0, 0.0], [0.0, 0.0]] } else { [lo, hi] }
}

impl Board {
    /// Adds a footprint from a `.kicad_mod` text at `at` (top side, angle 0), named
    /// `lib_id`, with its reference and value set. Returns its index.
    ///
    /// # Errors
    /// A text that is not a footprint, bad names or place, or a failed re-read.
    pub fn add_footprint(
        &mut self,
        mod_text: &str,
        lib_id: &str,
        reference: &str,
        value: &str,
        at: Pt,
        limits: &Limits,
    ) -> Result<usize, Error> {
        if !at.iter().all(|v| v.is_finite() && v.abs() <= 10_000.0) {
            return Err(bad("the place must be within 10 m"));
        }
        for t in [lib_id, reference, value] {
            if t.chars().count() > 200 || t.chars().any(char::is_control) {
                return Err(bad("names are at most 200 characters, without control characters"));
            }
        }
        let fp = sexpr::parse(mod_text, &limits.sexpr)?;
        if !matches!(fp.head(), Some("footprint" | "module")) {
            return Err(bad("the footprint file does not hold a footprint"));
        }
        let mut items: Vec<Node> = vec![Node::sym("footprint"), Node::str(lib_id)];
        let mut has_layer = false;
        for c in fp.args().iter().skip(1) {
            match c.head() {
                Some("version" | "generator" | "generator_version" | "at" | "uuid" | "tstamp") => {}
                Some("layer") => {
                    has_layer = true;
                    items.push(list(vec![Node::sym("layer"), Node::str("F.Cu")]));
                    items.push(list(vec![Node::sym("at"), Node::num(at[0]), Node::num(at[1])]));
                }
                Some("property") if c.arg(0) == Some("Reference") => items.push(with_arg(c, 1, reference)),
                Some("property") if c.arg(0) == Some("Value") => items.push(with_arg(c, 1, value)),
                Some("fp_text") if c.arg(0) == Some("reference") => items.push(with_arg(c, 1, reference)),
                Some("fp_text") if c.arg(0) == Some("value") => items.push(with_arg(c, 1, value)),
                _ => items.push(c.clone()),
            }
        }
        if !has_layer {
            items.insert(2, list(vec![Node::sym("layer"), Node::str("F.Cu")]));
            items.insert(3, list(vec![Node::sym("at"), Node::num(at[0]), Node::num(at[1])]));
        }
        if self.version.is_some_and(|v| v >= 20_210_000) {
            let n = self.tree.items().len();
            let id = crate::edit::uuid_for(&format!("{lib_id} {reference}"), n);
            items.insert(3, list(vec![Node::sym("uuid"), Node::str(id)]));
        }
        let n = self.footprints.len();
        self.tree.items_mut().ok_or_else(|| bad("not a board"))?.push(Node::List(items));
        let tree = std::mem::take(&mut self.tree);
        *self = Board::from_tree(tree, limits)?;
        if self.footprints.len() != n + 1 {
            return Err(bad("the footprint did not read back"));
        }
        Ok(n)
    }

    /// Sets pad nets by reference and pad number (`None`: no net). Pads of a footprint
    /// in the map whose number is not in its map lose their net (mechanical pads without
    /// a number are left alone). Returns the number of pads changed.
    ///
    /// # Errors
    /// A failed re-read.
    pub fn assign_pad_nets(
        &mut self,
        nets: &HashMap<String, HashMap<String, String>>,
        limits: &Limits,
    ) -> Result<usize, Error> {
        let mut changed = 0;
        // Declare new nets first: that inserts top-level items, which would move the
        // footprints' positions below.
        let mut names: Vec<&String> = nets.values().flat_map(|m| m.values()).collect();
        names.sort();
        names.dedup();
        for n in names {
            self.net_node(Some(n))?;
        }
        let tree = std::mem::take(&mut self.tree);
        *self = Board::from_tree(tree, limits)?;
        let fps: Vec<(usize, String)> = self.footprints.iter().map(|f| (f.node, f.reference.clone())).collect();
        for (node, reference) in fps {
            let Some(map) = nets.get(&reference) else { continue };
            // The pads' new nets, in the order the footprint lists them.
            let pads: Vec<(usize, Option<String>)> = {
                let Some(fp) = self.tree.items().get(node) else { continue };
                fp.items()
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| c.head() == Some("pad"))
                    .filter_map(|(k, c)| {
                        let num = c.arg(0).unwrap_or("");
                        (!num.is_empty()).then(|| (k, map.get(num).cloned()))
                    })
                    .collect()
            };
            for (k, net) in pads {
                // The pad's net now, by name.
                let old = {
                    let pad = self.tree.items().get(node).and_then(|f| f.items().get(k));
                    pad.and_then(|p| p.child("net")).and_then(|n| {
                        let numbered = matches!(n.args().first(), Some(Node::Atom(a)) if !a.quoted);
                        if numbered {
                            let id: i64 = n.arg(0)?.parse().ok()?;
                            n.arg(1)
                                .map(str::to_string)
                                .or_else(|| self.nets.iter().find(|x| x.id == id).map(|x| x.name.clone()))
                        } else {
                            n.arg(0).map(str::to_string)
                        }
                    })
                };
                let old = old.filter(|n| !n.is_empty());
                if old == net {
                    continue;
                }
                let new = match net.as_deref() {
                    // Numbered boards write the name beside the number in pads.
                    Some(n) => self.net_node(Some(n))?.map(|mut node| {
                        if let Some(v) = node.items_mut()
                            && v.len() == 2
                            && self.numbered_nets
                        {
                            v.push(Node::str(n));
                        }
                        node
                    }),
                    None => None,
                };
                let Some(pad) = self
                    .tree
                    .items_mut()
                    .and_then(|v| v.get_mut(node))
                    .and_then(|f| f.items_mut())
                    .and_then(|v| v.get_mut(k))
                else {
                    continue;
                };
                let Some(v) = pad.items_mut() else { continue };
                match (v.iter().position(|c| c.head() == Some("net")), new) {
                    (Some(i), Some(n)) => {
                        if let Some(slot) = v.get_mut(i) {
                            *slot = n;
                        }
                    }
                    (Some(i), None) => {
                        v.remove(i);
                    }
                    (None, Some(n)) => {
                        // Before the pad's uuid / pinfunction when present, else at the end.
                        let at = v
                            .iter()
                            .position(|c| matches!(c.head(), Some("pinfunction" | "pintype" | "uuid" | "tstamp")))
                            .unwrap_or(v.len());
                        v.insert(at, n);
                    }
                    (None, None) => continue,
                }
                changed += 1;
            }
        }
        let tree = std::mem::take(&mut self.tree);
        *self = Board::from_tree(tree, limits)?;
        Ok(changed)
    }
}

fn list(v: Vec<Node>) -> Node {
    Node::List(v)
}

/// A copy of `n` with argument `i` replaced by the string `t`.
fn with_arg(n: &Node, i: usize, t: &str) -> Node {
    let mut n = n.clone();
    if let Some(slot) = n.items_mut().and_then(|v| v.get_mut(i + 1)) {
        *slot = Node::str(t);
    }
    n
}

/// Brings the board in line with the netlist: adds footprints the board lacks (when
/// `find` returns the footprint file's text for the part's `Lib:Name`), then sets every
/// pad's net. Parts marked "not on board" are left out.
///
/// # Errors
/// A failed edit (a footprint file that does not read, a failed re-read).
pub fn update_board(
    board: &mut Board,
    nl: &Netlist,
    find: &mut dyn FnMut(&str) -> Option<String>,
    limits: &Limits,
) -> Result<Report, Error> {
    let mut r = Report::default();
    let parts: BTreeMap<&str, &crate::netlist::Component> =
        nl.components.iter().filter(|c| c.on_board).map(|c| (c.reference.as_str(), c)).collect();
    // New footprints in a row right of the board.
    let [_, hi] = extent(board);
    let mut place = [hi[0] + 5.0, extent(board)[0][1].max(0.0)];
    for (reference, c) in &parts {
        if board.footprint_index(reference).is_some() {
            continue;
        }
        if c.footprint.is_empty() {
            r.no_footprint.push(reference.to_string());
            continue;
        }
        match find(&c.footprint) {
            Some(text) => {
                board.add_footprint(&text, &c.footprint, reference, &c.value, place, limits)?;
                r.added.push(reference.to_string());
                place[1] += 5.0;
            }
            None => r.not_found.push((reference.to_string(), c.footprint.clone())),
        }
    }
    for f in &board.footprints {
        if f.reference.is_empty() || f.attrs.iter().any(|a| a == "board_only") {
            continue;
        }
        match parts.get(f.reference.as_str()) {
            None => r.extra.push(f.reference.clone()),
            Some(c) => {
                if c.value != f.value {
                    r.value_differs.push((f.reference.clone(), f.value.clone(), c.value.clone()));
                }
                if !c.footprint.is_empty() && c.footprint != f.lib_id {
                    r.footprint_differs.push((f.reference.clone(), f.lib_id.clone(), c.footprint.clone()));
                }
            }
        }
    }
    // Pad nets.
    let mut nets: HashMap<String, HashMap<String, String>> = HashMap::new();
    for n in &nl.nets {
        for x in &n.nodes {
            if parts.contains_key(x.reference.as_str()) {
                nets.entry(x.reference.clone()).or_default().insert(x.pin.clone(), n.name.clone());
            }
        }
    }
    for (reference, pins) in &nets {
        let Some(i) = board.footprint_index(reference) else { continue };
        let Some(f) = board.footprints.get(i) else { continue };
        let numbers: Vec<&str> = f.pads.iter().filter_map(|&k| board.pads.get(k)).map(|p| p.number.as_str()).collect();
        for pin in pins.keys() {
            if !numbers.contains(&pin.as_str()) && r.pins_without_pad.len() < 1000 {
                r.pins_without_pad.push((reference.clone(), pin.clone()));
            }
        }
    }
    r.pins_without_pad.sort();
    r.pads_changed = board.assign_pad_nets(&nets, limits)?;
    Ok(r)
}

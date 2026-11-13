//! Net edits (ADR 0047 phase 3): a pad's net, renaming and merging nets, and new copper
//! zones, so a board drawn in Gusset from nothing gets the nets it is routed by.
//!
//! Like every edit, these change the S-expression tree and re-read it: items they do not
//! touch are written back as read.

use crate::board::{Board, Error, Limits, Pt, ZoneConnect, ZoneSettings};
use crate::sexpr::Node;
use std::collections::HashMap;

fn bad(msg: impl Into<String>) -> Error {
    Error::Edit(msg.into())
}

fn list(items: Vec<Node>) -> Node {
    Node::List(items)
}

fn kv(head: &str, v: Node) -> Node {
    list(vec![Node::sym(head), v])
}

/// A net name the files can hold: 1 to 200 characters, no control characters.
fn check_net(name: &str) -> Result<(), Error> {
    if name.is_empty() || name.chars().count() > 200 || name.chars().any(char::is_control) {
        return Err(bad("a net name is 1 to 200 characters, without control characters"));
    }
    Ok(())
}

/// What renaming one net changes in a node.
struct Rename<'a> {
    from: &'a str,
    to: &'a str,
    /// Numbered files: the old and the new number (the same when only the name changes).
    ids: Option<(String, String)>,
    changed: usize,
}

impl Rename<'_> {
    /// Iterative walk (no recursion on depth) over every node but the top-level net
    /// declarations, which the caller handles.
    fn walk(&mut self, root: &mut Node) {
        let mut stack: Vec<&mut Node> = vec![root];
        while let Some(n) = stack.pop() {
            let kind = match n.head() {
                Some("net") => 1,
                Some("net_name" | "add_net") => 2,
                _ => 0,
            };
            match kind {
                1 => self.net(n),
                2 => {
                    if n.arg(0) == Some(self.from)
                        && let Some(slot) = n.items_mut().and_then(|v| v.get_mut(1))
                    {
                        *slot = Node::str(self.to);
                        self.changed += 1;
                    }
                }
                _ => {
                    if let Some(v) = n.items_mut() {
                        stack.extend(v.iter_mut().skip(1).filter(|c| matches!(c, Node::List(_))));
                    }
                }
            }
        }
    }

    /// `(net 3 "GND")`, `(net 3)` or `(net "GND")`.
    fn net(&mut self, n: &mut Node) {
        let numbered = matches!(n.args().first(), Some(Node::Atom(a)) if !a.quoted)
            && n.arg(0).is_some_and(|t| t.parse::<i64>().is_ok());
        let Some(v) = n.items_mut() else { return };
        if numbered {
            let Some((old, new)) = &self.ids else { return };
            if v.get(1).and_then(Node::atom) != Some(old.as_str()) {
                return;
            }
            if let Some(slot) = v.get_mut(1) {
                *slot = Node::sym(new.clone());
            }
            if let Some(slot) = v.get_mut(2) {
                *slot = Node::str(self.to);
            }
            self.changed += 1;
        } else if v.get(1).and_then(Node::atom) == Some(self.from) {
            if let Some(slot) = v.get_mut(1) {
                *slot = Node::str(self.to);
            }
            self.changed += 1;
        }
    }
}

/// A new zone's settings.
#[derive(Debug, Clone, PartialEq)]
pub struct NewZone {
    pub outline: Vec<Pt>,
    /// More outlines of the same zone (KiCad's several polygons in one zone).
    pub more_outlines: Vec<Vec<Pt>>,
    pub layers: Vec<String>,
    pub net: Option<String>,
    pub settings: ZoneSettings,
}

impl Board {
    /// Renames net `from` to `to` on every pad, track, via and zone (and KiCad 5 net
    /// classes). When `to` already exists the two nets merge. Returns the number of
    /// references changed.
    ///
    /// # Errors
    /// No net `from`, a bad name, or a failed re-read.
    pub fn rename_net(&mut self, from: &str, to: &str, limits: &Limits) -> Result<usize, Error> {
        check_net(to)?;
        if from == to {
            return Ok(0);
        }
        let used = self.nets.iter().any(|n| n.name == from)
            || self.pads.iter().any(|p| self.net_name(&p.net).as_deref() == Some(from));
        if !used {
            return Err(bad("the board has no such net"));
        }
        let merge = self.nets.iter().find(|n| n.name == to).map(|n| n.id);
        let ids = if self.numbered_nets {
            let old = self.nets.iter().find(|n| n.name == from).map(|n| n.id).ok_or_else(|| bad("no such net"))?;
            Some((old.to_string(), merge.unwrap_or(old).to_string()))
        } else {
            None
        };
        let mut r = Rename { from, to, ids, changed: 0 };
        let items = self.tree.items_mut().ok_or_else(|| bad("not a board"))?;
        let mut drop_decl = None;
        for (i, n) in items.iter_mut().enumerate().skip(1) {
            if n.head() == Some("net") {
                // A declaration: `(net 3 "GND")` (numbered) or `(net "GND")`.
                let name_at = if r.ids.is_some() { 2 } else { 1 };
                if n.arg(name_at - 1) == Some(from) {
                    if merge.is_some() {
                        drop_decl = Some(i);
                    } else if let Some(slot) = n.items_mut().and_then(|v| v.get_mut(name_at)) {
                        *slot = Node::str(to);
                    }
                    r.changed += 1;
                }
                continue;
            }
            r.walk(n);
        }
        if let Some(i) = drop_decl
            && i < items.len()
        {
            items.remove(i);
        }
        let changed = r.changed;
        self.reread(limits)?;
        Ok(changed)
    }

    /// Sets the net of pads named by footprint reference and pad number (`None`: no net);
    /// every pad of that footprint with that number changes. Returns the pads changed.
    ///
    /// # Errors
    /// An unknown footprint or pad, a bad net name, or a failed re-read.
    pub fn set_pad_nets(
        &mut self,
        pads: &[(String, String)],
        net: Option<&str>,
        limits: &Limits,
    ) -> Result<usize, Error> {
        if let Some(n) = net {
            check_net(n)?;
        }
        let mut map: HashMap<String, HashMap<String, String>> = HashMap::new();
        for (reference, number) in pads {
            let fps: Vec<&crate::board::Footprint> =
                self.footprints.iter().filter(|f| &f.reference == reference).collect();
            if fps.is_empty() {
                return Err(bad(format!("there is no footprint {}", reference.chars().take(40).collect::<String>())));
            }
            let has = fps.iter().any(|f| f.pads.iter().any(|&p| self.pads.get(p).is_some_and(|x| &x.number == number)));
            if !has {
                return Err(bad(format!(
                    "{} has no pad {}",
                    reference.chars().take(40).collect::<String>(),
                    number.chars().take(20).collect::<String>()
                )));
            }
            // Every numbered pad of the footprint keeps its net unless named here.
            let m = map.entry(reference.clone()).or_insert_with(|| {
                let mut m = HashMap::new();
                for f in &fps {
                    for p in f.pads.iter().filter_map(|&p| self.pads.get(p)) {
                        if let Some(name) = self.net_name(&p.net)
                            && !p.number.is_empty()
                        {
                            m.insert(p.number.clone(), name);
                        }
                    }
                }
                m
            });
            match net {
                Some(n) => m.insert(number.clone(), n.to_string()),
                None => m.remove(number),
            };
        }
        self.assign_pad_nets(&map, limits)
    }

    /// Adds a copper zone (filled later by [`crate::zones::fill`]); returns its index in
    /// [`Board::zones`].
    ///
    /// # Errors
    /// Fewer than 3 or bad points, a layer that is not copper, or a failed re-read.
    pub fn add_zone(&mut self, z: &NewZone, limits: &Limits) -> Result<usize, Error> {
        let all: Vec<Vec<Pt>> = std::iter::once(z.outline.clone()).chain(z.more_outlines.iter().cloned()).collect();
        check_outlines(&all)?;
        if z.layers.is_empty() {
            return Err(bad("a zone needs a copper layer"));
        }
        for l in &z.layers {
            self.copper_layer(l)?;
        }
        let st = &z.settings;
        let lengths = [st.clearance, st.min_width, st.thermal_gap, st.thermal_width];
        if lengths.iter().any(|v| !(v.is_finite() && (0.0..=100.0).contains(v))) {
            return Err(bad("zone clearances and widths are 0 to 100 mm"));
        }
        if let Some(n) = &z.net {
            check_net(n)?;
        }
        let mut items = vec![Node::sym("zone")];
        items.extend(self.net_node(z.net.as_deref())?);
        items.push(kv("net_name", Node::str(z.net.clone().unwrap_or_default())));
        if let [one] = z.layers.as_slice() {
            items.push(kv("layer", Node::str(one.clone())));
        } else {
            let mut v = vec![Node::sym("layers")];
            v.extend(z.layers.iter().map(|l| Node::str(l.clone())));
            items.push(list(v));
        }
        let text = format!("zone {:?} {:?} {:?} {:?}", z.outline, z.more_outlines, z.layers, z.net);
        if self.writes_uuids() {
            items.push(kv("uuid", Node::str(crate::edit::uuid_for(&text, self.tree.items().len()))));
        }
        items.push(list(vec![Node::sym("hatch"), Node::sym("edge"), Node::num(0.5)]));
        if st.priority != 0 {
            items.push(kv("priority", Node::num(st.priority as f64)));
        }
        let mut cp = vec![Node::sym("connect_pads")];
        match st.connect {
            ZoneConnect::Thermal => {}
            ZoneConnect::Solid => cp.push(Node::sym("yes")),
            ZoneConnect::None => cp.push(Node::sym("no")),
            ZoneConnect::ThtThermal => cp.push(Node::sym("thru_hole_only")),
        }
        cp.push(kv("clearance", Node::num(st.clearance)));
        items.push(list(cp));
        items.push(kv("min_thickness", Node::num(st.min_width)));
        items.push(kv("filled_areas_thickness", Node::sym("no")));
        let mut fill = vec![Node::sym("fill")];
        if let Some(h) = st.hatch {
            fill.push(kv("mode", Node::sym("hatch")));
            fill.push(kv("thermal_gap", Node::num(st.thermal_gap)));
            fill.push(kv("thermal_bridge_width", Node::num(st.thermal_width)));
            fill.push(kv("hatch_thickness", Node::num(h.thickness)));
            fill.push(kv("hatch_gap", Node::num(h.gap)));
            fill.push(kv("hatch_orientation", Node::num(h.angle)));
            fill.push(kv("hatch_smoothing_level", Node::num(0.0)));
            fill.push(kv("hatch_smoothing_value", Node::num(0.1)));
            fill.push(kv("hatch_border_algorithm", Node::sym("hatch_thickness")));
            fill.push(kv("hatch_min_hole_area", Node::num(h.min_hole_area)));
        } else {
            fill.push(kv("thermal_gap", Node::num(st.thermal_gap)));
            fill.push(kv("thermal_bridge_width", Node::num(st.thermal_width)));
        }
        if st.island_mode != 0 {
            fill.push(kv("island_removal_mode", Node::num(f64::from(st.island_mode))));
            fill.push(kv("island_area_min", Node::num(st.island_min_area)));
        }
        items.push(list(fill));
        items.extend(all.iter().map(|o| polygon_node(o)));
        let n = self.zones.len();
        let node = list(items);
        // Zones go after the tracks, as KiCad writes them.
        let all = self.tree.items_mut().ok_or_else(|| bad("not a board"))?;
        let at = all
            .iter()
            .rposition(|c| matches!(c.head(), Some("zone" | "segment" | "arc" | "via")))
            .map_or(all.len(), |i| i + 1);
        all.insert(at, node);
        self.reread(limits)?;
        if self.zones.len() != n + 1 {
            return Err(bad("the zone did not read back"));
        }
        // The new zone is the one at the inserted place.
        self.zones.iter().position(|z| z.node == at).ok_or_else(|| bad("the zone did not read back"))
    }

    /// Replaces the outlines of zone `zone` (the first is the zone's main polygon). The
    /// zone's fills are dropped (they no longer match); fill it again.
    ///
    /// # Errors
    /// No such zone, an outline of fewer than 3 or bad points, or a failed re-read.
    pub fn set_zone_outlines(&mut self, zone: usize, outlines: &[Vec<Pt>], limits: &Limits) -> Result<(), Error> {
        check_outlines(outlines)?;
        let node = self.zones.get(zone).ok_or_else(|| bad(format!("there is no zone {zone}")))?.node;
        let items = self.tree.items_mut().and_then(|v| v.get_mut(node)).and_then(Node::items_mut);
        let items = items.ok_or_else(|| bad("not a board"))?;
        let at = items.iter().position(|c| c.head() == Some("polygon")).unwrap_or(items.len());
        items.retain(|c| !matches!(c.head(), Some("polygon" | "filled_polygon" | "fill_segments")));
        let at = at.min(items.len());
        for (k, o) in outlines.iter().enumerate() {
            items.insert(at + k, polygon_node(o));
        }
        self.reread(limits)
    }

    /// Deletes zones by index in [`Board::zones`]; returns how many went.
    ///
    /// # Errors
    /// An index past the list, or a failed re-read.
    pub fn delete_zones(&mut self, zones: &[usize], limits: &Limits) -> Result<usize, Error> {
        let mut nodes = Vec::new();
        for &i in zones {
            nodes.push(self.zones.get(i).ok_or_else(|| bad(format!("there is no zone {i}")))?.node);
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

/// Outlines of a zone: 1 to 1000, each 3 to 100000 finite points within 10 m.
fn check_outlines(all: &[Vec<Pt>]) -> Result<(), Error> {
    if all.is_empty() || all.len() > 1000 {
        return Err(bad("a zone has 1 to 1000 outlines"));
    }
    if all.iter().any(|o| o.len() < 3 || o.len() > 100_000) {
        return Err(bad("a zone outline has 3 to 100000 points"));
    }
    if all.iter().flatten().flatten().any(|v| !v.is_finite() || v.abs() > 10_000.0) {
        return Err(bad("coordinates must be finite and within 10 m"));
    }
    Ok(())
}

/// `(polygon (pts (xy x y) ...))`.
fn polygon_node(o: &[Pt]) -> Node {
    let mut pts = vec![Node::sym("pts")];
    pts.extend(o.iter().map(|p| list(vec![Node::sym("xy"), Node::num(p[0]), Node::num(p[1])])));
    list(vec![Node::sym("polygon"), list(pts)])
}

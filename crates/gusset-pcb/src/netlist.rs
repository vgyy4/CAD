//! Nets from a schematic design (ADR 0047 phase 2a): wires, junctions, labels (local,
//! global, hierarchical through sheet pins), power symbols and pins, across the sheet
//! hierarchy.
//!
//! Connection rules (read from KiCad's documentation and checked against the nets of
//! KiCad's demo boards): items connect where their connection points meet; a point
//! (pin, label, sheet pin, junction, no-connect flag, wire end) on a wire, ends or middle,
//! connects to it; wires that only cross do not connect (a junction on the crossing
//! does). Local labels join equal names on one sheet (in one use of it), global labels
//! and power symbols (by value) join across the design, hierarchical labels join the
//! parent sheet's pin of the same name, and hidden power-input pins of ordinary symbols
//! join the global net of their pin name.
//!
//! Buses (phase 2b): a bus sheet pin (`D[0..7]`, `{SDA SCL}`, `I2C{SDA SCL}`) joins the
//! child sheet's members (its local labels `D0`..`D7`) to the parent's members of the
//! same names. Sheet pins on one bus of the parent join member by member in order (vectors
//! of one width; other widths by name), named by the bus's own label when it has one (as
//! a local label of the parent sheet), else by its first pin.
//!
//! Net names follow KiCad's: a power or global name as written; a local label as the
//! sheet path plus the label (`/sub/SDA`, `/SDA` on the root sheet); otherwise
//! `Net-(R1-Pad2)` (or the pin name, `Net-(U1-CLK)`), or `unconnected-(…-PadN)` for a
//! lone pin.

use crate::board::Pt;
use crate::sch::{LabelKind, PinType, Schematic};
use std::collections::{BTreeMap, HashMap};

/// Points closer than this (mm) are one point (files round to 0.0001 mm or coarser).
pub const EPS: f64 = 1e-3;
/// Most wire-grid entries a netlist builds (bounds work on hostile files).
const MAX_GRID: usize = 8_000_000;
/// Deepest sheet nesting followed, and most sheet uses.
const MAX_DEPTH: usize = 32;
const MAX_SHEET_USES: usize = 4096;

/// A design: the root sheet first, then the sheets it uses (by file name).
#[derive(Debug, Clone, Default)]
pub struct Design {
    pub sheets: Vec<(String, Schematic)>,
}

/// One use of a sheet file in the hierarchy.
#[derive(Debug, Clone, PartialEq)]
pub struct SheetUse {
    /// Index into [`Design::sheets`].
    pub sheet: usize,
    /// Sheet uuids from the root (`/root` or `/root/sheet`), as in symbol instances.
    pub path: String,
    /// Sheet names from the root, ending in `/` (`/` for the root).
    pub name_path: String,
    /// The parent use and the index of the sheet item in its schematic.
    pub parent: Option<(usize, usize)>,
}

impl Design {
    /// A design of one sheet.
    pub fn single(file: String, s: Schematic) -> Design {
        Design { sheets: vec![(file, s)] }
    }

    /// The sheet with this file name.
    pub fn sheet_named(&self, file: &str) -> Option<usize> {
        self.sheets.iter().position(|(f, _)| f == file)
    }

    /// Every use of every sheet, root first (depth first); sheets whose file is not in
    /// the design, deeper than 32 or recursive are left out.
    pub fn uses(&self) -> Vec<SheetUse> {
        let Some((_, root)) = self.sheets.first() else { return Vec::new() };
        let mut out = vec![SheetUse {
            sheet: 0,
            path: format!("/{}", root.uuid.as_deref().unwrap_or("")),
            name_path: "/".into(),
            parent: None,
        }];
        let mut stack = vec![(0usize, 0usize)];
        while let Some((u, depth)) = stack.pop() {
            let Some(cur) = out.get(u).cloned() else { continue };
            let Some((_, s)) = self.sheets.get(cur.sheet) else { continue };
            if depth >= MAX_DEPTH {
                continue;
            }
            for (k, sh) in s.sheets.iter().enumerate() {
                let Some(child) = self.sheet_named(&sh.file) else { continue };
                // A sheet that uses itself (directly or above) is not followed.
                let mut up = Some(u);
                let mut cycle = false;
                while let Some(x) = up {
                    let Some(p) = out.get(x) else { break };
                    if p.sheet == child {
                        cycle = true;
                        break;
                    }
                    up = p.parent.map(|(i, _)| i);
                }
                if cycle || out.len() >= MAX_SHEET_USES {
                    continue;
                }
                out.push(SheetUse {
                    sheet: child,
                    path: format!("{}/{}", cur.path, sh.uuid.as_deref().unwrap_or("")),
                    name_path: format!("{}{}/", cur.name_path, sh.name),
                    parent: Some((u, k)),
                });
                stack.push((out.len() - 1, depth + 1));
            }
        }
        out
    }
}

/// A component pin on a net.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub reference: String,
    pub pin: String,
    pub pin_name: String,
    pub kind: PinType,
    /// Where: sheet use, symbol index in that sheet, pin position.
    pub sheet_use: usize,
    pub symbol: usize,
    pub at: Pt,
}

/// A net.
#[derive(Debug, Clone, PartialEq)]
pub struct Net {
    pub name: String,
    /// Named by a label or power symbol (not derived from a pin).
    pub named: bool,
    pub nodes: Vec<Node>,
    /// Power symbols on it (their pins are not component pins).
    pub power_symbols: usize,
    /// Labels and sheet pins on it.
    pub labels: usize,
    /// No-connect flags on it.
    pub no_connects: usize,
    /// A power-output pin is on it (a component's, or a `PWR_FLAG`).
    pub power_out: bool,
    /// Where its labels and no-connect flags are: (sheet use, place, label text or
    /// empty for a flag); at most 16.
    pub marks: Vec<(usize, Pt, String)>,
}

/// A component (all units of one reference).
#[derive(Debug, Clone, PartialEq)]
pub struct Component {
    pub reference: String,
    pub value: String,
    pub footprint: String,
    pub lib_id: String,
    pub on_board: bool,
    pub in_bom: bool,
    pub dnp: bool,
    /// (sheet use, symbol index) of each unit placed.
    pub units: Vec<(usize, usize)>,
}

/// Nets and components of a design.
#[derive(Debug, Clone, Default)]
pub struct Netlist {
    pub uses: Vec<SheetUse>,
    pub nets: Vec<Net>,
    pub components: Vec<Component>,
    /// For each sheet use, the net of each wire (index into `nets`).
    pub wire_nets: Vec<Vec<Option<usize>>>,
    /// Problems found while connecting (too many items...).
    pub notes: Vec<String>,
}

impl Netlist {
    /// The net of a component pin.
    pub fn net_of(&self, reference: &str, pin: &str) -> Option<&Net> {
        self.nets.iter().find(|n| n.nodes.iter().any(|x| x.reference == reference && x.pin == pin))
    }

    /// (reference, pin) → net name, for every component pin.
    pub fn pin_map(&self) -> HashMap<(String, String), String> {
        let mut m = HashMap::new();
        for n in &self.nets {
            for x in &n.nodes {
                m.insert((x.reference.clone(), x.pin.clone()), n.name.clone());
            }
        }
        m
    }
}

struct Dsu(Vec<usize>);

impl Dsu {
    fn new(n: usize) -> Dsu {
        Dsu((0..n).collect())
    }
    fn find(&mut self, mut x: usize) -> usize {
        while let Some(&p) = self.0.get(x) {
            if p == x {
                break;
            }
            let gp = self.0.get(p).copied().unwrap_or(p);
            if let Some(slot) = self.0.get_mut(x) {
                *slot = gp;
            }
            x = p;
        }
        x
    }
    fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b
            && let Some(slot) = self.0.get_mut(a.max(b))
        {
            *slot = a.min(b);
        }
    }
}

/// What an item of the connection graph is.
#[derive(Debug, Clone)]
enum Item {
    /// sheet use, symbol, pin number, pin name, type, at, power symbol, hidden.
    Pin {
        u: usize,
        sym: usize,
        number: String,
        name: String,
        kind: PinType,
        at: Pt,
        power: bool,
        hidden: bool,
    },
    Label {
        u: usize,
        kind: LabelKind,
        text: String,
        at: Pt,
    },
    SheetPin {
        u: usize,
        sheet: usize,
        name: String,
    },
    NoConnect {
        u: usize,
        at: Pt,
    },
    Other,
}

/// Distance from `p` to segment `a`-`b`.
pub fn point_segment(p: Pt, a: Pt, b: Pt) -> f64 {
    crate::copper::point_segment(p, a, b).0
}

fn key(p: Pt) -> (i64, i64) {
    ((p[0] / EPS).round() as i64, (p[1] / EPS).round() as i64)
}

const CELL: f64 = 2.54;

fn cell(p: Pt) -> (i64, i64) {
    ((p[0] / CELL).floor() as i64, (p[1] / CELL).floor() as i64)
}

/// Wires of one sheet bucketed on a grid.
struct Grid {
    cells: HashMap<(i64, i64), Vec<usize>>,
}

impl Grid {
    fn new(wires: &[(Pt, Pt)], budget: &mut usize) -> Option<Grid> {
        let mut cells: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
        for (i, &(a, b)) in wires.iter().enumerate() {
            let len = libm::hypot(b[0] - a[0], b[1] - a[1]);
            let steps = (len / (CELL / 2.0)).ceil() as usize + 1;
            *budget = budget.checked_sub(steps)?;
            let mut last = None;
            for k in 0..=steps {
                let t = k as f64 / steps as f64;
                let c = cell([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
                if last != Some(c) {
                    let v = cells.entry(c).or_default();
                    if v.last() != Some(&i) {
                        v.push(i);
                    }
                    last = Some(c);
                }
            }
        }
        Some(Grid { cells })
    }

    /// Wires through `p` (within [`EPS`]).
    fn at(&self, p: Pt, wires: &[(Pt, Pt)], out: &mut Vec<usize>) {
        out.clear();
        let (cx, cy) = cell(p);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for &w in self.cells.get(&(cx + dx, cy + dy)).into_iter().flatten() {
                    if let Some(&(a, b)) = wires.get(w)
                        && point_segment(p, a, b) <= EPS
                        && !out.contains(&w)
                    {
                        out.push(w);
                    }
                }
            }
        }
    }
}

/// Escapes a name part the way KiCad writes net names (`/` → `{slash}`).
/// For each bus segment, a representative of the segments it touches (end to end or an
/// end on another segment).
fn bus_groups(buses: &[crate::sch::Wire]) -> Vec<usize> {
    let mut dsu = Dsu::new(buses.len());
    for (i, a) in buses.iter().enumerate() {
        for (j, b) in buses.iter().enumerate().skip(i + 1) {
            let touch = [a.a, a.b].iter().any(|p| point_segment(*p, b.a, b.b) < 1e-6)
                || [b.a, b.b].iter().any(|p| point_segment(*p, a.a, a.b) < 1e-6);
            if touch {
                dsu.union(i, j);
            }
        }
    }
    (0..buses.len()).map(|i| dsu.find(i)).collect()
}

/// The members of a bus name: `D[0..7]` gives `D0`..`D7`, `{SDA SCL}` gives `SDA` and
/// `SCL`, `I2C{SDA SCL}` gives `I2C.SDA` and `I2C.SCL`, groups may hold vectors. Plain
/// names have none. At most 1024 members.
pub fn bus_members(name: &str) -> Vec<String> {
    const MAX: usize = 1024;
    let vector = |t: &str| -> Option<Vec<String>> {
        let (prefix, rest) = t.split_once('[')?;
        let range = rest.strip_suffix(']')?;
        let (a, b) = range.split_once("..")?;
        let (a, b): (i64, i64) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
        if (a - b).unsigned_abs() >= MAX as u64 {
            return None;
        }
        let (lo, hi) = (a.min(b), a.max(b));
        Some((lo..=hi).map(|i| format!("{prefix}{i}")).collect())
    };
    if let Some(open) = name.find('{')
        && let Some(inner) = name.get(open + 1..).and_then(|r| r.strip_suffix('}'))
    {
        let group = name.get(..open).unwrap_or_default();
        let mut out = Vec::new();
        for part in inner.split_whitespace() {
            let members = vector(part).unwrap_or_else(|| vec![part.to_string()]);
            for m in members {
                if out.len() < MAX {
                    out.push(if group.is_empty() { m } else { format!("{group}.{m}") });
                }
            }
        }
        return out;
    }
    vector(name).unwrap_or_default()
}

fn escape(s: &str) -> String {
    s.replace('/', "{slash}")
}

/// The unit letter KiCad appends to a multi-unit reference (`U1A`).
fn unit_letter(unit: u32) -> String {
    let mut n = unit.max(1);
    let mut s = Vec::new();
    while n > 0 {
        let r = ((n - 1) % 26) as u8;
        s.push(char::from(b'A' + r));
        n = (n - 1) / 26;
    }
    s.iter().rev().collect()
}

/// Natural order of references and pin numbers (`R2` before `R10`).
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let split = |s: &str| {
        let t = s.trim_end_matches(|c: char| c.is_ascii_digit());
        (t.to_string(), s.get(t.len()..).and_then(|d| d.parse::<u64>().ok()).unwrap_or(0))
    };
    split(a).cmp(&split(b)).then_with(|| a.cmp(b))
}

/// Whether a pin has a name of its own (not empty, `~` or its number).
fn pin_named(n: &Node) -> bool {
    !n.pin_name.is_empty() && n.pin_name != "~" && n.pin_name != n.pin
}

/// Builds the netlist of a design.
pub fn netlist(design: &Design) -> Netlist {
    let uses = design.uses();
    let mut nl = Netlist { uses: uses.clone(), ..Netlist::default() };
    let mut items: Vec<Item> = Vec::new();
    // Per use: the first item index of its wires.
    let mut wire_base: Vec<(usize, usize)> = Vec::new();
    let mut unions: Vec<(usize, usize)> = Vec::new();
    let mut budget = MAX_GRID;
    for (u, su) in uses.iter().enumerate() {
        let Some((_, s)) = design.sheets.get(su.sheet) else { continue };
        let wires: Vec<(Pt, Pt)> = s.wires.iter().map(|w| (w.a, w.b)).collect();
        let base = items.len();
        wire_base.push((base, wires.len()));
        items.extend(wires.iter().map(|_| Item::Other));
        // Points of this sheet use.
        let mut points: Vec<(Pt, usize)> = Vec::new();
        for (k, sym) in s.symbols.iter().enumerate() {
            let power = s.is_power(sym);
            for p in s.pins(sym) {
                items.push(Item::Pin {
                    u,
                    sym: k,
                    number: p.number,
                    name: p.name,
                    kind: p.kind,
                    at: p.at,
                    power,
                    hidden: p.hidden,
                });
                points.push((p.at, items.len() - 1));
            }
        }
        for l in &s.labels {
            items.push(Item::Label { u, kind: l.kind, text: l.text.clone(), at: l.at });
            points.push((l.at, items.len() - 1));
        }
        for (k, sh) in s.sheets.iter().enumerate() {
            for p in &sh.pins {
                items.push(Item::SheetPin { u, sheet: k, name: p.name.clone() });
                points.push((p.at, items.len() - 1));
            }
        }
        for j in &s.junctions {
            items.push(Item::Other);
            points.push((j.at, items.len() - 1));
        }
        for n in &s.no_connects {
            items.push(Item::NoConnect { u, at: n.at });
            points.push((n.at, items.len() - 1));
        }
        let Some(grid) = Grid::new(&wires, &mut budget) else {
            nl.notes.push("the schematic has too many wires to connect".into());
            break;
        };
        let mut found = Vec::new();
        for &(p, it) in &points {
            grid.at(p, &wires, &mut found);
            unions.extend(found.iter().map(|&w| (it, base + w)));
        }
        for (i, &(a, b)) in wires.iter().enumerate() {
            for e in [a, b] {
                grid.at(e, &wires, &mut found);
                unions.extend(found.iter().map(|&w| (base + i, base + w)));
            }
        }
        let mut at_point: HashMap<(i64, i64), usize> = HashMap::new();
        for &(p, it) in &points {
            match at_point.get(&key(p)) {
                Some(&first) => unions.push((first, it)),
                None => {
                    at_point.insert(key(p), it);
                }
            }
        }
    }
    let mut dsu = Dsu::new(items.len());
    for (a, b) in unions {
        dsu.union(a, b);
    }
    // Names that join items: local labels per use, global names, hierarchical links.
    let mut by_name: HashMap<(Option<usize>, String), usize> = HashMap::new();
    let mut join = |dsu: &mut Dsu, k: (Option<usize>, String), it: usize| match by_name.get(&k) {
        Some(&first) => dsu.union(first, it),
        None => {
            by_name.insert(k, it);
        }
    };
    let power_name = |design: &Design, u: usize, sym: usize, pin_name: &str| -> String {
        let s = uses.get(u).and_then(|su| design.sheets.get(su.sheet)).map(|(_, s)| s);
        let v = s.and_then(|s| s.symbols.get(sym)).map(|x| x.value().to_string()).unwrap_or_default();
        if v.is_empty() { pin_name.to_string() } else { v }
    };
    // Hierarchical labels per (use, name), sheet pins per (parent use, sheet, name).
    let mut hier: HashMap<(usize, String), usize> = HashMap::new();
    let mut sheet_pins: Vec<(usize, usize, String, usize)> = Vec::new();
    for (it, item) in items.iter().enumerate() {
        match item {
            // Names compare as KiCad writes them (`/` is `{slash}` in some files).
            Item::Label { u, kind: LabelKind::Local, text, .. } => join(&mut dsu, (Some(*u), escape(text)), it),
            Item::Label { kind: LabelKind::Global, text, .. } => join(&mut dsu, (None, escape(text)), it),
            Item::Label { u, kind: LabelKind::Hierarchical, text, .. } => {
                if let Some(&first) = hier.get(&(*u, escape(text))) {
                    dsu.union(first, it);
                } else {
                    hier.insert((*u, escape(text)), it);
                }
            }
            Item::SheetPin { u, sheet, name } => sheet_pins.push((*u, *sheet, escape(name), it)),
            // A power symbol's power-input pin names a global net (not PWR_FLAG's output).
            Item::Pin { u, sym, name, power: true, kind: PinType::PowerIn, .. } => {
                let n = power_name(design, *u, *sym, name);
                join(&mut dsu, (None, escape(&n)), it);
            }
            Item::Pin { name, kind: PinType::PowerIn, hidden: true, power: false, .. } if !name.is_empty() => {
                join(&mut dsu, (None, escape(name)), it);
            }
            _ => {}
        }
    }
    // Bus members named on a sheet by its bus (not by a label there): (an item of the
    // member's net, the sheet use, the member's name), offered as names like local labels.
    let mut bus_names: Vec<(usize, usize, String)> = Vec::new();
    // A child's bus member `item` takes the parent-sheet name `name` (joined like a local
    // label of that sheet use).
    let mut name_member =
        |dsu: &mut Dsu, by_name: &mut HashMap<(Option<usize>, String), usize>, u: usize, name: String, item: usize| {
            match by_name.get(&(Some(u), name.clone())) {
                Some(&a) => dsu.union(a, item),
                None => {
                    by_name.insert((Some(u), name.clone()), item);
                    if bus_names.len() < 100_000 {
                        bus_names.push((item, u, name));
                    }
                }
            }
        };
    // Buses on each sheet use: groups of touching segments, the vector bus labels on them,
    // the sheet pins on them.
    let mut on_bus: std::collections::HashSet<(usize, usize, String)> = std::collections::HashSet::new();
    for (u, su) in uses.iter().enumerate() {
        let Some((_, s)) = design.sheets.get(su.sheet) else { continue };
        if s.buses.is_empty() || s.buses.len() > 20_000 {
            continue;
        }
        let groups = bus_groups(&s.buses);
        let group_at =
            |p: Pt| s.buses.iter().position(|b| point_segment(p, b.a, b.b) < 1e-6).and_then(|g| groups.get(g).copied());
        let mut labels: HashMap<usize, Vec<String>> = HashMap::new();
        for l in s.labels.iter().filter(|l| l.kind == LabelKind::Local) {
            if let Some(g) = group_at(l.at)
                && !bus_members(&escape(&l.text)).is_empty()
            {
                labels.entry(g).or_default().push(escape(&l.text));
            }
        }
        let mut pins: HashMap<usize, Vec<(usize, String)>> = HashMap::new();
        for (si, sh) in s.sheets.iter().enumerate() {
            let Some(child) = uses.iter().position(|c| c.parent == Some((u, si))) else { continue };
            for pin in &sh.pins {
                if let Some(g) = group_at(pin.at) {
                    on_bus.insert((u, si, escape(&pin.name)));
                    pins.entry(g).or_default().push((child, escape(&pin.name)));
                }
            }
        }
        for (g, ps) in &mut pins {
            ps.sort();
            // The bus's own name: its label (the first in name order), else its first pin.
            let mut names = labels.get(g).cloned().unwrap_or_default();
            names.sort();
            let naming = names.first().or_else(|| ps.first().map(|p| &p.1)).map(|n| bus_members(n)).unwrap_or_default();
            for (child, pname) in ps.iter() {
                let ms = bus_members(pname);
                // Vectors of one width join member by member, in order; others by name.
                let positional = ms.len() == naming.len();
                for (k, m) in ms.iter().enumerate() {
                    let Some(&item) = by_name.get(&(Some(*child), m.clone())) else { continue };
                    let target =
                        if positional { naming.get(k).cloned().unwrap_or_else(|| m.clone()) } else { m.clone() };
                    name_member(&mut dsu, &mut by_name, u, target, item);
                }
            }
        }
    }
    for (u, sheet, name, it) in &sheet_pins {
        let (u, sheet) = (*u, *sheet);
        // The use of that sheet item under use `u`.
        let Some(child) = uses.iter().position(|c| c.parent == Some((u, sheet))) else { continue };
        if let Some(&l) = hier.get(&(child, name.clone())) {
            dsu.union(l, *it);
        }
        // A bus pin not on a bus of the parent: its members join the parent's labels of
        // the same names.
        if on_bus.contains(&(u, sheet, name.clone())) {
            continue;
        }
        for m in bus_members(name) {
            if let Some(&b) = by_name.get(&(Some(child), m.clone())) {
                name_member(&mut dsu, &mut by_name, u, m, b);
            }
        }
    }
    // Components.
    let mut comps: BTreeMap<String, Component> = BTreeMap::new();
    let mut multi_unit: HashMap<(usize, usize), bool> = HashMap::new();
    for (u, su) in uses.iter().enumerate() {
        let Some((_, s)) = design.sheets.get(su.sheet) else { continue };
        for (k, sym) in s.symbols.iter().enumerate() {
            let r = sym.reference_in(&su.path);
            if s.is_power(sym) || r.starts_with('#') || r.is_empty() {
                continue;
            }
            multi_unit.insert((u, k), s.lib_symbol(sym).is_some_and(|l| l.unit_count() > 1));
            let c = comps.entry(r.to_string()).or_insert_with(|| Component {
                reference: r.to_string(),
                value: sym.value().to_string(),
                footprint: sym.footprint().to_string(),
                lib_id: sym.lib_id.clone(),
                on_board: sym.on_board,
                in_bom: sym.in_bom,
                dnp: sym.dnp,
                units: Vec::new(),
            });
            c.units.push((u, k));
        }
    }
    let mut comps: Vec<Component> = comps.into_values().collect();
    comps.sort_by(|a, b| natural_cmp(&a.reference, &b.reference));
    nl.components = comps;
    // Group items into nets.
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for it in 0..items.len() {
        let r = dsu.find(it);
        groups.entry(r).or_default().push(it);
    }
    let mut bus_named: HashMap<usize, Vec<(usize, String)>> = HashMap::new();
    for (it, u, m) in bus_names {
        bus_named.entry(dsu.find(it)).or_default().push((u, m));
    }
    let mut net_of_root: HashMap<usize, usize> = HashMap::new();
    let unit_of = |n: &Node| {
        uses.get(n.sheet_use)
            .and_then(|su| design.sheets.get(su.sheet))
            .and_then(|(_, s)| s.symbols.get(n.symbol))
            .map_or(1, |s| s.unit)
    };
    for (root, members) in groups {
        let mut net = Net {
            name: String::new(),
            named: false,
            nodes: Vec::new(),
            power_symbols: 0,
            labels: 0,
            no_connects: 0,
            power_out: false,
            marks: Vec::new(),
        };
        // (priority, depth, name) of the best name source: power and global names first,
        // then local labels, then hierarchical labels, each the highest in the hierarchy
        // first, then the first name in order.
        let mut best: Option<(u8, usize, String)> = None;
        let mut offer = |prio: u8, depth: usize, name: String| {
            let rank = |p: u8, d: usize| (p >= 5, p >= 4, if p >= 5 { 0 } else { usize::MAX - d }, p);
            let better = match &best {
                None => true,
                Some((p, d, n)) => (rank(prio, depth), std::cmp::Reverse(&name)) > (rank(*p, *d), std::cmp::Reverse(n)),
            };
            if better {
                best = Some((prio, depth, name));
            }
        };
        let depth_of = |u: usize| uses.get(u).map_or(0, |su| su.name_path.matches('/').count());
        let path_of = |u: usize| uses.get(u).map_or_else(|| "/".to_string(), |su| su.name_path.clone());
        for (u, m) in bus_named.get(&root).into_iter().flatten() {
            offer(4, depth_of(*u), format!("{}{m}", path_of(*u)));
        }
        for &it in &members {
            match items.get(it) {
                Some(Item::Pin { u, sym, number, name, kind, at, power, hidden }) => {
                    net.power_out |= *kind == PinType::PowerOut;
                    if *power {
                        net.power_symbols += 1;
                        if *kind == PinType::PowerIn {
                            offer(6, 0, escape(&power_name(design, *u, *sym, name)));
                        }
                        continue;
                    }
                    if *kind == PinType::PowerIn && *hidden && !name.is_empty() {
                        offer(5, 0, escape(name));
                    }
                    let Some(su) = uses.get(*u) else { continue };
                    let Some(symbol) = design.sheets.get(su.sheet).and_then(|(_, s)| s.symbols.get(*sym)) else {
                        continue;
                    };
                    let r = symbol.reference_in(&su.path);
                    if r.starts_with('#') || r.is_empty() {
                        continue;
                    }
                    net.nodes.push(Node {
                        reference: r.to_string(),
                        pin: number.clone(),
                        pin_name: name.clone(),
                        kind: *kind,
                        sheet_use: *u,
                        symbol: *sym,
                        at: *at,
                    });
                }
                Some(Item::Label { u, kind, text, at }) => {
                    net.labels += 1;
                    if net.marks.len() < 16 {
                        net.marks.push((*u, *at, text.clone()));
                    }
                    match kind {
                        LabelKind::Global => offer(5, 0, escape(text)),
                        LabelKind::Local => offer(4, depth_of(*u), format!("{}{}", path_of(*u), escape(text))),
                        LabelKind::Hierarchical => {
                            offer(3, depth_of(*u), format!("{}{}", path_of(*u), escape(text)));
                        }
                    }
                }
                // A sheet pin names nothing: the hierarchical label inside does.
                Some(Item::SheetPin { .. }) => net.labels += 1,
                Some(Item::NoConnect { u, at }) => {
                    net.no_connects += 1;
                    if net.marks.len() < 16 {
                        net.marks.push((*u, *at, String::new()));
                    }
                }
                _ => {}
            }
        }
        if net.nodes.is_empty() && best.is_none() {
            continue;
        }
        net.nodes.sort_by(|a, b| natural_cmp(&a.reference, &b.reference).then_with(|| natural_cmp(&a.pin, &b.pin)));
        net.nodes.dedup_by(|a, b| a.reference == b.reference && a.pin == b.pin);
        if let Some((_, _, name)) = best.as_ref().filter(|(p, ..)| *p >= 3) {
            net.name = name.clone();
            net.named = true;
        } else if let Some(first) = net
            .nodes
            .iter()
            .filter(|n| pin_named(n))
            .min_by(|a, b| {
                natural_cmp(&a.reference, &b.reference)
                    .then_with(|| unit_of(a).cmp(&unit_of(b)))
                    .then_with(|| natural_cmp(&a.pin, &b.pin))
            })
            .or_else(|| net.nodes.first())
        {
            let multi = multi_unit.get(&(first.sheet_use, first.symbol)).copied().unwrap_or(false);
            let unit = unit_of(first);
            let r = if multi { format!("{}{}", first.reference, unit_letter(unit)) } else { first.reference.clone() };
            let named_pin = pin_named(first);
            let pin_name = escape(&first.pin_name);
            net.name = if net.nodes.len() == 1 && net.labels == 0 {
                if named_pin {
                    format!("unconnected-({r}-{pin_name}-Pad{})", first.pin)
                } else {
                    format!("unconnected-({r}-Pad{})", first.pin)
                }
            } else if named_pin {
                format!("Net-({r}-{pin_name})")
            } else {
                format!("Net-({r}-Pad{})", first.pin)
            };
        } else if let Some((_, _, name)) = best {
            net.name = name;
            net.named = true;
        }
        net_of_root.insert(root, nl.nets.len());
        nl.nets.push(net);
    }
    // Same names from different groups (two unrelated unconnected nets cannot clash; two
    // sheet pins without a label inside could): later ones get a suffix.
    let mut seen: HashMap<String, usize> = HashMap::new();
    for n in &mut nl.nets {
        let c = seen.entry(n.name.clone()).or_insert(0);
        *c += 1;
        if *c > 1 {
            n.name = format!("{}_{}", n.name, c);
        }
    }
    for (u, &(base, count)) in wire_base.iter().enumerate() {
        let v = (0..count).map(|w| net_of_root.get(&dsu.find(base + w)).copied()).collect();
        if u < uses.len() {
            nl.wire_nets.push(v);
        }
    }
    nl
}

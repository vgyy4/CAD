//! Hierarchical sheet edits (ADR 0057): a sheet on a schematic, its pins, and the empty
//! sub-sheet file it points to. Like the other edits they change the S-expression tree and
//! re-read the typed view, so everything else is written back as it was read.

use crate::board::{Limits, Pt};
use crate::sch::{Error, Schematic};
use crate::sch_edit::{at_node, bad, check_name, check_pt, effects, kv, list, yes};
use crate::sexpr::Node;

/// Smallest sheet side (mm).
pub const MIN_SIDE: f64 = 5.08;
/// Largest sheet side (mm).
pub const MAX_SIDE: f64 = 2_000.0;
/// KiCad's grid step along a sheet edge for new pins (mm).
const PIN_STEP: f64 = 2.54;

/// The kinds of a sheet pin (KiCad's pin shapes).
pub const PIN_KINDS: [&str; 5] = ["input", "output", "bidirectional", "tri_state", "passive"];

/// An edge of a sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    /// `left`, `right`, `top` or `bottom`.
    pub fn parse(s: &str) -> Option<Side> {
        Some(match s {
            "left" => Side::Left,
            "right" => Side::Right,
            "top" => Side::Top,
            "bottom" => Side::Bottom,
            _ => return None,
        })
    }
}

fn pin_node(s: &Schematic, name: &str, kind: &str, at: Pt, angle: f64, justify: &str) -> Node {
    let mut items = vec![Node::sym("pin"), Node::str(name), Node::sym(kind), at_node(at, Some(angle))];
    items.push(s.new_uuid(&items));
    items.push(effects(Some(justify), false));
    list(items)
}

impl Schematic {
    /// Places a hierarchical sheet at `at` of `size` (mm), named `name`, pointing at the
    /// sheet file `file`. `parent_path` is the instance path of this sheet's own use
    /// (`/root-uuid` on the root sheet) and `page` the sheet's page number. Returns the
    /// index in [`Schematic::sheets`]. The sub-sheet file itself is the caller's to make
    /// (see [`Schematic::new_empty`]).
    ///
    /// # Errors
    /// A bad name, file, place or size, or a failed re-read.
    #[allow(clippy::too_many_arguments)]
    pub fn add_sheet(
        &mut self,
        name: &str,
        file: &str,
        at: Pt,
        size: Pt,
        project: &str,
        parent_path: &str,
        page: &str,
        limits: &Limits,
    ) -> Result<usize, Error> {
        check_pt(at)?;
        check_name("sheet name", name)?;
        check_name("sheet file", file)?;
        if !size.iter().all(|v| v.is_finite() && (MIN_SIDE..=MAX_SIDE).contains(v)) {
            return Err(bad(format!("a sheet is {MIN_SIDE} to {MAX_SIDE} mm on each side")));
        }
        check_pt([at[0] + size[0], at[1] + size[1]])?;
        let mut items = vec![
            Node::sym("sheet"),
            at_node(at, None),
            list(vec![Node::sym("size"), Node::num(size[0]), Node::num(size[1])]),
            yes("exclude_from_sim", false),
            yes("in_bom", true),
            yes("on_board", true),
            yes("dnp", false),
            yes("fields_autoplaced", true),
            list(vec![Node::sym("stroke"), kv("width", Node::num(0.1524)), kv("type", Node::sym("solid"))]),
            kv("fill", list(vec![Node::sym("color"), Node::num(0.0), Node::num(0.0), Node::num(0.0), Node::num(0.0)])),
        ];
        let uuid = self.new_uuid(&items);
        let sheet_uuid = uuid.arg(0).unwrap_or("").to_string();
        items.push(uuid);
        let field = |key: &str, value: &str, p: Pt, justify: &str| {
            list(vec![
                Node::sym("property"),
                Node::str(key),
                Node::str(value),
                at_node(p, Some(0.0)),
                effects(Some(justify), false),
            ])
        };
        items.push(field("Sheetname", name, [at[0], at[1] - 0.7116], "left bottom"));
        items.push(field("Sheetfile", file, [at[0], at[1] + size[1] + 0.5754], "left top"));
        items.push(list(vec![
            Node::sym("instances"),
            list(vec![
                Node::sym("project"),
                Node::str(project),
                list(vec![Node::sym("path"), Node::str(parent_path), kv("page", Node::str(page))]),
            ]),
        ]));
        let n = self.sheets.len();
        self.insert(list(items))?;
        self.reread(limits)?;
        if self.sheets.len() != n + 1 {
            return Err(bad("the new sheet did not read back"));
        }
        self.sheets
            .iter()
            .position(|s| s.uuid.as_deref() == Some(sheet_uuid.as_str()))
            .ok_or_else(|| bad("the new sheet did not read back"))
    }

    /// Adds a pin named `name` of `kind` on one edge of a sheet. `offset` is the distance
    /// along the edge (from the top for left and right, from the left for top and bottom);
    /// without it the pin takes the first free grid step. Returns the pin's index.
    ///
    /// # Errors
    /// No such sheet, a bad name, kind or offset, a name the sheet already has, or a
    /// failed re-read.
    pub fn add_sheet_pin(
        &mut self,
        sheet: usize,
        name: &str,
        kind: &str,
        side: Side,
        offset: Option<f64>,
        limits: &Limits,
    ) -> Result<usize, Error> {
        check_name("sheet pin name", name)?;
        if !PIN_KINDS.contains(&kind) {
            return Err(bad("the pin kind is input, output, bidirectional, tri_state or passive"));
        }
        let sh = self.sheets.get(sheet).ok_or_else(|| bad("no such sheet"))?;
        if sh.pins.iter().any(|p| p.name == name) {
            return Err(bad(format!("the sheet already has a pin named '{name}'")));
        }
        let (at, size) = (sh.at, sh.size);
        let (along, length) = match side {
            Side::Left | Side::Right => (1, size[1]),
            Side::Top | Side::Bottom => (0, size[0]),
        };
        let (edge, angle, justify) = match side {
            Side::Left => (at[0], 180.0, "right"),
            Side::Right => (at[0] + size[0], 0.0, "left"),
            Side::Top => (at[1], 90.0, "left"),
            Side::Bottom => (at[1] + size[1], 270.0, "right"),
        };
        let on_edge = |p: Pt| (if along == 1 { p[0] } else { p[1] } - edge).abs() < 1e-6;
        let coord = |p: Pt| if along == 1 { p[1] } else { p[0] };
        let used: Vec<f64> = sh.pins.iter().filter(|p| on_edge(p.at)).map(|p| coord(p.at) - coord(at)).collect();
        let off = match offset {
            Some(o) if o.is_finite() && o > 0.0 && o < length => o,
            Some(_) => return Err(bad("the pin offset lies on the sheet's edge, past its corners")),
            None => (1..)
                .map(|k| f64::from(k) * PIN_STEP)
                .take_while(|o| *o < length)
                .find(|o| used.iter().all(|u| (u - o).abs() > PIN_STEP / 2.0))
                .ok_or_else(|| bad("the sheet's edge has no free place for another pin"))?,
        };
        let mut p = [0.0, 0.0];
        if along == 1 {
            p = [edge, at[1] + off];
        } else {
            p[0] = at[0] + off;
            p[1] = edge;
        }
        let node = sh.node;
        let pin = pin_node(self, name, kind, p, angle, justify);
        let items = self.top_item_mut(node)?;
        let at = items.iter().position(|n| n.head() == Some("instances")).unwrap_or(items.len());
        items.insert(at, pin);
        self.reread(limits)?;
        self.sheets
            .get(sheet)
            .and_then(|s| s.pins.iter().position(|q| q.name == name))
            .ok_or_else(|| bad("the new pin did not read back"))
    }

    /// Renames a sheet pin (the hierarchical label of that name inside the sheet is not
    /// touched: the netlist and ERC show the mismatch).
    ///
    /// # Errors
    /// No such sheet or pin, a bad or taken new name, or a failed re-read.
    pub fn rename_sheet_pin(&mut self, sheet: usize, from: &str, to: &str, limits: &Limits) -> Result<(), Error> {
        check_name("sheet pin name", to)?;
        let sh = self.sheets.get(sheet).ok_or_else(|| bad("no such sheet"))?;
        if !sh.pins.iter().any(|p| p.name == from) {
            return Err(bad(format!("the sheet has no pin named '{from}'")));
        }
        if from != to && sh.pins.iter().any(|p| p.name == to) {
            return Err(bad(format!("the sheet already has a pin named '{to}'")));
        }
        let node = sh.node;
        let items = self.top_item_mut(node)?;
        for pin in items.iter_mut().filter(|n| n.head() == Some("pin") && n.arg(0) == Some(from)) {
            if let Some(slot) = pin.items_mut().and_then(|v| v.get_mut(1)) {
                *slot = Node::str(to);
            }
        }
        self.reread(limits)
    }

    /// Removes a sheet pin (wires on it stay).
    ///
    /// # Errors
    /// No such sheet or pin, or a failed re-read.
    pub fn remove_sheet_pin(&mut self, sheet: usize, name: &str, limits: &Limits) -> Result<(), Error> {
        let sh = self.sheets.get(sheet).ok_or_else(|| bad("no such sheet"))?;
        if !sh.pins.iter().any(|p| p.name == name) {
            return Err(bad(format!("the sheet has no pin named '{name}'")));
        }
        let node = sh.node;
        let items = self.top_item_mut(node)?;
        items.retain(|n| !(n.head() == Some("pin") && n.arg(0) == Some(name)));
        self.reread(limits)
    }
}

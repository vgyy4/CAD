//! Design rules: net classes (widths and clearances) and the board's minimums.
//!
//! KiCad 5 wrote net classes into the board (`(net_class Default "" (clearance 0.2)
//! (trace_width 0.25) (via_dia 0.8) (via_drill 0.4) (add_net "GND"))`); KiCad 6 and later
//! keep them and the design rules in the project file (`.kicad_pro`, JSON), which the
//! caller reads into a [`Rules`] (this crate has no JSON reader).

use crate::board::Board;
use crate::sexpr::Node;

/// A net class: the width new tracks get and the clearance its nets keep.
#[derive(Debug, Clone, PartialEq)]
pub struct NetClass {
    pub name: String,
    /// mm.
    pub clearance: Option<f64>,
    pub track_width: Option<f64>,
    pub via_diameter: Option<f64>,
    pub via_drill: Option<f64>,
    /// Nets named in the class itself (KiCad 5 `add_net`, KiCad 6 `nets`).
    pub nets: Vec<String>,
}

impl NetClass {
    /// A KiCad 5 `(net_class ...)` node.
    pub fn from_kicad5(n: &Node) -> NetClass {
        let v = |h: &str| n.child(h).and_then(|c| c.num_arg(0)).filter(|v| v.is_finite() && (0.0..100.0).contains(v));
        NetClass {
            name: n.arg(0).unwrap_or("Default").chars().take(200).collect(),
            clearance: v("clearance"),
            track_width: v("trace_width"),
            via_diameter: v("via_dia"),
            via_drill: v("via_drill"),
            nets: n.children("add_net").filter_map(|a| a.arg(0)).take(100_000).map(str::to_string).collect(),
        }
    }
}

/// The rules edits and the design rule check use. Lengths in mm.
#[derive(Debug, Clone, PartialEq)]
pub struct Rules {
    /// Copper to copper of other nets, when no class says more.
    pub clearance: f64,
    /// Narrowest track allowed.
    pub min_track_width: f64,
    /// Smallest copper ring around a plated hole.
    pub min_annular_ring: f64,
    /// Hole edge to board edge.
    pub hole_to_edge: f64,
    /// Copper to board edge.
    pub edge_clearance: f64,
    /// Width of new tracks, via size and drill, when no class says otherwise.
    pub track_width: f64,
    pub via_diameter: f64,
    pub via_drill: f64,
    pub classes: Vec<NetClass>,
    /// Net → class, from the project's assignments.
    pub assignments: Vec<(String, String)>,
    /// (pattern with `*` and `?`, class), first match wins.
    pub patterns: Vec<(String, String)>,
}

impl Default for Rules {
    /// KiCad's defaults for a new board's `Default` class, with common fab minimums.
    fn default() -> Self {
        Rules {
            clearance: 0.2,
            min_track_width: 0.15,
            min_annular_ring: 0.1,
            hole_to_edge: 0.3,
            edge_clearance: 0.3,
            track_width: 0.25,
            via_diameter: 0.6,
            via_drill: 0.3,
            classes: Vec::new(),
            assignments: Vec::new(),
            patterns: Vec::new(),
        }
    }
}

impl Rules {
    /// The rules of a board: the defaults with the board's own (KiCad 5) net classes.
    pub fn for_board(b: &Board) -> Rules {
        let mut r = Rules { classes: b.net_classes.clone(), ..Rules::default() };
        r.apply_default_class();
        r
    }

    /// The `Default` class, if given, sets the board-wide values.
    pub fn apply_default_class(&mut self) {
        let Some(d) = self.classes.iter().find(|c| c.name == "Default").cloned() else { return };
        if let Some(v) = d.clearance {
            self.clearance = v;
        }
        if let Some(v) = d.track_width {
            self.track_width = v;
        }
        if let Some(v) = d.via_diameter {
            self.via_diameter = v;
        }
        if let Some(v) = d.via_drill {
            self.via_drill = v;
        }
    }

    /// The class of a net, if one is assigned.
    pub fn class_of(&self, net: &str) -> Option<&NetClass> {
        let by_name = |n: &str| self.classes.iter().find(|c| c.name == n);
        if let Some(c) = self.classes.iter().find(|c| c.name != "Default" && c.nets.iter().any(|x| x == net)) {
            return Some(c);
        }
        if let Some((_, c)) = self.assignments.iter().find(|(n, _)| n == net) {
            return by_name(c);
        }
        if let Some((_, c)) = self.patterns.iter().find(|(p, _)| glob(p, net)) {
            return by_name(c);
        }
        None
    }

    /// The clearance a net keeps (its class's, else the board's).
    pub fn clearance_of(&self, net: Option<&str>) -> f64 {
        net.and_then(|n| self.class_of(n)).and_then(|c| c.clearance).unwrap_or(self.clearance)
    }

    /// The width a new track of this net gets.
    pub fn track_width_of(&self, net: Option<&str>) -> f64 {
        net.and_then(|n| self.class_of(n)).and_then(|c| c.track_width).unwrap_or(self.track_width)
    }

    /// The size and drill of a new via of this net.
    pub fn via_of(&self, net: Option<&str>) -> (f64, f64) {
        let c = net.and_then(|n| self.class_of(n));
        (
            c.and_then(|c| c.via_diameter).unwrap_or(self.via_diameter),
            c.and_then(|c| c.via_drill).unwrap_or(self.via_drill),
        )
    }
}

/// `*` (any run) and `?` (one character) wildcards, iterative.
pub fn glob(pattern: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        match p.get(pi) {
            Some('*') => {
                star = Some((pi, ti));
                pi += 1;
            }
            Some(c) if *c == '?' || Some(c) == t.get(ti) => {
                pi += 1;
                ti += 1;
            }
            _ => match star {
                Some((sp, st)) => {
                    pi = sp + 1;
                    ti = st + 1;
                    star = Some((sp, st + 1));
                }
                None => return false,
            },
        }
    }
    p.get(pi..).is_some_and(|rest| rest.iter().all(|c| *c == '*'))
}

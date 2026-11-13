//! A subset of KiCad's custom rules file (`<board>.kicad_dru`, ADR 0054), from its documented
//! language: `(version 1)` then `(rule "name" (condition "...") (layer ..) (constraint ..))`.
//!
//! Supported: conditions over `A.NetClass`, `A.NetName`, `A.Type`, `A.Layer` (and the same
//! for `B`, in pair constraints) compared with `==` / `!=` against a quoted string or another
//! property, joined with `&&`, `||`, `!` and parentheses; constraints `clearance`,
//! `track_width`, `via_diameter`, `hole_size`, `hole_to_hole`, `edge_clearance` with `min`,
//! `max`, `opt`. Anything else makes the rule skipped, with a warning (never half-applied).
//! The reader is bounded (size, rule count, condition length, nesting) and never recurses
//! deeper than [`MAX_DEPTH`] (plus [`MAX_COMPARISONS`] for chained `&&`/`||`).

use crate::sexpr::{self, Node};

/// Largest file read (bytes).
pub const MAX_BYTES: usize = 1 << 20;
/// Most rules kept; later ones are dropped with a warning.
pub const MAX_RULES: usize = 2000;
/// Longest condition (characters).
pub const MAX_CONDITION: usize = 4096;
/// Deepest nesting of parentheses and `!` in a condition.
pub const MAX_DEPTH: usize = 24;
/// Most comparisons in one condition.
pub const MAX_COMPARISONS: usize = 64;
/// Most warnings kept.
const MAX_WARNINGS: usize = 100;

/// The rules of a file and what could not be used.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CustomRules {
    /// In file order: a later rule beats an earlier one.
    pub rules: Vec<Rule>,
    /// One line per rule or construct left out (the rule's name is quoted).
    pub warnings: Vec<String>,
}

/// One rule.
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub name: String,
    pub condition: Option<Expr>,
    pub layer: Option<LayerSel>,
    pub constraints: Vec<Constraint>,
}

/// A `(layer ..)` clause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayerSel {
    Name(String),
    Outer,
    Inner,
}

/// What a constraint limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Clearance,
    TrackWidth,
    ViaDiameter,
    /// Drill diameter of vias and pads.
    HoleSize,
    HoleToHole,
    EdgeClearance,
}

impl Kind {
    fn parse(s: &str) -> Option<Kind> {
        Some(match s {
            "clearance" => Kind::Clearance,
            "track_width" => Kind::TrackWidth,
            "via_diameter" => Kind::ViaDiameter,
            "hole_size" => Kind::HoleSize,
            "hole_to_hole" => Kind::HoleToHole,
            "edge_clearance" => Kind::EdgeClearance,
            _ => return None,
        })
    }
    /// Constraints between two items; the others look at one.
    fn pairwise(self) -> bool {
        matches!(self, Kind::Clearance | Kind::HoleToHole)
    }
}

/// A constraint with its limits (mm).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Constraint {
    pub kind: Kind,
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// Preferred value: read, not checked.
    pub opt: Option<f64>,
}

/// A condition.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Cmp { lhs: Operand, equal: bool, rhs: Operand },
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
}

/// One side of a comparison.
#[derive(Debug, Clone, PartialEq)]
pub enum Operand {
    Prop(Side, Prop),
    Str(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    A,
    B,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prop {
    NetClass,
    NetName,
    Type,
    Layer,
}

/// What a condition can ask about an item.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Subject {
    pub net_class: String,
    pub net_name: String,
    /// `track`, `via`, `pad` or `zone`.
    pub kind: &'static str,
    /// The copper layers it is on (the one being checked, when a check is per layer).
    pub layers: Vec<String>,
}

impl Expr {
    fn uses_b(&self) -> bool {
        match self {
            Expr::Cmp { lhs, rhs, .. } => [lhs, rhs].iter().any(|o| matches!(o, Operand::Prop(Side::B, _))),
            Expr::And(x, y) | Expr::Or(x, y) => x.uses_b() || y.uses_b(),
            Expr::Not(x) => x.uses_b(),
        }
    }

    fn eval(&self, a: &Subject, b: Option<&Subject>) -> bool {
        match self {
            Expr::And(x, y) => x.eval(a, b) && y.eval(a, b),
            Expr::Or(x, y) => x.eval(a, b) || y.eval(a, b),
            Expr::Not(x) => !x.eval(a, b),
            Expr::Cmp { lhs, equal, rhs } => {
                let (l, r) = (values(lhs, a, b), values(rhs, a, b));
                // A quoted string on the right may hold `*` and `?`.
                let wild = matches!(rhs, Operand::Str(_));
                let ci = matches!(lhs, Operand::Prop(_, Prop::Type)) || matches!(rhs, Operand::Prop(_, Prop::Type));
                let hit = l.iter().any(|x| {
                    r.iter().any(|y| {
                        if ci {
                            x.eq_ignore_ascii_case(y)
                        } else if wild {
                            crate::rules::glob(y, x)
                        } else {
                            x == y
                        }
                    })
                });
                hit == *equal
            }
        }
    }
}

fn values(o: &Operand, a: &Subject, b: Option<&Subject>) -> Vec<String> {
    match o {
        Operand::Str(s) => vec![s.clone()],
        Operand::Prop(side, p) => {
            let Some(s) = (if *side == Side::A { Some(a) } else { b }) else { return Vec::new() };
            match p {
                Prop::NetClass => vec![s.net_class.clone()],
                Prop::NetName => vec![s.net_name.clone()],
                Prop::Type => vec![s.kind.to_string()],
                Prop::Layer => s.layers.clone(),
            }
        }
    }
}

impl LayerSel {
    fn matches(&self, layers: &[String]) -> bool {
        layers.iter().any(|l| match self {
            LayerSel::Name(n) => l == n,
            LayerSel::Outer => l == "F.Cu" || l == "B.Cu",
            LayerSel::Inner => l.starts_with("In") && l.ends_with(".Cu"),
        })
    }
}

impl Rule {
    /// Whether the rule applies to the item (single constraints) or pair.
    fn matches(&self, a: &Subject, b: Option<&Subject>) -> bool {
        let layer_ok = |s: &Subject| self.layer.as_ref().is_none_or(|l| l.matches(&s.layers));
        let cond = |x: &Subject, y: Option<&Subject>| self.condition.as_ref().is_none_or(|c| c.eval(x, y));
        match b {
            None => layer_ok(a) && cond(a, None),
            // Either order; the layer clause holds for either of the two.
            Some(b) => (layer_ok(a) || layer_ok(b)) && (cond(a, Some(b)) || cond(b, Some(a))),
        }
    }
}

impl CustomRules {
    /// Reads a `.kicad_dru` text. Never fails: what cannot be read is left out and said in
    /// `warnings`.
    pub fn parse(text: &str) -> CustomRules {
        let mut out = CustomRules::default();
        if text.len() > MAX_BYTES {
            out.warn("the rules file is too large and was not read".into());
            return out;
        }
        // `#` starts a comment line; the file is a sequence of lists, read as one.
        let body: String =
            text.lines().map(|l| if l.trim_start().starts_with('#') { "" } else { l }).collect::<Vec<_>>().join("\n");
        let wrapped = format!("(dru {body}\n)");
        let lim = sexpr::Limits { max_bytes: MAX_BYTES + 64, max_depth: 12, max_nodes: 200_000 };
        let root = match sexpr::parse(&wrapped, &lim) {
            Ok(r) => r,
            Err(e) => {
                out.warn(format!("the rules file does not read ({e}); no custom rules were used"));
                return out;
            }
        };
        let version = root.child("version").and_then(|v| v.arg(0)).unwrap_or("");
        if version != "1" {
            out.warn("the rules file has no (version 1); no custom rules were used".into());
            return out;
        }
        for node in root.args() {
            match node.head() {
                Some("version") => {}
                Some("rule") => {
                    if out.rules.len() >= MAX_RULES {
                        out.warn(format!("more than {MAX_RULES} rules: the rest were left out"));
                        break;
                    }
                    let name: String =
                        node.arg(0).unwrap_or("").chars().filter(|c| !c.is_control()).take(100).collect();
                    match parse_rule(node, &name) {
                        Ok(r) => out.rules.push(r),
                        Err(why) => out.warn(format!("rule \"{name}\" skipped: {why}")),
                    }
                }
                Some(other) => out.warn(format!("`{}` is not supported and was left out", clip(other))),
                None => {}
            }
        }
        out
    }

    fn warn(&mut self, w: String) {
        if self.warnings.len() < MAX_WARNINGS {
            self.warnings.push(w);
        }
    }

    /// Whether any rule limits `kind`.
    pub fn has(&self, kind: Kind) -> bool {
        self.rules.iter().any(|r| r.constraints.iter().any(|c| c.kind == kind))
    }

    /// The constraint of `kind` that governs `a` (and `b`, for pair constraints): the last
    /// matching rule's. Subjects carry the layer being checked.
    pub fn find(&self, kind: Kind, a: &Subject, b: Option<&Subject>) -> Option<(&Rule, &Constraint)> {
        self.rules
            .iter()
            .rev()
            .filter(|r| r.matches(a, if kind.pairwise() { b } else { None }))
            .find_map(|r| r.constraints.iter().find(|c| c.kind == kind).map(|c| (r, c)))
    }

    /// The largest `min` of `kind` over all rules (a search radius).
    pub fn largest_min(&self, kind: Kind) -> f64 {
        self.rules
            .iter()
            .flat_map(|r| &r.constraints)
            .filter(|c| c.kind == kind)
            .filter_map(|c| c.min)
            .fold(0.0, f64::max)
    }
}

fn clip(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).take(40).collect()
}

fn parse_rule(node: &Node, name: &str) -> Result<Rule, String> {
    if name.is_empty() {
        return Err("it has no name".into());
    }
    let (mut condition, mut layer, mut constraints) = (None, None, Vec::new());
    for c in node.args().iter().skip(1) {
        match c.head() {
            Some("condition") => condition = Some(parse_condition(c.arg(0).unwrap_or(""))?),
            Some("layer") => {
                layer = Some(match c.arg(0) {
                    Some("outer") => LayerSel::Outer,
                    Some("inner") => LayerSel::Inner,
                    Some(n) if n.ends_with(".Cu") && n.len() <= 40 => LayerSel::Name(n.to_string()),
                    other => return Err(format!("layer `{}` is not supported", clip(other.unwrap_or("")))),
                });
            }
            Some("severity") if c.arg(0) == Some("error") => {}
            Some("severity") => return Err(format!("severity `{}` is not supported", clip(c.arg(0).unwrap_or("")))),
            Some("constraint") => constraints.push(parse_constraint(c)?),
            Some(other) => return Err(format!("`{}` is not supported", clip(other))),
            None => {}
        }
    }
    if constraints.is_empty() {
        return Err("it has no constraint".into());
    }
    if let Some(e) = &condition
        && e.uses_b()
        && let Some(c) = constraints.iter().find(|c| !c.kind.pairwise())
    {
        return Err(format!(
            "`B.` in the condition is only supported with clearance and hole_to_hole, not {:?}",
            c.kind
        ));
    }
    Ok(Rule { name: name.to_string(), condition, layer, constraints })
}

fn parse_constraint(c: &Node) -> Result<Constraint, String> {
    let kind_name = c.arg(0).unwrap_or("");
    let kind = Kind::parse(kind_name).ok_or_else(|| format!("constraint `{}` is not supported", clip(kind_name)))?;
    let (mut min, mut max, mut opt) = (None, None, None);
    for v in c.args().iter().skip(1) {
        let slot = match v.head() {
            Some("min") => &mut min,
            Some("max") => &mut max,
            Some("opt") => &mut opt,
            other => return Err(format!("`{}` in a constraint is not supported", clip(other.unwrap_or("")))),
        };
        *slot = Some(length(v.arg(0).unwrap_or(""))?);
    }
    if min.is_none() && max.is_none() && opt.is_none() {
        return Err(format!("constraint {kind_name} has no limit"));
    }
    if matches!(kind, Kind::Clearance | Kind::HoleToHole | Kind::EdgeClearance) && (max.is_some() || min.is_none()) {
        return Err(format!("{kind_name} is supported with min only"));
    }
    Ok(Constraint { kind, min, max, opt })
}

/// A length: `0.2`, `0.2mm`, `8mil`, `0.01in`.
fn length(t: &str) -> Result<f64, String> {
    let t = t.trim();
    let split = t.find(|c: char| c.is_ascii_alphabetic() && c != 'e' && c != 'E').unwrap_or(t.len());
    let (num, unit) = t.split_at(split);
    let v: f64 = num.parse().map_err(|_| format!("`{}` is not a length", clip(t)))?;
    let mm = match unit.trim() {
        "" | "mm" => v,
        "in" => v * 25.4,
        "mil" => v * 0.0254,
        u => return Err(format!("unit `{}` is not supported", clip(u))),
    };
    if mm.is_finite() && (0.0..=1000.0).contains(&mm) { Ok(mm) } else { Err(format!("`{}` is out of range", clip(t))) }
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    Str(String),
    Eq,
    Ne,
    And,
    Or,
    Not,
    Open,
    Close,
}

fn tokens(s: &str) -> Result<Vec<Tok>, String> {
    let c: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(&ch) = c.get(i) {
        let next = c.get(i + 1).copied();
        match ch {
            ' ' | '\t' | '\r' | '\n' => i += 1,
            '(' => {
                out.push(Tok::Open);
                i += 1;
            }
            ')' => {
                out.push(Tok::Close);
                i += 1;
            }
            '=' if next == Some('=') => {
                out.push(Tok::Eq);
                i += 2;
            }
            '!' if next == Some('=') => {
                out.push(Tok::Ne);
                i += 2;
            }
            '!' => {
                out.push(Tok::Not);
                i += 1;
            }
            '&' if next == Some('&') => {
                out.push(Tok::And);
                i += 2;
            }
            '|' if next == Some('|') => {
                out.push(Tok::Or);
                i += 2;
            }
            '\'' | '"' => {
                let end = c
                    .iter()
                    .skip(i + 1)
                    .position(|x| *x == ch)
                    .ok_or_else(|| "an unclosed string in the condition".to_string())?;
                out.push(Tok::Str(c.iter().skip(i + 1).take(end).collect()));
                i += end + 2;
            }
            x if x.is_ascii_alphabetic() || x == '_' => {
                let w: String =
                    c.iter().skip(i).take_while(|x| x.is_ascii_alphanumeric() || **x == '_' || **x == '.').collect();
                i += w.chars().count();
                out.push(Tok::Ident(w));
            }
            x => return Err(format!("`{}` in a condition is not supported", clip(&x.to_string()))),
        }
    }
    Ok(out)
}

struct Parser {
    toks: Vec<Tok>,
    at: usize,
    comparisons: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.at)
    }
    fn or(&mut self, depth: usize) -> Result<Expr, String> {
        let mut e = self.and(depth)?;
        while self.peek() == Some(&Tok::Or) {
            self.at += 1;
            e = Expr::Or(Box::new(e), Box::new(self.and(depth)?));
        }
        Ok(e)
    }
    fn and(&mut self, depth: usize) -> Result<Expr, String> {
        let mut e = self.unary(depth)?;
        while self.peek() == Some(&Tok::And) {
            self.at += 1;
            e = Expr::And(Box::new(e), Box::new(self.unary(depth)?));
        }
        Ok(e)
    }
    fn unary(&mut self, depth: usize) -> Result<Expr, String> {
        if depth > MAX_DEPTH {
            return Err("the condition nests too deeply".into());
        }
        match self.peek() {
            Some(Tok::Not) => {
                self.at += 1;
                Ok(Expr::Not(Box::new(self.unary(depth + 1)?)))
            }
            Some(Tok::Open) => {
                self.at += 1;
                let e = self.or(depth + 1)?;
                if self.peek() != Some(&Tok::Close) {
                    return Err("a missing `)` in the condition".into());
                }
                self.at += 1;
                Ok(e)
            }
            _ => {
                self.comparisons += 1;
                if self.comparisons > MAX_COMPARISONS {
                    return Err("the condition has too many comparisons".into());
                }
                let lhs = self.operand()?;
                let equal = match self.peek() {
                    Some(Tok::Eq) => true,
                    Some(Tok::Ne) => false,
                    _ => return Err("expected == or != in the condition".into()),
                };
                self.at += 1;
                let rhs = self.operand()?;
                Ok(Expr::Cmp { lhs, equal, rhs })
            }
        }
    }
    fn operand(&mut self) -> Result<Operand, String> {
        let t = self.peek().cloned();
        self.at += 1;
        match t {
            Some(Tok::Str(s)) => Ok(Operand::Str(s)),
            Some(Tok::Ident(w)) => {
                if self.peek() == Some(&Tok::Open) {
                    return Err(format!("the function `{}` is not supported", clip(&w)));
                }
                let (side, prop) = w.split_once('.').ok_or_else(|| format!("`{}` is not supported", clip(&w)))?;
                let side = match side.to_ascii_lowercase().as_str() {
                    "a" => Side::A,
                    "b" => Side::B,
                    _ => return Err(format!("`{}` is not supported", clip(&w))),
                };
                let prop = match prop.to_ascii_lowercase().as_str() {
                    "netclass" => Prop::NetClass,
                    "netname" => Prop::NetName,
                    "type" => Prop::Type,
                    "layer" => Prop::Layer,
                    _ => return Err(format!("the property `{}` is not supported", clip(prop))),
                };
                Ok(Operand::Prop(side, prop))
            }
            _ => Err("expected a property or a string in the condition".into()),
        }
    }
}

fn parse_condition(s: &str) -> Result<Expr, String> {
    if s.chars().count() > MAX_CONDITION {
        return Err("the condition is too long".into());
    }
    let mut p = Parser { toks: tokens(s)?, at: 0, comparisons: 0 };
    let e = p.or(0)?;
    if p.at < p.toks.len() {
        return Err("unexpected text after the condition".into());
    }
    check_layers(&e)?;
    Ok(e)
}

/// `A.Layer == 'x'` only with copper layer names (`F.Cu`, `In1.Cu`) or wildcards: other
/// names (`outer`, silkscreen layers) would never match, silently.
fn check_layers(e: &Expr) -> Result<(), String> {
    match e {
        Expr::And(x, y) | Expr::Or(x, y) => check_layers(x).and(check_layers(y)),
        Expr::Not(x) => check_layers(x),
        Expr::Cmp { lhs, rhs, .. } => {
            let layer = |o: &Operand| matches!(o, Operand::Prop(_, Prop::Layer));
            let other = if layer(lhs) {
                rhs
            } else if layer(rhs) {
                lhs
            } else {
                return Ok(());
            };
            match other {
                Operand::Str(s) if s.ends_with(".Cu") || s.contains('*') => Ok(()),
                Operand::Str(s) => Err(format!("layer `{}` is not supported (copper layer names only)", clip(s))),
                Operand::Prop(..) => Ok(()),
            }
        }
    }
}

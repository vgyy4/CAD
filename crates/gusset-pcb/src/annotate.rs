//! Annotation (ADR 0052): which symbols get which reference numbers. A pure plan over the
//! sheets of a design; the command applies it. Units of a multi-unit part share one
//! reference (`U1A`, `U1B`: the letter is the unit, shown, not stored).

use crate::sch::Schematic;
use std::collections::{BTreeSet, HashMap, HashSet};

/// How to number.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    /// Renumber every symbol (else numbered symbols keep their number).
    pub all: bool,
    /// Order by Y first, then X (default: X first, then Y).
    pub y_first: bool,
    /// Sheet n (1 for the first file) starts at n * `sheet_step` (0: one run of numbers).
    pub sheet_step: u32,
}

/// One symbol's new reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Renumber {
    /// Index of the sheet file in the design.
    pub sheet: usize,
    /// Index in the sheet's symbols.
    pub symbol: usize,
    pub old: String,
    /// The new reference (`U3`), without a unit letter.
    pub new: String,
    /// The unit letter of a multi-unit part (`A`), shown after the reference.
    pub unit_letter: Option<String>,
}

/// The prefix and number of a reference (`R12` is `("R", Some(12))`, `R?` is `("R", None)`).
pub fn split_reference(r: &str) -> (String, Option<u64>) {
    let p: String = r.chars().take_while(|c| !c.is_ascii_digit() && *c != '?').collect();
    let rest = r.get(p.len()..).unwrap_or("");
    (p, rest.parse().ok())
}

/// A unit's letter: 1 is `A`, 26 `Z`, then `AA`.
pub fn unit_letter(unit: u32) -> String {
    let mut n = unit.max(1);
    let mut out = Vec::new();
    while n > 0 {
        let r = ((n - 1) % 26) as u8;
        out.push(char::from(b'A' + r));
        n = (n - 1) / 26;
    }
    out.iter().rev().collect()
}

/// A group of units sharing one reference.
struct Group {
    sheet: usize,
    lib: String,
    /// The reference the members have now (the key units join by).
    old: String,
    prefix: String,
    /// The fixed number of a group that is kept.
    number: Option<u64>,
    units: BTreeSet<u32>,
    members: Vec<usize>,
}

/// The renumbering of the design's symbols, in the order they are numbered. Symbols that
/// already have the right reference are left out.
pub fn plan(sheets: &[&Schematic], opts: Options) -> Vec<Renumber> {
    let mut used: HashMap<String, HashSet<u64>> = HashMap::new();
    let mut kept: Vec<Group> = Vec::new();
    // (sheet, symbol, position) still to number.
    let mut todo: Vec<(usize, usize, [f64; 2])> = Vec::new();
    for (si, sch) in sheets.iter().enumerate() {
        for (k, sym) in sch.symbols.iter().enumerate() {
            let (prefix, n) = split_reference(sym.reference());
            match n {
                Some(n) if !opts.all => {
                    used.entry(prefix.clone()).or_default().insert(n);
                    let multi = sch.lib_symbol(sym).is_some_and(|l| l.unit_count() > 1);
                    if multi {
                        let old = sym.reference().to_string();
                        match kept.iter_mut().find(|g| g.sheet == si && g.lib == sym.lib_key() && g.old == old) {
                            Some(g) => {
                                g.units.insert(sym.unit);
                            }
                            None => kept.push(Group {
                                sheet: si,
                                lib: sym.lib_key().to_string(),
                                old,
                                prefix,
                                number: Some(n),
                                units: BTreeSet::from([sym.unit]),
                                members: Vec::new(),
                            }),
                        }
                    }
                }
                _ => todo.push((si, k, sym.at)),
            }
        }
    }
    let order = |a: &(usize, usize, [f64; 2]), b: &(usize, usize, [f64; 2])| {
        let (a1, a2, b1, b2) = if opts.y_first { (a.2[1], a.2[0], b.2[1], b.2[0]) } else { (a.2[0], a.2[1], b.2[0], b.2[1]) };
        a.0.cmp(&b.0).then(a1.total_cmp(&b1)).then(a2.total_cmp(&b2))
    };
    todo.sort_by(order);

    let mut groups: Vec<Group> = Vec::new();
    for (si, k, _) in &todo {
        let Some(sch) = sheets.get(*si) else { continue };
        let Some(sym) = sch.symbols.get(*k) else { continue };
        let multi = sch.lib_symbol(sym).is_some_and(|l| l.unit_count() > 1);
        let old = sym.reference().to_string();
        let (prefix, n) = split_reference(&old);
        if multi {
            // Another unit of a part numbered in this run with the same reference ...
            if let Some(g) = groups
                .iter_mut()
                .find(|g| g.sheet == *si && g.lib == sym.lib_key() && g.old == old && !g.units.contains(&sym.unit))
            {
                g.units.insert(sym.unit);
                g.members.push(*k);
                continue;
            }
            // ... or the free unit of a part that keeps its number.
            if n.is_none()
                && let Some(g) = kept
                    .iter_mut()
                    .filter(|g| g.sheet == *si && g.lib == sym.lib_key() && g.prefix == prefix && !g.units.contains(&sym.unit))
                    .min_by_key(|g| g.number)
            {
                g.units.insert(sym.unit);
                g.members.push(*k);
                continue;
            }
        }
        groups.push(Group {
            sheet: *si,
            lib: sym.lib_key().to_string(),
            old,
            prefix,
            number: None,
            units: BTreeSet::from([sym.unit]),
            members: vec![*k],
        });
    }

    let mut out = Vec::new();
    // The members that joined a kept group take its number.
    for g in &kept {
        let Some(sch) = sheets.get(g.sheet) else { continue };
        for &k in &g.members {
            let Some(sym) = sch.symbols.get(k) else { continue };
            let prefix = if g.prefix.is_empty() { "U".to_string() } else { g.prefix.clone() };
            let new = format!("{prefix}{}", g.number.unwrap_or(1));
            if new != sym.reference() {
                out.push(Renumber {
                    sheet: g.sheet,
                    symbol: k,
                    old: sym.reference().to_string(),
                    new,
                    unit_letter: Some(unit_letter(sym.unit)),
                });
            }
        }
    }
    for g in &groups {
        let Some(sch) = sheets.get(g.sheet) else { continue };
        let prefix = if g.prefix.is_empty() { "U".to_string() } else { g.prefix.clone() };
        let start = if opts.sheet_step > 0 { u64::from(opts.sheet_step) * (u64::try_from(g.sheet).unwrap_or(0) + 1) } else { 1 };
        let taken = used.entry(prefix.clone()).or_default();
        let n = (start..).find(|n| !taken.contains(n)).unwrap_or(start);
        taken.insert(n);
        let new = format!("{prefix}{n}");
        for &k in &g.members {
            let Some(sym) = sch.symbols.get(k) else { continue };
            let multi = sch.lib_symbol(sym).is_some_and(|l| l.unit_count() > 1);
            if new != sym.reference() {
                out.push(Renumber {
                    sheet: g.sheet,
                    symbol: k,
                    old: sym.reference().to_string(),
                    new: new.clone(),
                    unit_letter: multi.then(|| unit_letter(sym.unit)),
                });
            }
        }
    }
    out
}

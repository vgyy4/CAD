//! Parameter/feature diff between two document states (the "compare" view).

use gusset_doc::Document;
use gusset_doc::text::{configuration_line, param_line, print_feature_object};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Added,
    Removed,
    /// Definition changed (old text, new text).
    Modified {
        old: String,
        new: String,
    },
    /// Same definition, different timeline position.
    Moved {
        old_index: usize,
        new_index: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemChange {
    pub name: String,
    pub change: Change,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DocDiff {
    pub units_changed: bool,
    /// Parameters, with their ranges (review v0.3 M5: a merge takes range changes).
    pub params: Vec<ItemChange>,
    pub configurations: Vec<ItemChange>,
    pub features: Vec<ItemChange>,
    /// Drawings (ADR 0032), whole: `old`/`new` are their texts.
    pub drawings: Vec<ItemChange>,
    /// Body appearances (ADR 0045), by body.
    pub appearances: Vec<ItemChange>,
    /// The open board and schematic (ADR 0047): at most one entry each, named by file.
    pub pcb: Vec<ItemChange>,
}

impl DocDiff {
    pub fn is_empty(&self) -> bool {
        !self.units_changed
            && self.params.is_empty()
            && self.configurations.is_empty()
            && self.features.is_empty()
            && self.drawings.is_empty()
            && self.appearances.is_empty()
            && self.pcb.is_empty()
    }
}

/// Compares `a` (old) to `b` (new). Output order is deterministic: items in `b` order,
/// then removed items in `a` order.
pub fn diff_documents(a: &Document, b: &Document) -> DocDiff {
    // Lookups by name through maps, so a compare stays linear (security review v0.3 W2).
    let mut d = DocDiff { units_changed: a.units != b.units, ..DocDiff::default() };
    let (pa, pb): (HashMap<_, _>, HashMap<_, _>) = (
        a.params.iter().map(|p| (p.name.as_str(), p)).collect(),
        b.params.iter().map(|p| (p.name.as_str(), p)).collect(),
    );
    for p in &b.params {
        match pa.get(p.name.as_str()) {
            None => d.params.push(ItemChange { name: p.name.clone(), change: Change::Added }),
            Some(old) if *old != p => d.params.push(ItemChange {
                name: p.name.clone(),
                change: Change::Modified { old: param_line(old), new: param_line(p) },
            }),
            Some(_) => {}
        }
    }
    for p in &a.params {
        if !pb.contains_key(p.name.as_str()) {
            d.params.push(ItemChange { name: p.name.clone(), change: Change::Removed });
        }
    }
    let (ca, cb): (HashMap<_, _>, HashMap<_, _>) = (
        a.configurations.iter().map(|c| (c.name.as_str(), c)).collect(),
        b.configurations.iter().map(|c| (c.name.as_str(), c)).collect(),
    );
    for c in &b.configurations {
        match ca.get(c.name.as_str()) {
            None => d.configurations.push(ItemChange { name: c.name.clone(), change: Change::Added }),
            Some(old) if *old != c => d.configurations.push(ItemChange {
                name: c.name.clone(),
                change: Change::Modified { old: configuration_line(old), new: configuration_line(c) },
            }),
            Some(_) => {}
        }
    }
    for c in &a.configurations {
        if !cb.contains_key(c.name.as_str()) {
            d.configurations.push(ItemChange { name: c.name.clone(), change: Change::Removed });
        }
    }
    let (aa, ab): (HashMap<_, _>, HashMap<_, _>) = (
        a.appearances.iter().map(|x| (x.body.as_str(), x)).collect(),
        b.appearances.iter().map(|x| (x.body.as_str(), x)).collect(),
    );
    for x in &b.appearances {
        match aa.get(x.body.as_str()).copied() {
            None => d.appearances.push(ItemChange { name: x.body.clone(), change: Change::Added }),
            Some(old) if old != x => d.appearances.push(ItemChange {
                name: x.body.clone(),
                change: Change::Modified {
                    old: gusset_doc::text::appearance_line(old),
                    new: gusset_doc::text::appearance_line(x),
                },
            }),
            Some(_) => {}
        }
    }
    for x in &a.appearances {
        if !ab.contains_key(x.body.as_str()) {
            d.appearances.push(ItemChange { name: x.body.clone(), change: Change::Removed });
        }
    }
    let board = |p: &gusset_doc::PcbBoard| format!("{} ({})", p.file, p.board.get(..12).unwrap_or(&p.board));
    match (&a.pcb, &b.pcb) {
        (None, Some(n)) => d.pcb.push(ItemChange { name: n.file.clone(), change: Change::Added }),
        (Some(o), None) => d.pcb.push(ItemChange { name: o.file.clone(), change: Change::Removed }),
        (Some(o), Some(n)) if o != n => {
            d.pcb.push(ItemChange { name: n.file.clone(), change: Change::Modified { old: board(o), new: board(n) } })
        }
        _ => {}
    }
    // The schematic (phase 2a) is listed with the board, named by its root sheet.
    let sheet = |p: &gusset_doc::PcbSchematic| format!("{} ({})", p.file, p.sheet.get(..12).unwrap_or(&p.sheet));
    match (&a.schematic, &b.schematic) {
        (None, Some(n)) => d.pcb.push(ItemChange { name: n.file.clone(), change: Change::Added }),
        (Some(o), None) => d.pcb.push(ItemChange { name: o.file.clone(), change: Change::Removed }),
        (Some(o), Some(n)) if o != n => {
            d.pcb.push(ItemChange { name: n.file.clone(), change: Change::Modified { old: sheet(o), new: sheet(n) } })
        }
        _ => {}
    }
    let (da, db): (HashMap<_, _>, HashMap<_, _>) = (
        a.drawings.iter().map(|x| (x.name.as_str(), x)).collect(),
        b.drawings.iter().map(|x| (x.name.as_str(), x)).collect(),
    );
    for x in &b.drawings {
        match da.get(x.name.as_str()) {
            None => d.drawings.push(ItemChange { name: x.name.clone(), change: Change::Added }),
            Some(old) if *old != x => d.drawings.push(ItemChange {
                name: x.name.clone(),
                change: Change::Modified {
                    old: gusset_doc::drawing::print_drawing_object(old),
                    new: gusset_doc::drawing::print_drawing_object(x),
                },
            }),
            Some(_) => {}
        }
    }
    for x in &a.drawings {
        if !db.contains_key(x.name.as_str()) {
            d.drawings.push(ItemChange { name: x.name.clone(), change: Change::Removed });
        }
    }
    // Positions among the features both sides share, so an insertion does not make
    // every later feature look "moved".
    let (fa, fb): (HashMap<_, _>, HashMap<_, _>) = (
        a.features.iter().map(|f| (f.name.as_str(), f)).collect(),
        b.features.iter().map(|f| (f.name.as_str(), f)).collect(),
    );
    let position = |d: &Document, other: &HashMap<&str, &gusset_doc::Feature>| -> HashMap<String, usize> {
        d.features
            .iter()
            .filter(|f| other.contains_key(f.name.as_str()))
            .enumerate()
            .map(|(i, f)| (f.name.clone(), i))
            .collect()
    };
    let (common_a, common_b) = (position(a, &fb), position(b, &fa));
    for f in &b.features {
        match fa.get(f.name.as_str()) {
            None => d.features.push(ItemChange { name: f.name.clone(), change: Change::Added }),
            Some(old) => {
                let oi = common_a.get(&f.name).copied().unwrap_or(0);
                let ni = common_b.get(&f.name).copied().unwrap_or(0);
                if *old != f {
                    d.features.push(ItemChange {
                        name: f.name.clone(),
                        change: Change::Modified { old: print_feature_object(old), new: print_feature_object(f) },
                    });
                } else if oi != ni {
                    d.features.push(ItemChange {
                        name: f.name.clone(),
                        change: Change::Moved { old_index: oi, new_index: ni },
                    });
                }
            }
        }
    }
    for f in &a.features {
        if !fb.contains_key(f.name.as_str()) {
            d.features.push(ItemChange { name: f.name.clone(), change: Change::Removed });
        }
    }
    d
}

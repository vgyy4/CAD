//! Parameter/feature diff between two document states (the "compare" view).

use gusset_doc::Document;
use gusset_doc::text::{configuration_line, param_line, print_feature_object};

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
}

impl DocDiff {
    pub fn is_empty(&self) -> bool {
        !self.units_changed && self.params.is_empty() && self.configurations.is_empty() && self.features.is_empty()
    }
}

/// Compares `a` (old) to `b` (new). Output order is deterministic: items in `b` order,
/// then removed items in `a` order.
pub fn diff_documents(a: &Document, b: &Document) -> DocDiff {
    let mut d = DocDiff { units_changed: a.units != b.units, ..DocDiff::default() };
    for p in &b.params {
        match a.params.iter().find(|x| x.name == p.name) {
            None => d.params.push(ItemChange { name: p.name.clone(), change: Change::Added }),
            Some(old) if old != p => d.params.push(ItemChange {
                name: p.name.clone(),
                change: Change::Modified { old: param_line(old), new: param_line(p) },
            }),
            Some(_) => {}
        }
    }
    for p in &a.params {
        if !b.params.iter().any(|x| x.name == p.name) {
            d.params.push(ItemChange { name: p.name.clone(), change: Change::Removed });
        }
    }
    for c in &b.configurations {
        match a.configurations.iter().find(|x| x.name == c.name) {
            None => d.configurations.push(ItemChange { name: c.name.clone(), change: Change::Added }),
            Some(old) if old != c => d.configurations.push(ItemChange {
                name: c.name.clone(),
                change: Change::Modified { old: configuration_line(old), new: configuration_line(c) },
            }),
            Some(_) => {}
        }
    }
    for c in &a.configurations {
        if !b.configurations.iter().any(|x| x.name == c.name) {
            d.configurations.push(ItemChange { name: c.name.clone(), change: Change::Removed });
        }
    }
    // Positions among the features both sides share, so an insertion does not make
    // every later feature look "moved".
    let common_a: Vec<&str> =
        a.features.iter().filter(|f| b.feature(&f.name).is_some()).map(|f| f.name.as_str()).collect();
    let common_b: Vec<&str> =
        b.features.iter().filter(|f| a.feature(&f.name).is_some()).map(|f| f.name.as_str()).collect();
    for f in &b.features {
        match a.feature(&f.name) {
            None => d.features.push(ItemChange { name: f.name.clone(), change: Change::Added }),
            Some(old) => {
                let oi = common_a.iter().position(|n| *n == f.name).unwrap_or(0);
                let ni = common_b.iter().position(|n| *n == f.name).unwrap_or(0);
                if old != f {
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
        if b.feature(&f.name).is_none() {
            d.features.push(ItemChange { name: f.name.clone(), change: Change::Removed });
        }
    }
    d
}

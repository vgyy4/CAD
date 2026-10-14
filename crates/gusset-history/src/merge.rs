//! Three-way merge of two document states (ADR 0006 "Full merge"): every parameter and
//! feature is merged by name against the common ancestor, so only items both sides
//! changed differently conflict.

use gusset_doc::text::print_feature_object;
use gusset_doc::{Document, Feature};
use gusset_units::params::Param;

/// Which side wins a conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Ours,
    Theirs,
}

/// An item both sides changed differently since the common ancestor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    /// `units`, `parameter`, `configuration`, `feature` or `order`.
    pub kind: &'static str,
    pub name: String,
    /// Each side's version as text (`None`: removed on that side).
    pub ours: Option<String>,
    pub theirs: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Merged {
    pub doc: Document,
    /// Conflicts found; resolved by `prefer` when given, else ours is kept for them.
    pub conflicts: Vec<Conflict>,
    /// Items whose result came from their side: `parameter p`, `feature F`, `order`.
    pub from_theirs: Vec<String>,
}

/// The three-way choice for one item: `Ok(result)` or `Err(())` for a conflict.
fn pick<T: PartialEq + Clone>(base: Option<&T>, ours: Option<&T>, theirs: Option<&T>) -> Result<Option<T>, ()> {
    if ours == theirs || theirs == base {
        Ok(ours.cloned())
    } else if ours == base {
        Ok(theirs.cloned())
    } else {
        Err(())
    }
}

/// Merges `theirs` into `ours`, both descendants of `base`.
pub fn merge_documents(base: &Document, ours: &Document, theirs: &Document, prefer: Option<Side>) -> Merged {
    let mut conflicts = Vec::new();
    let mut from_theirs = Vec::new();
    let mut resolve = |kind: &'static str, name: &str, o: Option<String>, t: Option<String>| {
        conflicts.push(Conflict { kind, name: name.to_string(), ours: o, theirs: t });
        prefer == Some(Side::Theirs)
    };

    let units = match pick(Some(&base.units), Some(&ours.units), Some(&theirs.units)) {
        Ok(u) => u.unwrap_or(ours.units),
        Err(()) => {
            if resolve("units", "units", Some(format!("{:?}", ours.units)), Some(format!("{:?}", theirs.units))) {
                theirs.units
            } else {
                ours.units
            }
        }
    };
    if units != ours.units {
        from_theirs.push("units".into());
    }

    // Parameters: ours' order, then those only theirs added.
    let pnames = names(ours.params.iter().map(|p| p.name.as_str()), theirs.params.iter().map(|p| p.name.as_str()));
    let param = |d: &Document, n: &str| d.params.iter().find(|p| p.name == n).cloned();
    let show_p = |p: &Option<Param>| p.as_ref().map(|p| p.expr.to_string());
    let mut params = Vec::new();
    for n in &pnames {
        let (b, o, t) = (param(base, n), param(ours, n), param(theirs, n));
        let r = match pick(b.as_ref(), o.as_ref(), t.as_ref()) {
            Ok(r) => r,
            Err(()) if resolve("parameter", n, show_p(&o), show_p(&t)) => t.clone(),
            Err(()) => o.clone(),
        };
        if r != o {
            from_theirs.push(format!("parameter {n}"));
        }
        params.extend(r);
    }

    // Configurations: like parameters.
    let cnames = names(
        ours.configurations.iter().map(|c| c.name.as_str()),
        theirs.configurations.iter().map(|c| c.name.as_str()),
    );
    let conf = |d: &Document, n: &str| d.configurations.iter().find(|c| c.name == n).cloned();
    let show_c = |c: &Option<gusset_doc::Configuration>| {
        c.as_ref().map(|c| c.set.iter().map(|(n, e)| format!("{n} = {e}")).collect::<Vec<_>>().join(", "))
    };
    let mut configurations = Vec::new();
    for n in &cnames {
        let (b, o, t) = (conf(base, n), conf(ours, n), conf(theirs, n));
        let r = match pick(b.as_ref(), o.as_ref(), t.as_ref()) {
            Ok(r) => r,
            Err(()) if resolve("configuration", n, show_c(&o), show_c(&t)) => t.clone(),
            Err(()) => o.clone(),
        };
        if r != o {
            from_theirs.push(format!("configuration {n}"));
        }
        configurations.extend(r);
    }

    // Features: merged one by one, placed in ours' order; those only theirs added go right
    // after the feature that precedes them on their side.
    let feature = |d: &Document, n: &str| d.feature(n).cloned();
    let show_f = |f: &Option<Feature>| f.as_ref().map(print_feature_object);
    let mut merged: Vec<(String, Option<Feature>)> = Vec::new();
    let fnames = names(ours.features.iter().map(|f| f.name.as_str()), theirs.features.iter().map(|f| f.name.as_str()));
    for n in &fnames {
        let (b, o, t) = (feature(base, n), feature(ours, n), feature(theirs, n));
        let r = match pick(b.as_ref(), o.as_ref(), t.as_ref()) {
            Ok(r) => r,
            Err(()) if resolve("feature", n, show_f(&o), show_f(&t)) => t.clone(),
            Err(()) => o.clone(),
        };
        if r != o {
            from_theirs.push(format!("feature {n}"));
        }
        merged.push((n.clone(), r));
    }
    let mut order: Vec<String> = ours.features.iter().map(|f| f.name.clone()).collect();
    for (i, f) in theirs.features.iter().enumerate() {
        if order.contains(&f.name) {
            continue;
        }
        let after = theirs
            .features
            .get(..i)
            .and_then(|before| before.iter().rev().find_map(|p| order.iter().position(|n| *n == p.name)));
        order.insert(after.map_or(0, |a| a + 1), f.name.clone());
    }
    // A reorder of shared features on one side is taken; on both sides it conflicts.
    let shared = |d: &Document| -> Vec<String> {
        d.features
            .iter()
            .filter(|f| {
                base.feature(&f.name).is_some() && ours.feature(&f.name).is_some() && theirs.feature(&f.name).is_some()
            })
            .map(|f| f.name.clone())
            .collect()
    };
    let (sb, so, st) = (shared(base), shared(ours), shared(theirs));
    let take_theirs_order = if so == st || st == sb {
        false
    } else if so == sb {
        true
    } else {
        resolve("order", "feature order", Some(so.join(", ")), Some(st.join(", ")))
    };
    if take_theirs_order {
        // The shared features' slots, refilled in their order.
        let mut next = st.iter();
        for slot in order.iter_mut() {
            if so.contains(slot)
                && let Some(n) = next.next()
            {
                slot.clone_from(n);
            }
        }
        from_theirs.push("order".into());
    }
    let features =
        order.iter().filter_map(|n| merged.iter().find(|(m, _)| m == n).and_then(|(_, f)| f.clone())).collect();
    Merged { doc: Document { units, params, configurations, features }, conflicts, from_theirs }
}

/// The names of `a`, then those of `b` not in `a`, each once.
fn names<'a>(a: impl Iterator<Item = &'a str>, b: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for n in a.chain(b) {
        if !out.iter().any(|x| x == n) {
            out.push(n.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(body: &str) -> Document {
        Document::from_text(&format!("gusset 1\nunits mm\n\n{body}")).unwrap_or_else(|e| panic!("{e}\n{body}"))
    }

    fn sk(name: &str) -> String {
        format!("sketch {name} (plane: XY, next_id: 1) {{\n}}\n")
    }

    fn text(d: &Document) -> String {
        d.to_text()
    }

    #[test]
    fn independent_changes_both_arrive() {
        let base = doc(&format!("param a = 1 mm\nparam b = 2 mm\n\n{}{}", sk("S1"), sk("S2")));
        let ours = doc(&format!("param a = 5 mm\nparam b = 2 mm\n\n{}{}{}", sk("S1"), sk("S2"), sk("Mine")));
        let theirs = doc(&format!("param a = 1 mm\nparam b = 9 mm\nparam c = 3 mm\n\n{}{}", sk("S1"), sk("Theirs")));
        let m = merge_documents(&base, &ours, &theirs, None);
        assert!(m.conflicts.is_empty(), "{:?}", m.conflicts);
        let t = text(&m.doc);
        assert!(t.contains("param a = 5 mm") && t.contains("param b = 9 mm") && t.contains("param c = 3 mm"), "{t}");
        // S2 removed by them, Mine added by us, Theirs added after S1 (its predecessor).
        let order: Vec<&str> = m.doc.features.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(order, ["S1", "Theirs", "Mine"]);
        assert!(
            m.from_theirs.contains(&"feature S2".to_string()) && m.from_theirs.contains(&"parameter b".to_string())
        );
    }

    #[test]
    fn the_same_item_changed_on_both_sides_conflicts() {
        let base = doc("param a = 1 mm\n");
        let ours = doc("param a = 2 mm\n");
        let theirs = doc("param a = 3 mm\n");
        let m = merge_documents(&base, &ours, &theirs, None);
        assert_eq!(m.conflicts.len(), 1);
        assert_eq!(m.conflicts[0].ours.as_deref(), Some("2 mm"));
        assert!(text(&m.doc).contains("param a = 2 mm"));
        let m = merge_documents(&base, &ours, &theirs, Some(Side::Theirs));
        assert!(text(&m.doc).contains("param a = 3 mm"));
        // The same change on both sides is no conflict.
        let m = merge_documents(&base, &ours, &ours, None);
        assert!(m.conflicts.is_empty());
        // Removed on one side, changed on the other: a conflict.
        let m = merge_documents(&base, &doc(""), &theirs, None);
        assert_eq!(m.conflicts.len(), 1);
        assert_eq!(m.conflicts[0].ours, None);
    }

    #[test]
    fn a_reorder_on_one_side_is_taken() {
        let base = doc(&format!("{}{}{}", sk("A"), sk("B"), sk("C")));
        let ours = doc(&format!("{}{}{}{}", sk("A"), sk("B"), sk("C"), sk("D")));
        let theirs = doc(&format!("{}{}{}", sk("C"), sk("A"), sk("B")));
        let m = merge_documents(&base, &ours, &theirs, None);
        assert!(m.conflicts.is_empty(), "{:?}", m.conflicts);
        let order: Vec<&str> = m.doc.features.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(order, ["C", "A", "B", "D"]);
        // Both reordered differently: a conflict.
        let ours = doc(&format!("{}{}{}", sk("B"), sk("A"), sk("C")));
        let m = merge_documents(&base, &ours, &theirs, None);
        assert_eq!(m.conflicts.iter().map(|c| c.kind).collect::<Vec<_>>(), ["order"]);
    }
}

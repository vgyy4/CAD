//! Three-way merge of two document states (ADR 0006 "Full merge"): every parameter and
//! feature is merged by name against the common ancestor, so only items both sides
//! changed differently conflict.

use gusset_doc::text::print_feature_object;
use gusset_doc::{Document, Feature};
use gusset_units::params::Param;
use std::collections::{HashMap, HashSet};

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
    // Lookups by name through maps: merging stays linear in the document size (security
    // review v0.3 W2).
    let (pb, po, pt) = (param_map(base), param_map(ours), param_map(theirs));
    let param = |m: &HashMap<&str, &Param>, n: &str| m.get(n).map(|p| (*p).clone());
    let show_p = |p: &Option<Param>| p.as_ref().map(|p| p.expr.to_string());
    let mut params = Vec::new();
    for n in &pnames {
        let (b, o, t) = (param(&pb, n), param(&po, n), param(&pt, n));
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
    let (cb, co, ct) = (conf_map(base), conf_map(ours), conf_map(theirs));
    let conf = |m: &HashMap<&str, &gusset_doc::Configuration>, n: &str| m.get(n).map(|c| (*c).clone());
    let show_c = |c: &Option<gusset_doc::Configuration>| {
        c.as_ref().map(|c| c.set.iter().map(|(n, e)| format!("{n} = {e}")).collect::<Vec<_>>().join(", "))
    };
    let mut configurations = Vec::new();
    for n in &cnames {
        let (b, o, t) = (conf(&cb, n), conf(&co, n), conf(&ct, n));
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
    let (fb, fo, ft) = (feature_map(base), feature_map(ours), feature_map(theirs));
    let feature = |m: &HashMap<&str, &Feature>, n: &str| m.get(n).map(|f| (*f).clone());
    let show_f = |f: &Option<Feature>| f.as_ref().map(print_feature_object);
    let mut merged: Vec<(String, Option<Feature>)> = Vec::new();
    let fnames = names(ours.features.iter().map(|f| f.name.as_str()), theirs.features.iter().map(|f| f.name.as_str()));
    for n in &fnames {
        let (b, o, t) = (feature(&fb, n), feature(&fo, n), feature(&ft, n));
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
    // A feature only theirs has goes right after the feature before it on their side (which
    // is placed by then): chains of followers hung on ours' order.
    let mut follower: HashMap<&str, &str> = HashMap::new();
    let mut head: Option<&str> = None;
    for (i, f) in theirs.features.iter().enumerate() {
        if fo.contains_key(f.name.as_str()) {
            continue;
        }
        match i.checked_sub(1).and_then(|j| theirs.features.get(j)) {
            Some(p) => {
                follower.insert(p.name.as_str(), f.name.as_str());
            }
            None => head = Some(f.name.as_str()),
        }
    }
    let mut order: Vec<String> = Vec::with_capacity(ours.features.len() + follower.len() + 1);
    let mut chain = |start: &str, order: &mut Vec<String>| {
        let mut at = Some(start);
        while let Some(n) = at {
            order.push(n.to_string());
            at = follower.remove(n);
        }
    };
    if let Some(h) = head {
        chain(h, &mut order);
    }
    for f in &ours.features {
        chain(&f.name, &mut order);
    }
    // A reorder of shared features on one side is taken; on both sides it conflicts.
    let shared = |d: &Document| -> Vec<String> {
        d.features
            .iter()
            .filter(|f| {
                let n = f.name.as_str();
                fb.contains_key(n) && fo.contains_key(n) && ft.contains_key(n)
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
        let so_set: HashSet<&String> = so.iter().collect();
        for slot in order.iter_mut() {
            if so_set.contains(slot)
                && let Some(n) = next.next()
            {
                slot.clone_from(n);
            }
        }
        from_theirs.push("order".into());
    }
    let mut merged: HashMap<String, Option<Feature>> = merged.into_iter().collect();
    let features = order.iter().filter_map(|n| merged.remove(n).flatten()).collect();

    // Drawings (ADR 0032): whole drawings, like configurations.
    let dmap = |d: &Document| -> HashMap<String, gusset_doc::drawing::Drawing> {
        d.drawings.iter().map(|x| (x.name.clone(), x.clone())).collect()
    };
    let dnames = names(ours.drawings.iter().map(|d| d.name.as_str()), theirs.drawings.iter().map(|d| d.name.as_str()));
    let (db, dw, dt) = (dmap(base), dmap(ours), dmap(theirs));
    let show_d = |d: &Option<gusset_doc::drawing::Drawing>| d.as_ref().map(gusset_doc::drawing::print_drawing_object);
    let mut drawings = Vec::new();
    for n in &dnames {
        let (b, o, t) = (db.get(n).cloned(), dw.get(n).cloned(), dt.get(n).cloned());
        let r = match pick(b.as_ref(), o.as_ref(), t.as_ref()) {
            Ok(r) => r,
            Err(()) if resolve("drawing", n, show_d(&o), show_d(&t)) => t.clone(),
            Err(()) => o.clone(),
        };
        if r != o {
            from_theirs.push(format!("drawing {n}"));
        }
        drawings.extend(r);
    }
    Merged { doc: Document { units, params, configurations, features, drawings }, conflicts, from_theirs }
}

fn param_map(d: &Document) -> HashMap<&str, &Param> {
    d.params.iter().map(|p| (p.name.as_str(), p)).collect()
}

fn conf_map(d: &Document) -> HashMap<&str, &gusset_doc::Configuration> {
    d.configurations.iter().map(|c| (c.name.as_str(), c)).collect()
}

fn feature_map(d: &Document) -> HashMap<&str, &Feature> {
    d.features.iter().map(|f| (f.name.as_str(), f)).collect()
}

/// The names of `a`, then those of `b` not in `a`, each once.
fn names<'a>(a: impl Iterator<Item = &'a str>, b: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for n in a.chain(b) {
        if seen.insert(n) {
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

#[cfg(test)]
mod scale_tests {
    use super::*;

    #[test]
    fn merging_and_comparing_large_documents_is_linear() {
        // Security review v0.3 W2: 40,000 features took a minute (quadratic lookups).
        let n = 20_000;
        let mut base = Document::from_text("gusset 1\nunits mm\n").unwrap_or_else(|e| panic!("{e}"));
        let probe = Document::from_text("gusset 1\nunits mm\n\nsketch S (plane: XY, next_id: 1) {\n}\n")
            .unwrap_or_else(|e| panic!("{e}"));
        let Some(sk) = probe.features.first().cloned() else { panic!("no sketch") };
        for i in 0..n {
            let mut f = sk.clone();
            f.name = format!("S{i}");
            base.features.push(f);
        }
        let (mut ours, mut theirs) = (base.clone(), base.clone());
        ours.features.remove(10);
        let mut extra = sk.clone();
        extra.name = "Theirs1".into();
        theirs.features.insert(500, extra);
        let start = std::time::Instant::now();
        let m = merge_documents(&base, &ours, &theirs, None);
        let d = crate::diff::diff_documents(&ours, &theirs);
        assert!(start.elapsed() < std::time::Duration::from_secs(5), "{:?}", start.elapsed());
        assert_eq!(m.doc.features.len(), n);
        assert_eq!(m.doc.features.get(499).map(|f| f.name.as_str()), Some("Theirs1"));
        assert!(m.conflicts.is_empty());
        assert_eq!(d.features.len(), 2, "{:?}", d.features);
    }
}

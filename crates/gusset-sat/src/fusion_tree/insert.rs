//! Component Insert: the inserted design's features stand in the insert's place.
//!
//! A root design that inserts another design stores that design's tool solids and pre-cut bodies
//! with its own, so the root's tree must carry the inserted design's tree, moved to where the
//! occurrence stands (round 7 notes, section 15 of `docs/formats/fusion-design-stream.md`).

use super::InsertResolver;
use super::json::{self, move_feature, placement_of, rename};
use serde_json::{Value, json};
use std::collections::HashMap;

/// Deepest chain of designs that insert designs.
const MAX_INSERT_DEPTH: usize = 4;

/// Most features one insert may bring.
const MAX_INSERT_FEATURES: usize = 50_000;

/// Feature kinds whose points and directions `move_feature` moves; a design with another kind
/// is not moved to a placement that is not the identity.
const MOVABLE: [&str; 9] =
    ["sketch", "extrude", "fillet", "chamfer", "hole", "mirror", "stored_body", "pattern_linear", "pattern_circular"];

/// Replaces each `component_insert` feature of `tree` by the features of the design it inserts.
/// An insert the resolver cannot resolve stays, and the tree's check names it.
pub(super) fn splice(tree: &mut Value, resolve: &mut InsertResolver, depth: usize) -> Result<(), String> {
    let Some(feats) = tree.get_mut("features").and_then(Value::as_array_mut) else { return Ok(()) };
    if !feats.iter().any(is_insert) {
        return Ok(());
    }
    if depth >= MAX_INSERT_DEPTH {
        return Err("designs insert designs more than four levels deep".into());
    }
    let old = std::mem::take(feats);
    let mut out: Vec<Value> = Vec::with_capacity(old.len());
    // Each merged insert and the names of the bodies it leaves.
    let mut merged: Vec<(String, Vec<String>)> = Vec::new();
    for f in old {
        if !is_insert(&f) {
            out.push(f);
            continue;
        }
        let keys: Vec<String> = f
            .get("insert_keys")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|k| k.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        let Some(files) = resolve(&keys) else {
            out.push(f);
            continue;
        };
        let name = f.get("name").and_then(Value::as_str).unwrap_or("Component Insert").to_string();
        let Some(pl) = f.get("placement").and_then(placement_of) else {
            return Err(format!("the placement of {name} is not readable"));
        };
        let inner = match super::built(&files, &name, resolve, depth + 1) {
            Ok(mut t) => {
                json::finish(&mut t);
                t.get_mut("features").map(Value::take).and_then(|v| v.as_array().cloned()).unwrap_or_default()
            }
            // A design without a timeline we can read: what its blobs store.
            Err(why) => stored_bodies(&files, &name, &why)?,
        };
        let brought = bring(inner, &name, &pl)?;
        merged.push((name.clone(), final_bodies(&brought)));
        out.extend(brought);
        if out.len() > MAX_INSERT_FEATURES {
            return Err("the inserted designs bring too many features".into());
        }
    }
    retarget(&mut out, &merged);
    if let Some(feats) = tree.get_mut("features").and_then(Value::as_array_mut) {
        *feats = out;
    }
    Ok(())
}

/// The bodies the features leave, by the name of the feature that made each (a join, cut or
/// intersect keeps its target's name, so those bodies stay under the same names).
pub(super) fn final_bodies(feats: &[Value]) -> Vec<String> {
    feats
        .iter()
        .filter(|f| {
            f.get("type").and_then(Value::as_str) == Some("stored_body")
                || f.get("creates_bodies").and_then(Value::as_u64).is_some_and(|n| n > 0)
        })
        .filter_map(|f| f.get("name").and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

/// A root feature that joins to or cuts the bodies "of" an insert names the insert feature, which
/// is no longer in the tree: it names the bodies the insert left instead (one name when it left
/// one; several make the target a list, which the tree's check refuses for these features).
pub(super) fn retarget(feats: &mut [Value], merged: &[(String, Vec<String>)]) {
    for f in feats {
        let Some(t) = f.get_mut("target") else { continue };
        let names: Vec<String> = match t {
            Value::String(s) => vec![s.clone()],
            Value::Array(a) => a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
            _ => continue,
        };
        if !names.iter().any(|n| merged.iter().any(|(m, _)| m == n)) {
            continue;
        }
        let mut now: Vec<String> = Vec::new();
        for n in names {
            match merged.iter().find(|(m, _)| *m == n) {
                Some((_, bodies)) => now.extend(bodies.iter().cloned()),
                None => now.push(n),
            }
        }
        now.dedup();
        *t = match now.as_slice() {
            [one] => one.as_str().into(),
            many => many.to_vec().into(),
        };
    }
}

fn is_insert(f: &Value) -> bool {
    f.get("type").and_then(Value::as_str) == Some("component_insert")
}

/// The inserted design's features in the root: moved by the insert's placement, named apart.
pub(super) fn bring(mut feats: Vec<Value>, insert: &str, pl: &crate::fusion::Placement) -> Result<Vec<Value>, String> {
    let identity = *pl == crate::fusion::Placement::IDENTITY;
    if !identity
        && let Some(bad) =
            feats.iter().find(|f| !MOVABLE.contains(&f.get("type").and_then(Value::as_str).unwrap_or("")))
    {
        let n = bad.get("name").and_then(Value::as_str).unwrap_or("?");
        return Err(format!("the inserted feature {n} cannot be moved to the placement of {insert}"));
    }
    let suffix = format!(" [{insert}]");
    let map: HashMap<String, String> = feats
        .iter()
        .filter_map(|f| f.get("name").and_then(Value::as_str))
        .map(|n| (n.to_string(), format!("{n}{suffix}")))
        .collect();
    for f in &mut feats {
        if let Some(o) = f.as_object_mut() {
            o.retain(|k, _| !k.starts_with('_') && k != "body");
        }
        rename(f, &map);
        if let Some(n) = f.get("name").and_then(Value::as_str).map(|n| format!("{n}{suffix}"))
            && let Some(o) = f.as_object_mut()
        {
            o.insert("name".into(), n.into());
        }
        if !identity {
            move_feature(f, pl);
        }
    }
    Ok(feats)
}

/// The bodies a design stores in its `.smb` blobs, as `stored_body` features (in the design's own
/// coordinates). `why` is what kept the design's own tree from being read.
fn stored_bodies(files: &[(String, Vec<u8>)], insert: &str, why: &str) -> Result<Vec<Value>, String> {
    let limits = crate::Limits::default();
    let mut out: Vec<Value> = Vec::new();
    for (n, b) in files {
        let Some(blob) = n.rsplit('/').next() else { continue };
        if !n.contains("Breps.BlobParts/") || !blob.ends_with(".smb") {
            continue;
        }
        let count = crate::body_count(b, &limits).map_err(|e| format!("{insert}: {blob}: {e}"))?;
        for index in 0..count {
            // Sheets (construction surfaces, decals) are not bodies of the model.
            if crate::to_step_nth(b, insert, &limits, None, index, count).is_err() {
                continue;
            }
            let k = out.len() + 1;
            let mut f = json!({"type": "stored_body", "name": format!("{insert} body {k}"), "brep": blob,
                "index": index, "count": count, "creates_bodies": 1});
            if k == 1
                && let Some(o) = f.as_object_mut()
            {
                o.insert("note".into(), format!("the inserted design's stored bodies ({why})").into());
            }
            out.push(f);
        }
    }
    if out.is_empty() {
        return Err(format!("the design {insert} inserts has no readable timeline and no stored bodies: {why}"));
    }
    Ok(out)
}

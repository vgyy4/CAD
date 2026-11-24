//! Solid bodies a feature makes and merges (round sw22): some parts keep several bodies, made
//! by bosses with "merge result" off and by patterns of bodies, and merged later by a boss
//! that touches them (the Ahm3d0x gears: a tooth made as a body of its own, repeated by a
//! circular pattern of bodies, then joined to the gear by the next boss).
//!
//! Every node of these files writes, after its common header block (`00 00 80 bf`, zeros,
//! `01`, zeros, `ff ff ff ff`, an 8-byte stamp, `02 00`, a 3-byte build word, `00`, `01 00`
//! and 12 zero bytes), two lists of body tags and the ids of the bodies it leaves: a `u32`
//! 0 or 1 and then that many `u16`-counted lists of `u16` tags, the bodies it makes; the same
//! for the bodies it takes in (merges or cuts); a zero `u16`; a `u32` count of `u32` body ids.
//! A boss that takes in no body makes a new one; one that takes in several merges them; a cut
//! that makes none removes its body whole. A pattern of bodies names the body it repeats by
//! its id (`moCompSolidBody_c`, see `patterns::Seeds::body_ids`).

use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

use super::{u16_at, u32_at};

/// Most tags or ids in one list.
const MAX_LIST: usize = 1024;
/// How far into a node its header block is looked for.
const HEADER_REACH: usize = 1200;

/// The body lists of one feature node.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct BodyLists {
    /// Tags of the bodies the feature leaves (makes or changes).
    pub made: Vec<u16>,
    /// Tags of the bodies it takes in.
    pub taken: Vec<u16>,
    /// Ids of the bodies it leaves.
    pub ids: Vec<u32>,
}

/// The body lists of the node spanning `[a, b)`, where its header block has the layout above.
pub(super) fn body_lists(d: &[u8], a: usize, b: usize) -> Option<BodyLists> {
    let end = b.min(d.len()).min(a.saturating_add(HEADER_REACH));
    let start = (a..end.saturating_sub(32)).find(|&i| header_at(d, i))?;
    let mut k = start + 32;
    let lists = |k: &mut usize| -> Option<Vec<u16>> {
        let n = u32_at(d, *k)?;
        *k += 4;
        match n {
            0 => Some(Vec::new()),
            1 => {
                let c = usize::from(u16_at(d, *k)?);
                if c > MAX_LIST {
                    return None;
                }
                let v = (0..c).map(|j| u16_at(d, *k + 2 + 2 * j)).collect::<Option<Vec<u16>>>()?;
                *k += 2 + 2 * c;
                Some(v)
            }
            _ => None,
        }
    };
    let made = lists(&mut k)?;
    let taken = lists(&mut k)?;
    if u16_at(d, k)? != 0 {
        return None;
    }
    let c = usize::try_from(u32_at(d, k + 2)?).ok().filter(|&c| c <= MAX_LIST)?;
    let ids = (0..c).map(|j| u32_at(d, k + 6 + 4 * j)).collect::<Option<Vec<u32>>>()?;
    if k + 6 + 4 * c > end.max(b.min(d.len())) {
        return None;
    }
    Some(BodyLists { made, taken, ids })
}

/// The header block's end at `i`: `ff ff ff ff`, a stamp, `02 00 X X X 00 01 00`, 12 zero
/// bytes, preceded within 40 bytes by `00 00 80 bf` and the `01` among zeros.
fn header_at(d: &[u8], i: usize) -> bool {
    d.get(i..i + 4) == Some(&[0xff; 4][..])
        && d.get(i + 12..i + 14) == Some(&[2, 0][..])
        && d.get(i + 17..i + 20) == Some(&[0, 1, 0][..])
        && d.get(i + 20..i + 32).is_some_and(|z| z.iter().all(|&x| x == 0))
        && (8..=40).any(|back| {
            i.checked_sub(back).is_some_and(|s| {
                d.get(s..s + 4) == Some(&[0, 0, 0x80, 0xbf][..])
                    && d.get(s + 4..i).is_some_and(|z| z.iter().filter(|&&x| x != 0).count() == 1 && z.contains(&1))
            })
        })
}

/// What a feature does to bodies, by its type in the tree.
fn kind(f: &Value) -> &str {
    let t = f.get("type").and_then(Value::as_str).unwrap_or_default();
    let op = f.get("operation").and_then(Value::as_str).unwrap_or_default();
    match t {
        "extrude" | "revolve" | "loft" | "sweep" if op == "cut" => "cut",
        "extrude" | "revolve" | "loft" | "sweep" => "boss",
        "hole" => "cut",
        "pattern_linear" | "pattern_circular" if f.get("body_source").is_some() => "body_pattern",
        "sketch" | "plane" | "axis" | "point" => "none",
        _ => "other",
    }
}

/// Rewrites `feats` for the bodies the nodes' lists say they make and merge. `lists` are by
/// feature name; `own_profiles` the profiles a new body's extrude uses (its own contours).
/// The tree is left as read unless every solid feature has lists and every tag resolves: a
/// boss that takes in no body after the first becomes a new body, one that takes in several
/// joins the first and a combine joins the others to it, a pattern of bodies repeats the body
/// its source id names as bodies of their own, and a body a cut removes whole is left out
/// with the features that made it.
pub(super) fn track(
    feats: Vec<Value>,
    lists: &HashMap<String, BodyLists>,
    own_profiles: &HashMap<String, Value>,
) -> Vec<Value> {
    let needed = feats.iter().any(|f| f.get("body_source").is_some())
        || feats.iter().filter(|f| kind(f) == "boss").skip(1).any(|f| {
            f.get("name").and_then(Value::as_str).and_then(|n| lists.get(n)).is_some_and(|l| l.taken.is_empty())
        });
    if !needed {
        return feats;
    }
    retarget(&feats, lists, own_profiles).unwrap_or(feats)
}

fn retarget(
    feats: &[Value],
    lists: &HashMap<String, BodyLists>,
    own_profiles: &HashMap<String, Value>,
) -> Option<Vec<Value>> {
    let mut by_tag: HashMap<u16, String> = HashMap::new();
    let mut by_id: HashMap<u32, String> = HashMap::new();
    // The features behind each body, and bodies removed whole.
    let mut makers: HashMap<String, Vec<String>> = HashMap::new();
    let mut gone: HashSet<String> = HashSet::new();
    let mut out: Vec<Value> = Vec::with_capacity(feats.len() + 4);
    let mut any_body = false;
    for f in feats {
        let k = kind(f);
        if k == "none" {
            out.push(f.clone());
            continue;
        }
        let name = f.get("name").and_then(Value::as_str)?.to_string();
        let l = lists.get(&name)?;
        let mut taken: Vec<String> = Vec::new();
        for t in &l.taken {
            let b = by_tag.get(t)?;
            if !taken.contains(b) {
                taken.push(b.clone());
            }
        }
        // The body with the longest history keeps its name (the gear's Boss-Extrude13 merges
        // three tooth copies into the gear, not the gear into a copy).
        let size = |b: &String| makers.get(b).map_or(0, Vec::len);
        if let Some(k) = (0..taken.len()).max_by_key(|&k| (taken.get(k).map_or(0, size), usize::MAX - k))
            && k > 0
        {
            let b = taken.remove(k);
            taken.insert(0, b);
        }
        let mut js = f.as_object()?.clone();
        let body: String = match k {
            "boss" if taken.is_empty() => {
                if any_body {
                    js.insert("operation".into(), json!("new"));
                    js.remove("target");
                    if js.get("profiles").and_then(Value::as_str) == Some("all")
                        && let Some(p) = own_profiles.get(&name)
                    {
                        js.insert("profiles".into(), p.clone());
                    }
                }
                name.clone()
            }
            "body_pattern" => {
                if !taken.is_empty() {
                    return None;
                }
                let src = f.get("body_source").and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok())?;
                let source = by_id.get(&src)?.clone();
                let count = f.get("count").and_then(Value::as_u64)?;
                if l.made.len() as u64 + 1 != count {
                    return None;
                }
                for key in ["body_source", "unresolved", "why"] {
                    js.remove(key);
                }
                js.insert("features".into(), json!([]));
                js.insert("target".into(), json!(source));
                js.insert("merge".into(), json!(false));
                for (j, t) in l.made.iter().enumerate() {
                    let copy = format!("{name}[{}]", j + 1);
                    by_tag.insert(*t, copy.clone());
                    makers.insert(copy, vec![name.clone()]);
                }
                out.push(Value::Object(js));
                continue;
            }
            _ if taken.is_empty() => return None,
            "cut" if l.made.is_empty() => {
                // The cut takes its body away whole: neither is kept (the gears' pin,
                // Boss-Extrude1, which Cut-Extrude3 removes).
                let [b] = taken.as_slice() else { return None };
                gone.insert(b.clone());
                continue;
            }
            _ => {
                let first = taken.first()?.clone();
                if js.contains_key("target") || k == "boss" {
                    js.insert("target".into(), json!(first));
                }
                first
            }
        };
        any_body = true;
        let merged: Vec<String> = taken.iter().skip(1).cloned().collect();
        out.push(Value::Object(js));
        if !merged.is_empty() {
            if k != "boss" {
                return None;
            }
            let cname = format!("{name} bodies");
            out.push(json!({"type": "combine", "name": cname, "target": body, "tools": merged, "operation": "join"}));
            for m in &merged {
                let from = makers.remove(m).unwrap_or_default();
                makers.entry(body.clone()).or_default().extend(from);
            }
        }
        makers.entry(body.clone()).or_default().push(name.clone());
        for t in &l.made {
            by_tag.insert(*t, body.clone());
        }
        for i in &l.ids {
            by_id.insert(*i, body.clone());
        }
    }
    if gone.is_empty() {
        return Some(out);
    }
    // A body removed whole: drop the features that made it, if it took in nothing else
    // and nothing else refers to it.
    let mut drop: HashSet<String> = HashSet::new();
    for g in &gone {
        let m = makers.get(g)?;
        drop.extend(m.iter().cloned());
    }
    let refers = |f: &Value| {
        let t = f.get("target").and_then(Value::as_str);
        let tools = f.get("tools").and_then(Value::as_array);
        t.is_some_and(|t| gone.contains(t))
            || tools.is_some_and(|ts| ts.iter().any(|x| x.as_str().is_some_and(|x| gone.contains(x))))
    };
    let mut kept = Vec::with_capacity(out.len());
    for f in out {
        let name = f.get("name").and_then(Value::as_str).unwrap_or_default();
        if drop.contains(name) {
            continue;
        }
        if refers(&f) {
            return None;
        }
        kept.push(f);
    }
    Some(kept)
}

/// The tree's features as a map by name, for tests.
#[cfg(test)]
fn names(v: &[Value]) -> Vec<String> {
    v.iter().filter_map(|f| f.get("name").and_then(Value::as_str).map(str::to_string)).collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;

    fn header(made: &[u16], taken: &[u16], ids: &[u32]) -> Vec<u8> {
        let mut d = vec![0u8; 8];
        d.extend([0, 0, 0x80, 0xbf]);
        d.extend([0u8; 17]);
        d.push(1);
        d.extend([0u8; 9]);
        d.extend([0xff; 4]);
        d.extend([0x6f, 0x28, 0xdc, 1, 0x95, 0x4d, 0xa5, 6]);
        d.extend([2, 0, 0xab, 0x7e, 5, 0, 1, 0]);
        d.extend([0u8; 12]);
        for l in [made, taken] {
            if l.is_empty() {
                d.extend(0u32.to_le_bytes());
            } else {
                d.extend(1u32.to_le_bytes());
                d.extend(u16::try_from(l.len()).unwrap().to_le_bytes());
                for t in l {
                    d.extend(t.to_le_bytes());
                }
            }
        }
        d.extend([0, 0]);
        d.extend(u32::try_from(ids.len()).unwrap().to_le_bytes());
        for i in ids {
            d.extend(i.to_le_bytes());
        }
        d.extend(3u32.to_le_bytes());
        d.extend([0u8; 16]);
        d
    }

    #[test]
    fn reads_the_lists() {
        // The gear's Boss-Extrude11: makes 0x58, takes in 0x59, leaves body 0x93.
        let d = header(&[0x58], &[0x59], &[0x93]);
        let l = body_lists(&d, 0, d.len()).unwrap();
        assert_eq!(l, BodyLists { made: vec![0x58], taken: vec![0x59], ids: vec![0x93] });
        // A cut that removes its body: makes nothing.
        let d = header(&[], &[0x3d], &[]);
        assert_eq!(body_lists(&d, 0, d.len()).unwrap().made, Vec::<u16>::new());
    }

    #[test]
    fn bodies_of_a_gear() {
        let ext = |n: &str, op: &str| json!({"type": "extrude", "name": n, "operation": op, "target": "B1", "profiles": "all"});
        let feats = vec![
            json!({"type": "extrude", "name": "B1", "operation": "new", "profiles": "all"}),
            ext("B6", "join"),
            ext("B9", "join"),
            json!({"type": "pattern_circular", "name": "P", "unresolved": true, "why": "a pattern of bodies",
                   "body_source": 0x82, "count": 3, "axis": {}, "angle": 360.0}),
            ext("B10", "join"),
            ext("C3", "cut"),
        ];
        let mut lists = HashMap::new();
        let l = |m: &[u16], t: &[u16], i: &[u32]| BodyLists { made: m.to_vec(), taken: t.to_vec(), ids: i.to_vec() };
        lists.insert("B1".to_string(), l(&[0x3d], &[], &[0x65]));
        lists.insert("B6".to_string(), l(&[0x3e], &[], &[0x73]));
        lists.insert("B9".to_string(), l(&[0x4a], &[], &[0x82]));
        lists.insert("P".to_string(), l(&[0x42, 0x43], &[], &[0x83, 0x84]));
        lists.insert("B10".to_string(), l(&[0x59], &[0x3e, 0x4a, 0x42, 0x43], &[0x92]));
        lists.insert("C3".to_string(), l(&[], &[0x3d], &[]));
        let mut own = HashMap::new();
        own.insert("B9".to_string(), json!([{"bbox": [0.0, 0.0, 12.0, 4.0]}]));
        let out = track(feats, &lists, &own);
        assert_eq!(names(&out), ["B6", "B9", "P", "B10", "B10 bodies"]);
        assert_eq!(out[0]["operation"], "new");
        assert_eq!(out[1]["operation"], "new");
        assert_eq!(out[1]["profiles"][0]["bbox"][2], 12.0);
        assert_eq!(out[2]["target"], "B9");
        assert_eq!(out[2]["merge"], false);
        assert!(out[2].get("unresolved").is_none());
        assert_eq!(out[3]["target"], "B6");
        assert_eq!(out[4]["tools"], json!(["B9", "P[1]", "P[2]"]));
    }

    #[test]
    fn unknown_tags_keep_the_tree() {
        let feats = vec![
            json!({"type": "extrude", "name": "B1", "operation": "new"}),
            json!({"type": "extrude", "name": "B2", "operation": "join", "target": "B1"}),
        ];
        let mut lists = HashMap::new();
        lists.insert("B1".to_string(), BodyLists { made: vec![1], taken: vec![], ids: vec![] });
        lists.insert("B2".to_string(), BodyLists { made: vec![2], taken: vec![], ids: vec![] });
        // B2 makes a new body: rewritten.
        assert_eq!(track(feats.clone(), &lists, &HashMap::new())[1]["operation"], "new");
        // A tag nothing made: as read.
        lists.insert("B2".to_string(), BodyLists { made: vec![2], taken: vec![7], ids: vec![] });
        assert_eq!(track(feats.clone(), &lists, &HashMap::new()), feats);
    }
}

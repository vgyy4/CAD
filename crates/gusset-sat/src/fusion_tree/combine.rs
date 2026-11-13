//! Combine `2A94257F` (ADR 0039 amendment, sections 21 and 22 of the format notes): tool bodies
//! joined to a target body. The record's body is `u32 operation` (0 = join: written as a cut, the
//! header part of 3900 fails its stored states, as a join it rebuilds them; 1 = cut, section
//! 55), `u32 n` and `n` references, a flag block,
//! then `u32 ntools` and that many `2CA5A1CD` selections (the tools), the tools' own body links
//! and `u32 1` and one `2CA5A1CD` (the target).
//!
//! No field keeps the tools (Fusion's Keep Tools, section 60): in every Combine of the sample sets
//! the tools' body records leave their component's body list (`CD57BC48`), while the component's
//! history blob keeps each tool's last shape. Those bodies are listed (`used_up`: the blob and the
//! body's tag in it, from the body map `74CA4562`) so that the importer does not take them for
//! end bodies.

use super::bodyops::Edit;
use super::stream::{self, parse_header, ref_ids, ref_lists, u32_at, u64_at};
use crate::cover;
use serde_json::{Value, json};

/// What a Combine becomes in the tree.
pub(super) enum Combined {
    /// A tree `combine`.
    Feature(Value),
    /// Not read (the reason).
    Refused(String),
}

impl Edit<'_> {
    /// The body links a `2CA5A1CD` selection names (through its `9716F783`).
    pub(super) fn links_of(&self, sel: u64) -> Vec<u64> {
        let mut out = Vec::new();
        for s in ref_ids(self.d, self.d.rec(sel)) {
            if self.d.is(s, "9716F783") {
                for l in ref_ids(self.d, self.d.rec(s)) {
                    if self.d.is(l, "D26351F0") && !out.contains(&l) {
                        out.push(l);
                    }
                }
            }
        }
        out
    }

    /// The tree entry of the feature that made a body link, and that feature's id.
    pub(super) fn maker(&self, link: u64) -> Option<(u64, &Value)> {
        let m = self.g.creator.get(&link).copied()?;
        let e = self
            .feats
            .iter()
            .find(|f| f.get("source_id").and_then(Value::as_u64) == Some(m) && f.get("name").is_some())?;
        if e.get("type").and_then(Value::as_str) != Some("stored_body") {
            return Some((m, e));
        }
        // A Base Feature's bodies are a stored body each, in the order of its body links (4431:
        // the Combine's tool is the connector's first body).
        let mine: Vec<u64> = ref_ids(self.d, self.d.rec(m))
            .into_iter()
            .filter(|l| self.d.is(*l, "D26351F0") && self.g.creator.get(l) == Some(&m))
            .collect();
        let stored: Vec<&Value> = self
            .feats
            .iter()
            .filter(|f| {
                f.get("source_id").and_then(Value::as_u64) == Some(m)
                    && f.get("type").and_then(Value::as_str) == Some("stored_body")
            })
            .collect();
        if stored.len() != mine.len() {
            // Not one stored body per link (lumps): only a feature of one body is that body.
            return (e.get("creates_bodies").and_then(Value::as_u64) == Some(1)).then_some((m, e));
        }
        let at = mine.iter().position(|l| *l == link)?;
        Some((m, stored.get(at)?))
    }

    /// A tool body the Combine used up, where the file still stores its shape: the history blob
    /// (`BREP.<id>.smbh`) of the tool's component and the tag of the tool's body in it, from the
    /// component's body map `74CA4562` (`u32 n`, then `n` pairs of the blob body's tag and the
    /// design's body record). `None` while the component's body list (`CD57BC48`, section 60)
    /// still holds the body, or where the map does not name it.
    fn used_up(&self, link: u64) -> Option<Value> {
        let d = self.d;
        let body = ref_ids(d, d.rec(link)).into_iter().find(|q| d.is(*q, "D3937028"))?;
        for list in ref_ids(d, d.rec(body)).into_iter().filter(|q| d.is(*q, "CD57BC48")) {
            let members = ref_ids(d, d.rec(list));
            if members.contains(&body) {
                return None;
            }
            for map in members.into_iter().filter(|q| d.is(*q, "74CA4562")) {
                let rec = d.rec(map);
                let Some(blob) = stream::wstrings_all(rec)
                    .into_iter()
                    .map(|w| w.2)
                    .find(|w| w.starts_with("BREP.") && w.ends_with(".smbh"))
                else {
                    continue;
                };
                let Some(h) = parse_header(rec) else { continue };
                let n = usize::try_from(u32_at(rec, h.body)?).ok()?;
                for k in 0..n.min(rec.len() / 16) {
                    let at = h.body + 4 + 16 * k;
                    if u64_at(rec, at + 8) == Some(body) {
                        cover::field(rec, h.body, 4 + 16 * n, "bodymap.entries");
                        return Some(json!({"blob": blob, "tag": u64_at(rec, at)?}));
                    }
                }
            }
        }
        None
    }

    pub(super) fn combine(&self, fid: u64, name: &str) -> Combined {
        let d = self.d;
        let rec = d.rec(fid);
        let refuse = |why: &str| Combined::Refused(why.to_string());
        let Some(h) = parse_header(rec) else { return refuse("the Combine's record is not recognised") };
        cover::field(rec, h.body, 4, "combine.operation");
        let op = match u32_at(rec, h.body) {
            Some(0) => "join",
            // 1 stands where a logo body is taken out of a case (5 records), as the API's
            // operations number them (join 0, cut 1); the importer's state checks confirm it
            // (6124's Combine1, section 55).
            Some(1) => "cut",
            _ => return refuse("the Combine's operation is not a known value"),
        };
        // The first list of selections is the tools, the next list of one selection the target.
        let lists: Vec<Vec<u64>> = ref_lists(d, rec, h.body + 4)
            .into_iter()
            .map(|(_, ids)| ids)
            .filter(|ids| ids.iter().all(|q| d.is(*q, "2CA5A1CD")))
            .collect();
        let (Some(tool_sels), Some([target_sel])) = (lists.first(), lists.get(1).map(Vec::as_slice)) else {
            return refuse("the Combine's tools and target are not where the known layout has them");
        };
        // A stored body is one body of its Base Feature (`maker` picks it).
        let one_body = |f: &Value| match f.get("type").and_then(Value::as_str) {
            Some("stored_body") => true,
            Some("extrude" | "sweep" | "revolve" | "mirror") => {
                f.get("creates_bodies").and_then(Value::as_u64) == Some(1)
            }
            _ => false,
        };
        let target = match self.links_of(*target_sel).as_slice() {
            [l] => match self.maker(*l) {
                Some((_, f)) if one_body(f) => f.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
                _ => return refuse("the Combine's target is not the one body of a feature of the tree"),
            },
            _ => return refuse("the Combine's target is not one body"),
        };
        let mut tools: Vec<String> = Vec::new();
        let mut copies: Vec<u64> = Vec::new();
        let mut used_up: Vec<Value> = Vec::new();
        for s in tool_sels {
            let links = self.links_of(*s);
            let [l] = links.as_slice() else { return refuse("a tool of the Combine is not one body") };
            used_up.extend(self.used_up(*l));
            match self.maker(*l) {
                Some((_, f)) if one_body(f) => {
                    let n = f.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
                    if n == target || tools.contains(&n) {
                        return refuse("the Combine names a body twice");
                    }
                    tools.push(n);
                }
                Some((m, f))
                    if matches!(f.get("type").and_then(Value::as_str), Some("pattern_linear" | "pattern_circular")) =>
                {
                    // A copy of a body pattern: a body of its own (`<pattern>[i]`). Which copy
                    // a link is, the record does not say; all copies of one pattern together
                    // need no such pairing.
                    if f.get("merge").and_then(Value::as_bool) != Some(false) {
                        return refuse("a tool of the Combine is a copy of a pattern that is not a body pattern");
                    }
                    copies.push(m);
                }
                _ => return refuse("a tool of the Combine is not the one body of a feature of the tree"),
            }
        }
        let mut patterns = copies.clone();
        patterns.sort_unstable();
        patterns.dedup();
        for p in patterns {
            // The tree entries of the pattern (two for a symmetric one) and the copies they make.
            let mut names: Vec<String> = Vec::new();
            for f in self.feats.iter().filter(|f| f.get("source_id").and_then(Value::as_u64) == Some(p)) {
                let (Some(n), Some(count)) =
                    (f.get("name").and_then(Value::as_str), f.get("count").and_then(Value::as_u64))
                else {
                    return refuse("a pattern whose copies the Combine takes has no count");
                };
                names.extend((1..count).map(|i| super::json::copy_name(n, i)));
            }
            let made = self.g.creator.values().filter(|c| **c == p).count();
            let taken = copies.iter().filter(|c| **c == p).count();
            if taken != made || names.len() != made {
                return refuse("the Combine takes some copies of a body pattern, not all of them");
            }
            tools.extend(names);
        }
        if tools.is_empty() {
            return refuse("the Combine has no tools");
        }
        let mut f = json!({"type": "combine", "name": name, "source_id": fid, "target": target,
            "tools": tools, "operation": op,
            "note": "operation = the record's first field (0: join, 1: cut)"});
        if let (false, Some(o)) = (used_up.is_empty(), f.as_object_mut()) {
            // The tools' shapes the history blobs keep after the Combine used them up.
            o.insert("used_up".into(), Value::Array(used_up));
        }
        Combined::Feature(f)
    }
}

//! Combine `2A94257F` (ADR 0039 amendment, sections 21 and 22 of the format notes): tool bodies
//! joined to a target body. The record's body is `u32 operation` (0 = join: written as a cut, the
//! header part of 3900 fails its stored states, as a join it rebuilds them; 1 is not confirmed
//! and not converted), `u32 n` and `n` references, a flag block,
//! then `u32 ntools` and that many `2CA5A1CD` selections (the tools), the tools' own body links
//! and `u32 1` and one `2CA5A1CD` (the target).

use super::bodyops::Edit;
use super::stream::{parse_header, ref_ids, ref_lists, u32_at};
use serde_json::{Value, json};

/// What a Combine becomes in the tree.
pub(super) enum Combined {
    /// A tree `combine`.
    Feature(Value),
    /// Nothing: the tree already holds its result (the reason).
    Redundant(String),
    /// Not read (the reason).
    Refused(String),
}

impl Edit<'_> {
    /// The body links a `2CA5A1CD` selection names (through its `9716F783`).
    fn links_of(&self, sel: u64) -> Vec<u64> {
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
    fn maker(&self, link: u64) -> Option<(u64, &Value)> {
        let m = self.g.creator.get(&link).copied()?;
        let e = self
            .feats
            .iter()
            .find(|f| f.get("source_id").and_then(Value::as_u64) == Some(m) && f.get("name").is_some())?;
        Some((m, e))
    }

    pub(super) fn combine(&self, fid: u64, name: &str) -> Combined {
        let d = self.d;
        let rec = d.rec(fid);
        let refuse = |why: &str| Combined::Refused(why.to_string());
        let Some(h) = parse_header(rec) else { return refuse("the Combine's record is not recognised") };
        let op = match u32_at(rec, h.body) {
            Some(0) => "join",
            // 1 stands where a logo body is taken out of a case (5 records), which reads like a
            // cut; no rebuild has confirmed it, so it is not converted.
            Some(1) => return refuse("the Combine's operation (value 1) is not confirmed by a rebuild yet"),
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
        let one_body = |f: &Value| {
            matches!(f.get("type").and_then(Value::as_str), Some("extrude" | "sweep" | "stored_body" | "mirror"))
                && f.get("creates_bodies").and_then(Value::as_u64) == Some(1)
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
        for s in tool_sels {
            let links = self.links_of(*s);
            let [l] = links.as_slice() else { return refuse("a tool of the Combine is not one body") };
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
                    // A copy of a body pattern. The tree's pattern of a body unites the copies
                    // with the original, so joining all the copies to that original is done.
                    let whole = f.get("target").and_then(Value::as_str) == Some(target.as_str())
                        && f.get("features").and_then(Value::as_array).is_some_and(Vec::is_empty);
                    if !whole || copies.first().is_some_and(|c| *c != m) {
                        return refuse("a tool of the Combine is a pattern copy of another body than its target");
                    }
                    copies.push(m);
                }
                _ => return refuse("a tool of the Combine is not the one body of a feature of the tree"),
            }
        }
        if let Some(p) = copies.first() {
            let made = self.g.creator.values().filter(|c| *c == p).count();
            if op != "join" || !tools.is_empty() || copies.len() != made {
                return refuse("the Combine takes pattern copies otherwise than joining all of them to their original");
            }
            return Combined::Redundant(
                "it joins the copies of a body pattern to the original, which the tree's pattern has done".into(),
            );
        }
        if tools.is_empty() {
            return refuse("the Combine has no tools");
        }
        Combined::Feature(json!({"type": "combine", "name": name, "source_id": fid, "target": target,
            "tools": tools, "operation": op,
            "note": "operation = the record's first field (0: join)"}))
    }
}

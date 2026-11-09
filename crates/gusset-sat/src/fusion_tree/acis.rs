//! A minimal reader for the binary ACIS blobs (`BREP.*.smb`, `.smbh`) of a Fusion file: what the
//! edge naming needs to check against the stored bodies, edges' end points per body and the
//! faces that carry a generator tag. Bounded, total: a blob that does not tokenize is skipped.

use super::geom::V3;
use std::collections::{HashMap, HashSet};

/// Most records one blob may hold.
const MAX_RECORDS: usize = 4_000_000;

#[derive(Debug, Default)]
struct ARec {
    ty: String,
    ptrs: Vec<i64>,
    /// Non-empty `0x07` strings.
    strs: Vec<String>,
    /// The first position (`0x13`) token.
    pos: Option<V3>,
}

fn find(b: &[u8], pat: &[u8]) -> Option<usize> {
    b.windows(pat.len()).position(|w| w == pat)
}

fn size_of(t: u8, wide: bool) -> Option<usize> {
    Some(match t {
        0x04 | 0x0c | 0x15 => {
            if wide {
                8
            } else {
                4
            }
        }
        0x05 | 0x03 => 4,
        0x06 | 0x16 => 8,
        0x13 | 0x14 => 24,
        0x0a | 0x0b | 0x0f | 0x10 | 0x11 => 0,
        _ => return None,
    })
}

fn records(b: &[u8]) -> Option<Vec<ARec>> {
    let h = find(b, b"\x0d\x09asmheader")?;
    let wide = b.starts_with(b"ASM BinaryFile8") || b.starts_with(b"ACIS BinaryFile8");
    let mut i = h;
    let mut cur = ARec::default();
    let mut names: Vec<String> = Vec::new();
    let mut out = Vec::new();
    while i < b.len() {
        let t = *b.get(i)?;
        i += 1;
        match t {
            0x07 | 0x0d | 0x0e => {
                let l = usize::from(*b.get(i)?);
                let s = b.get(i + 1..i + 1 + l)?;
                i += 1 + l;
                if !s.is_empty() {
                    let text: String = s.iter().map(|&c| char::from(c)).collect();
                    if t == 0x07 {
                        cur.strs.push(text);
                    } else {
                        names.push(text);
                    }
                }
            }
            0x08 => {
                let l = usize::from(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?));
                i += 2 + l;
            }
            0x09 | 0x12 => {
                let l = u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?) as usize;
                i = i.checked_add(4 + l)?;
            }
            _ => {
                let n = size_of(t, wide)?;
                let v = b.get(i..i + n)?;
                i += n;
                match t {
                    0x0c => {
                        let p = if n == 4 {
                            i64::from(i32::from_le_bytes(v.try_into().ok()?))
                        } else {
                            i64::from_le_bytes(v.try_into().ok()?)
                        };
                        cur.ptrs.push(p);
                    }
                    0x13 if cur.pos.is_none() => {
                        let f =
                            |k: usize| v.get(k * 8..k * 8 + 8).and_then(|s| s.try_into().ok()).map(f64::from_le_bytes);
                        cur.pos = Some([f(0)?, f(1)?, f(2)?]);
                    }
                    _ => {}
                }
                if t == 0x11 {
                    cur.ty = names.join("-");
                    names.clear();
                    out.push(std::mem::take(&mut cur));
                    if out.len() > MAX_RECORDS {
                        return None;
                    }
                }
            }
        }
    }
    Some(out)
}

fn is_body(t: &str) -> bool {
    t.ends_with("body") && !t.ends_with("_body")
}

/// The records reachable from body record `k` through pointers, without crossing into other
/// bodies, attributes or transforms.
fn closure(recs: &[ARec], k: usize) -> HashSet<usize> {
    let mut seen = HashSet::from([k]);
    let mut st = vec![k];
    while let Some(x) = st.pop() {
        let Some(r) = recs.get(x) else { continue };
        for &p in &r.ptrs {
            let Ok(p) = usize::try_from(p) else { continue };
            let Some(pr) = recs.get(p) else { continue };
            if !seen.contains(&p) && !is_body(&pr.ty) && !pr.ty.contains("attrib") && !pr.ty.contains("transform") {
                seen.insert(p);
                st.push(p);
            }
        }
    }
    seen
}

fn vertex_points(recs: &[ARec]) -> HashMap<usize, V3> {
    let pos: HashMap<usize, V3> = recs
        .iter()
        .enumerate()
        .filter(|(_, r)| r.ty.ends_with("point"))
        .filter_map(|(k, r)| Some((k, r.pos?)))
        .collect();
    let mut vpt = HashMap::new();
    for (k, r) in recs.iter().enumerate() {
        if r.ty.ends_with("vertex")
            && let Some(p) = r.ptrs.iter().find_map(|p| usize::try_from(*p).ok().and_then(|p| pos.get(&p)))
        {
            vpt.insert(k, *p);
        }
    }
    vpt
}

/// An edge of a stored body: its end points (cm).
pub(super) type Edge = (V3, V3);

/// Per body of the blob: its edges' end points.
pub(super) fn body_edges(b: &[u8]) -> Option<Vec<Vec<Edge>>> {
    let recs = records(b)?;
    let vpt = vertex_points(&recs);
    let mut out = Vec::new();
    for (k, r) in recs.iter().enumerate() {
        if !is_body(&r.ty) {
            continue;
        }
        let mut edges = Vec::new();
        for e in closure(&recs, k) {
            let Some(er) = recs.get(e) else { continue };
            if er.ty.ends_with("edge") && !er.ty.ends_with("coedge") {
                let vs: Vec<V3> = er
                    .ptrs
                    .iter()
                    .filter_map(|p| usize::try_from(*p).ok().and_then(|p| vpt.get(&p)).copied())
                    .collect();
                if let [a, b, ..] = vs.as_slice() {
                    edges.push((*a, *b));
                }
            }
        }
        if !edges.is_empty() {
            out.push(edges);
        }
    }
    Some(out)
}

/// A face carrying a generator tag (`generic_tag_attrib_def`): the tag text and the vertex
/// positions of its edges.
#[derive(Debug)]
pub(super) struct TaggedFace {
    pub tag: String,
    pub verts: Vec<V3>,
}

pub(super) fn tagged_faces(b: &[u8]) -> Option<Vec<TaggedFace>> {
    let recs = records(b)?;
    let vpt = vertex_points(&recs);
    let mut tag: HashMap<usize, String> = HashMap::new();
    for r in &recs {
        if !r.ty.contains("attrib") || r.strs.first().map(String::as_str) != Some("generic_tag_attrib_def") {
            continue;
        }
        let owner = r
            .ptrs
            .iter()
            .filter_map(|p| usize::try_from(*p).ok())
            .find(|p| recs.get(*p).is_some_and(|o| !o.ty.contains("attrib")));
        if let (Some(o), Some(t)) = (owner, r.strs.last())
            && recs.get(o).is_some_and(|o| o.ty.ends_with("face"))
        {
            tag.insert(o, t.clone());
        }
    }
    let mut ce_loop: HashMap<usize, Vec<usize>> = HashMap::new();
    for (k, r) in recs.iter().enumerate() {
        if r.ty.ends_with("coedge") {
            for p in &r.ptrs {
                if let Ok(p) = usize::try_from(*p)
                    && recs.get(p).is_some_and(|x| x.ty.ends_with("loop"))
                {
                    ce_loop.entry(p).or_default().push(k);
                }
            }
        }
    }
    let mut out = Vec::new();
    for (k, r) in recs.iter().enumerate() {
        if !is_body(&r.ty) {
            continue;
        }
        let seen = closure(&recs, k);
        let mut faces: Vec<usize> = seen.iter().copied().filter(|f| tag.contains_key(f)).collect();
        faces.sort_unstable();
        for f in faces {
            let Some(fr) = recs.get(f) else { continue };
            let mut verts: Vec<V3> = Vec::new();
            for p in &fr.ptrs {
                let Ok(l) = usize::try_from(*p) else { continue };
                if !recs.get(l).is_some_and(|x| x.ty.ends_with("loop")) {
                    continue;
                }
                for ce in ce_loop.get(&l).map(Vec::as_slice).unwrap_or(&[]) {
                    let Some(cr) = recs.get(*ce) else { continue };
                    for p in &cr.ptrs {
                        let Ok(e) = usize::try_from(*p) else { continue };
                        let Some(er) = recs.get(e) else { continue };
                        if er.ty.ends_with("edge") && !er.ty.ends_with("coedge") {
                            for q in &er.ptrs {
                                if let Some(v) = usize::try_from(*q).ok().and_then(|q| vpt.get(&q))
                                    && !verts.contains(v)
                                {
                                    verts.push(*v);
                                }
                            }
                        }
                    }
                }
            }
            if let Some(t) = tag.get(&f) {
                out.push(TaggedFace { tag: t.clone(), verts });
            }
        }
    }
    Some(out)
}

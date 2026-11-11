//! What is in Fusion design streams, byte by byte (a reading aid; `docs/formats/fusion-coverage.md`).
//! Runs when `GUSSET_F3D_SAMPLES` names a folder of `.f3d`/`.f3z` files (searched recursively, not
//! in the repository) and `GUSSET_F3D_COVERAGE` an output folder. For every design it writes
//! `<set>-<number>.txt`: each BulkStream record with its class, offset and length, the fields the
//! reader knows by name and the unknown spans in hex with guessed references, strings, doubles
//! and the model parameters they equal. `summary.md` aggregates the corpus: the share of bytes
//! decoded, the record classes no reader touches ranked by the designs that hold them, and the
//! layout of the most common ones compared across samples.
//!
//! `GUSSET_F3D_OUTCOMES` names the log of the `f3d_history` corpus test run with
//! `GUSSET_F3D_VERBOSE` on the same folder: each design's import outcome (history, or the blocker)
//! is then listed against the classes. `GUSSET_F3D_ONLY` keeps the files whose path contains the
//! text. Designs are named by their folder and leading number only.

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use gusset_sat::fusion_tree::{Coverage, Guess, RecordCover, SpanKind};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

type Entries = Vec<(String, Vec<u8>)>;
/// Class, section class, offset in the section, width, parameter name.
type FieldKey = (String, String, isize, &'static str, String);

fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("f3d") || x.eq_ignore_ascii_case("f3z")) {
                files.push(p);
            }
        }
    }
    files.sort();
    files
}

fn fnv(b: &[u8]) -> u64 {
    b.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, c| (h ^ u64::from(*c)).wrapping_mul(0x0100_0000_01b3))
}

/// `<first folder under the samples>-<leading number of the file name>` (a hash when the name has
/// no number): no model names.
fn label(root: &Path, f: &Path) -> String {
    let rel = f.strip_prefix(root).unwrap_or(f);
    let set = rel.components().next().map(|c| c.as_os_str().to_string_lossy().into_owned()).unwrap_or_default();
    let name = f.file_name().unwrap().to_string_lossy();
    let num: String = name.chars().take_while(char::is_ascii_digit).collect();
    let num = if num.is_empty() { format!("h{:08x}", fnv(name.as_bytes()) as u32) } else { num };
    format!("{set}-{num}")
}

/// The designs of a file: one for an `.f3d`, each held `.f3d` for an `.f3z`.
fn designs_of(bytes: &[u8]) -> Vec<Entries> {
    let limits = gusset_meshio::Limits::default();
    let mut budget = limits.max_decompressed_bytes;
    let keep = |n: &str| n.ends_with("Stream.dat") || n.ends_with(".smb") || n.ends_with(".smbh");
    if let Ok(e) = gusset_meshio::archive::read_entries_within(bytes, &limits, &keep, 256 << 20, &mut budget)
        && e.iter().any(|(n, _)| n.ends_with("Stream.dat"))
    {
        return vec![e];
    }
    let mut out = Vec::new();
    if let Ok(inner) = gusset_meshio::archive::read_entries_within(
        bytes,
        &limits,
        &|n| n.to_ascii_lowercase().ends_with(".f3d"),
        256 << 20,
        &mut budget,
    ) {
        for (_, data) in inner {
            if let Ok(e) = gusset_meshio::archive::read_entries_within(&data, &limits, &keep, 256 << 20, &mut budget) {
                out.push(e);
            }
        }
    }
    out
}

/// Import outcome per file from an `f3d_history` verbose log (matched by file name): `None` for
/// history, else the blocker text.
fn outcomes(log: &str, files: &[PathBuf]) -> HashMap<PathBuf, Option<String>> {
    let mut out = HashMap::new();
    for line in log.lines() {
        for f in files {
            // By file name, so a log of the same files in another folder serves too.
            let needle = format!("/{}: ", f.file_name().unwrap().to_string_lossy());
            let Some(rest) = line.find(&needle).and_then(|k| line.get(k + needle.len()..)) else { continue };
            if rest.starts_with("raw ") {
                continue;
            }
            out.insert(f.clone(), if rest.starts_with("history") { None } else { Some(rest.to_string()) });
        }
    }
    out
}

/// A blocker without its numbers (as `f3d_history` prints reasons, shortened).
fn blocker_kind(why: &str) -> String {
    let s: String = why.chars().take(90).collect();
    s.replace(|c: char| c.is_ascii_digit(), "#")
}

#[derive(Default)]
struct ClassStat {
    designs: BTreeSet<String>,
    records: usize,
    bytes: usize,
    tally: [usize; 3],
    /// Decoded bytes under a reader's own field name (not framing or a generic reference/string).
    field_bytes: usize,
    tags: BTreeSet<u32>,
}

fn framing(name: &str) -> bool {
    name.starts_with("header.")
        || name.starts_with("section.")
        || matches!(name, "ref" | "wstr" | "own-id" | "list.count" | "list.pad")
}

fn pct(a: usize, b: usize) -> f64 {
    100.0 * a as f64 / b.max(1) as f64
}

/// A record's header length (where its own body starts).
fn body0(r: &RecordCover) -> usize {
    r.sections.first().map_or(0, |s| s.0)
}

fn hex(b: &[u8], max: usize) -> String {
    let mut s: String = b.iter().take(max).map(|c| format!("{c:02x}")).collect();
    if b.len() > max {
        s.push('…');
    }
    s
}

/// The layout of a class's most common record shape, compared across its records: runs of bytes
/// that are decoded, constant or varying, with the parameters the varying ones equal.
fn layout(class: &str, recs: &[(&str, &RecordCover)], out: &mut String) {
    let mut by_len: BTreeMap<usize, Vec<&(&str, &RecordCover)>> = BTreeMap::new();
    for r in recs {
        by_len.entry(r.1.bytes.len() - body0(r.1)).or_default().push(r);
    }
    let mut lens: Vec<(usize, usize)> = by_len.iter().map(|(l, v)| (*l, v.len())).collect();
    lens.sort_by_key(|x| std::cmp::Reverse(x.1));
    let shapes: Vec<String> = lens.iter().take(4).map(|(l, n)| format!("{l} ({n})")).collect();
    let _ = writeln!(
        out,
        "\n#### `{class}`\n\nBody lengths after the header (records): {}{}.",
        shapes.join(", "),
        if lens.len() > 4 { format!(", {} more", lens.len() - 4) } else { String::new() }
    );
    let Some(&(bl, _)) = lens.first() else { return };
    let group = &by_len[&bl];
    let designs: BTreeSet<&str> = group.iter().map(|r| r.0).collect();
    let _ = writeln!(
        out,
        "Most common shape: {bl} bytes, {} records in {} designs. Offsets from the body start:\n",
        group.len(),
        designs.len()
    );
    let n = group.len();
    // Per position: majority kind and name, and whether the byte is the same in every record.
    let mut runs: Vec<(usize, usize, String)> = Vec::new();
    let mut p = 0;
    let at = |r: &RecordCover, p: usize| body0(r) + p;
    let kind_at = |r: &RecordCover, p: usize| {
        let a = at(r, p);
        r.spans
            .iter()
            .find(|s| s.start <= a && a < s.start + s.len)
            .map_or((SpanKind::Unknown, ""), |s| (s.kind, s.name))
    };
    while p < bl {
        let mut kinds: HashMap<(SpanKind, &str), usize> = HashMap::new();
        for r in group {
            *kinds.entry(kind_at(r.1, p)).or_default() += 1;
        }
        let (k, _) = kinds.into_iter().max_by_key(|x| (x.1, x.0)).unwrap();
        let same =
            |p: usize| group.iter().all(|r| r.1.bytes.get(at(r.1, p)) == group[0].1.bytes.get(at(group[0].1, p)));
        let class_of = |p: usize| {
            if k.0 != SpanKind::Unknown {
                format!("{:?} {}", k.0, k.1)
            } else if same(p) {
                "const".to_string()
            } else {
                "varies".to_string()
            }
        };
        let c = class_of(p);
        let mut q = p + 1;
        while q < bl
            && {
                let mut kk: HashMap<(SpanKind, &str), usize> = HashMap::new();
                for r in group {
                    *kk.entry(kind_at(r.1, q)).or_default() += 1;
                }
                kk.into_iter().max_by_key(|x| (x.1, x.0)).unwrap().0 == k
            }
            && class_of(q) == c
        {
            q += 1;
        }
        runs.push((p, q, c));
        p = q;
    }
    let _ = writeln!(out, "| offset | len | what | values / evidence |\n|---|---|---|---|");
    for (a, b, c) in runs.iter().take(60) {
        let len = b - a;
        let first = group[0].1;
        let ev = if c == "const" {
            format!("`{}`", hex(first.bytes.get(at(first, *a)..at(first, *b)).unwrap_or(&[]), 16))
        } else if c == "varies" {
            let mut ev = Vec::new();
            // Guessed references and strings that start in the run, by majority.
            let mut refs: HashMap<String, usize> = HashMap::new();
            let mut hits: HashMap<(usize, &str, &str), usize> = HashMap::new();
            let mut distinct: BTreeSet<&[u8]> = BTreeSet::new();
            for (_, r) in group {
                let (ra, rb) = (at(r, *a), at(r, *b));
                distinct.insert(r.bytes.get(ra..rb).unwrap_or(&[]));
                for (g0, _, g) in r.guesses.iter().filter(|g| ra <= g.0 && g.0 < rb) {
                    let what = match g {
                        Guess::Ref { class, .. } => format!("ref to {}", class.get(..8).unwrap_or(class)),
                        Guess::WStr(_) => "wstr".into(),
                        Guess::Str(_) => "str".into(),
                        Guess::F64(_) => continue,
                    };
                    *refs.entry(format!("+{} {what}", g0 - ra)).or_default() += 1;
                }
                for h in r.param_hits.iter().filter(|h| ra <= h.at && h.at < rb) {
                    *hits.entry((h.at - ra, h.width, h.param.as_str())).or_default() += 1;
                }
            }
            let mut refs: Vec<_> = refs.into_iter().filter(|x| x.1 * 2 >= n).collect();
            refs.sort();
            for (w, k) in refs.iter().take(4) {
                ev.push(format!("{w} ({k}/{n})"));
            }
            let mut hits: Vec<_> = hits.into_iter().filter(|x| x.1 * 4 >= n || x.1 >= 3).collect();
            hits.sort_by_key(|x| (std::cmp::Reverse(x.1), x.0));
            for ((o, w, pn), k) in hits.iter().take(4) {
                ev.push(format!("+{o} {w} = parameter `{pn}` ({k}/{n})"));
            }
            let samples: Vec<String> = distinct.iter().take(3).map(|v| format!("`{}`", hex(v, 12))).collect();
            ev.push(format!("{} distinct, e.g. {}", distinct.len(), samples.join(" ")));
            ev.join("; ")
        } else {
            String::new()
        };
        let _ = writeln!(out, "| +{a} | {len} | {c} | {ev} |");
    }
    if runs.len() > 60 {
        let _ = writeln!(out, "| … | | {} more runs | |", runs.len() - 60);
    }
}

#[test]
fn fusion_design_streams_are_covered_on_request() {
    let (Ok(dir), Ok(out)) = (std::env::var("GUSSET_F3D_SAMPLES"), std::env::var("GUSSET_F3D_COVERAGE")) else {
        return;
    };
    let root = PathBuf::from(&dir);
    let out = PathBuf::from(out);
    std::fs::create_dir_all(&out).unwrap();
    let mut files = files_under(&root);
    if let Ok(only) = std::env::var("GUSSET_F3D_ONLY") {
        files.retain(|f| f.to_string_lossy().contains(&only));
    }
    let outcome = std::env::var("GUSSET_F3D_OUTCOMES")
        .ok()
        .map(|p| outcomes(&std::fs::read_to_string(p).unwrap(), &files))
        .unwrap_or_default();

    // Distinct streams: (label, coverage); a stream met again under another file is counted once.
    let mut seen: HashMap<u64, String> = HashMap::new();
    let mut covs: Vec<(String, Coverage)> = Vec::new();
    let mut design_outcome: BTreeMap<String, Option<String>> = BTreeMap::new();
    let mut unreadable = Vec::new();
    let (mut meta_len, mut meta_used) = (0usize, 0usize);
    let mut labels_used: BTreeSet<String> = BTreeSet::new();
    for f in &files {
        let bytes = std::fs::read(f).unwrap();
        let designs = designs_of(&bytes);
        let base = label(&root, f);
        for (k, entries) in designs.iter().enumerate() {
            let mut lab = if designs.len() > 1 { format!("{base}.{k}") } else { base.clone() };
            while !labels_used.insert(lab.clone()) {
                lab.push('+');
            }
            let key = entries.iter().find(|(n, _)| n.ends_with("BulkStream.dat")).map_or(0, |(_, b)| fnv(b));
            match gusset_sat::fusion_tree::coverage(entries) {
                Ok(c) => {
                    std::fs::write(out.join(format!("{lab}.txt")), c.dump()).unwrap();
                    if let Some(first) = seen.get(&key) {
                        eprintln!("{lab}: same stream as {first}");
                        continue;
                    }
                    seen.insert(key, lab.clone());
                    meta_len += c.meta_len;
                    meta_used += c.meta_used;
                    if let Some(o) = outcome.get(f) {
                        design_outcome.insert(lab.clone(), o.clone());
                    }
                    covs.push((lab, c));
                }
                Err(e) => unreadable.push(format!("{lab}: {e}")),
            }
        }
    }

    // Totals and per class.
    let mut total = [0usize; 3];
    let mut outside = 0usize;
    let mut bulk = 0usize;
    let mut classes: BTreeMap<String, ClassStat> = BTreeMap::new();
    let mut names: BTreeMap<&'static str, usize> = BTreeMap::new();
    for (lab, c) in &covs {
        bulk += c.bulk_len;
        outside += c.outside_records;
        for r in &c.records {
            let t = r.tally();
            for (a, b) in total.iter_mut().zip(t) {
                *a += b;
            }
            let cs = classes.entry(r.class.clone()).or_default();
            cs.designs.insert(lab.clone());
            cs.records += 1;
            cs.bytes += r.bytes.len();
            if let Some(tag) = r.tag {
                cs.tags.insert(tag);
            }
            for (a, b) in cs.tally.iter_mut().zip(t) {
                *a += b;
            }
            for s in &r.spans {
                if s.kind != SpanKind::Unknown {
                    *names.entry(s.name).or_default() += s.len;
                }
                if s.kind == SpanKind::Decoded && !framing(s.name) {
                    cs.field_bytes += s.len;
                }
            }
        }
    }
    let failing: BTreeMap<&String, &String> =
        design_outcome.iter().filter_map(|(l, o)| o.as_ref().map(|w| (l, w))).collect();
    let history = design_outcome.values().filter(|o| o.is_none()).count();

    let mut s = String::new();
    let [u, sk, d] = total;
    let all = u + sk + d;
    let _ = writeln!(
        s,
        "## Corpus\n\n{} files, {} distinct design streams ({} unreadable); import outcomes known for {} \
         ({history} as history, {} blocked).\n",
        files.len(),
        covs.len(),
        unreadable.len(),
        design_outcome.len(),
        failing.len()
    );
    let _ = writeln!(
        s,
        "| | bytes | share |\n|---|---|---|\n| BulkStream, in records | {all} | 100% |\n| decoded | {d} | {:.1}% |\n\
         | decoded by a reader's own field (not framing, references or strings) | {} | {:.1}% |\n\
         | known-skipped | {sk} | {:.1}% |\n| unknown | {u} | {:.1}% |\n| BulkStream outside records | {outside} | of {bulk} |\n\
         | MetaStream read by the framing | {meta_used} | {:.1}% of {meta_len} |\n",
        pct(d, all),
        classes.values().map(|c| c.field_bytes).sum::<usize>(),
        pct(classes.values().map(|c| c.field_bytes).sum::<usize>(), all),
        pct(sk, all),
        pct(u, all),
        pct(meta_used, meta_len),
    );
    let mut nm: Vec<_> = names.iter().collect();
    nm.sort_by_key(|x| std::cmp::Reverse(*x.1));
    let _ = writeln!(s, "Bytes by field name (decoded and skipped), largest first:\n");
    for (n, b) in nm.iter().take(40) {
        let _ = writeln!(s, "- `{n}`: {b} ({:.1}%)", pct(**b, all));
    }

    // Classes no reader touches, by the designs that hold them.
    let mut untouched: Vec<(&String, &ClassStat)> = classes.iter().filter(|(_, c)| c.field_bytes == 0).collect();
    untouched.sort_by_key(|(k, c)| (std::cmp::Reverse(c.designs.len()), std::cmp::Reverse(c.tally[0]), (*k).clone()));
    let _ = writeln!(
        s,
        "\n## Record classes no reader decodes a field of ({} of {} classes, {} unknown bytes)\n\n\
         Ranked by the designs that hold them. `blocked` = designs holding the class whose import \
         still stops at a blocker; the blockers are the most common among them.\n\n\
         | class | tag(s) | designs | blocked | records | bytes | unknown | top blockers of those designs |\n|---|---|---|---|---|---|---|---|",
        untouched.len(),
        classes.len(),
        untouched.iter().map(|c| c.1.tally[0]).sum::<usize>()
    );
    let blockers_of = |c: &ClassStat| {
        let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
        let mut n = 0;
        for l in &c.designs {
            if let Some(w) = failing.get(l) {
                n += 1;
                *kinds.entry(blocker_kind(w)).or_default() += 1;
            }
        }
        let mut k: Vec<_> = kinds.into_iter().collect();
        k.sort_by_key(|x| std::cmp::Reverse(x.1));
        let top: Vec<String> = k.iter().take(3).map(|(w, n)| format!("{n}× {w}")).collect();
        (n, top.join("<br>"))
    };
    for (k, c) in untouched.iter().take(40) {
        let (nb, top) = blockers_of(c);
        let tags: Vec<String> = c.tags.iter().take(3).map(u32::to_string).collect();
        let _ = writeln!(
            s,
            "| `{}` | {} | {} | {nb} | {} | {} | {:.0}% | {top} |",
            if k.is_empty() { "(no class)" } else { k.as_str() },
            tags.join(","),
            c.designs.len(),
            c.records,
            c.bytes,
            pct(c.tally[0], c.bytes)
        );
    }

    // Untouched classes over-represented among the blocked designs.
    if !failing.is_empty() {
        let (nd, nf) = (design_outcome.len() as f64, failing.len() as f64);
        let mut lift: Vec<(f64, &String, usize, usize)> = untouched
            .iter()
            .filter_map(|(k, c)| {
                let held = c.designs.iter().filter(|l| design_outcome.contains_key(*l)).count();
                let blocked = c.designs.iter().filter(|l| failing.contains_key(l)).count();
                (blocked >= 2).then(|| ((blocked as f64 / nf) / (held as f64 / nd), *k, held, blocked))
            })
            .collect();
        lift.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.3.cmp(&a.3)));
        let _ = writeln!(
            s,
            "\n## Untouched classes held mostly by blocked designs\n\n`lift` = share of blocked designs holding the \
             class / share of all designs (with an outcome) holding it; 1 = no relation. At least two blocked designs.\n\n\
             | class | designs | blocked | lift | top blockers of those designs |\n|---|---|---|---|---|"
        );
        for (l, k, held, blocked) in lift.iter().filter(|x| x.0 > 1.2).take(25) {
            let (_, top) = blockers_of(&classes[*k]);
            let _ = writeln!(s, "| `{k}` | {held} | {blocked} | {l:.2} | {top} |");
        }
    }

    // Classes read in part, by unknown bytes.
    let mut partial: Vec<(&String, &ClassStat)> = classes.iter().filter(|(_, c)| c.field_bytes > 0).collect();
    partial.sort_by_key(|(_, c)| std::cmp::Reverse(c.tally[0]));
    let _ = writeln!(
        s,
        "\n## Record classes read in part, by unknown bytes\n\n| class | designs | blocked | records | bytes | decoded | unknown |\n|---|---|---|---|---|---|---|"
    );
    for (k, c) in partial.iter().take(25) {
        let (nb, _) = blockers_of(c);
        let _ = writeln!(
            s,
            "| `{k}` | {} | {nb} | {} | {} | {:.0}% | {:.0}% |",
            c.designs.len(),
            c.records,
            c.bytes,
            pct(c.tally[2], c.bytes),
            pct(c.tally[0], c.bytes)
        );
    }

    // Blockers and the untouched classes their designs hold that most other designs do not.
    let _ = writeln!(
        s,
        "\n## Blocked designs\n\n| design | blocker | untouched classes held (most designs first) |\n|---|---|---|"
    );
    for (l, w) in &failing {
        let mut held: Vec<(&String, usize)> =
            untouched.iter().filter(|(_, c)| c.designs.contains(*l)).map(|(k, c)| (*k, c.designs.len())).collect();
        held.sort_by_key(|x| std::cmp::Reverse(x.1));
        let held: Vec<String> = held.iter().take(6).map(|(k, _)| format!("`{}`", k.get(..8).unwrap_or(k))).collect();
        let _ = writeln!(s, "| {l} | {} | {} |", blocker_kind(w), held.join(" "));
    }

    // The layouts of the most common untouched classes, compared across samples.
    let _ = writeln!(s, "\n## Layouts of the ten most common untouched classes\n");
    for (k, _) in untouched.iter().filter(|(k, _)| !k.is_empty()).take(10) {
        let recs: Vec<(&str, &RecordCover)> = covs
            .iter()
            .flat_map(|(l, c)| c.records.iter().filter(|r| &r.class == *k).map(move |r| (l.as_str(), r)))
            .collect();
        layout(k, &recs, &mut s);
    }

    // Fields anywhere that equal a known parameter in most records of their class and shape.
    let mut fields: BTreeMap<FieldKey, BTreeSet<(String, u64)>> = BTreeMap::new();
    let mut per_class: HashMap<&str, usize> = HashMap::new();
    for (l, c) in &covs {
        for r in &c.records {
            *per_class.entry(r.class.as_str()).or_default() += 1;
            for h in &r.param_hits {
                let (sec, rel) = r.section_of(h.at);
                fields
                    .entry((r.class.clone(), sec.chars().take(8).collect(), rel, h.width, h.param.clone()))
                    .or_default()
                    .insert((l.clone(), r.id));
            }
        }
    }
    let mut strong: Vec<_> = fields
        .into_iter()
        .filter(|((cl, _, _, w, _), recs)| {
            let n = per_class.get(cl.as_str()).copied().unwrap_or(0);
            recs.len() >= 3 && (recs.len() * 3 >= n || *w == "f64" && recs.len() >= 5)
        })
        .collect();
    strong.sort_by_key(|x| std::cmp::Reverse(x.1.len()));
    let _ = writeln!(
        s,
        "\n## Unknown fields that equal a known parameter\n\nA double (`f64`) or integer (`u32`) in an unknown span \
         that equals a model parameter of a record within two references (role name given), at the same offset \
         of the same section in at least three records.\n\n| class | section | offset | width | parameter | records (of the class) | designs |\n|---|---|---|---|---|---|---|"
    );
    for ((cl, sec, rel, w, pn), recs) in strong.iter().take(40) {
        let designs: BTreeSet<&String> = recs.iter().map(|r| &r.0).collect();
        let _ = writeln!(
            s,
            "| `{}` | `{sec}` | +{rel} | {w} | `{pn}` | {} ({}) | {} |",
            cl.get(..8).unwrap_or(cl),
            recs.len(),
            per_class.get(cl.as_str()).copied().unwrap_or(0),
            designs.len()
        );
    }
    if !unreadable.is_empty() {
        let _ = writeln!(s, "\n## Designs without a readable stream\n");
        for u in &unreadable {
            let _ = writeln!(s, "- {u}");
        }
    }
    std::fs::write(out.join("summary.md"), &s).unwrap();
    eprintln!(
        "{} streams: decoded {:.1}%, skipped {:.1}%, unknown {:.1}% of {all} record bytes; summary in {}",
        covs.len(),
        pct(d, all),
        pct(sk, all),
        pct(u, all),
        out.join("summary.md").display()
    );
}

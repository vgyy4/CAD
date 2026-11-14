#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::print_stderr)]
//! A local check over KiCad sample boards the user has (never in the repository: KiCad's
//! demo boards are CC-BY-SA). Runs only with `GUSSET_KICAD_SAMPLES=<folder of .kicad_pcb>`;
//! use a release build for the larger boards.

use crate::board::{Board, Limits, PadKind};
use crate::rules::Rules;
use crate::{drc, gerber, push, route, shove};

#[test]
fn local_samples_phase3() {
    let Some(dir) = std::env::var_os("GUSSET_KICAD_SAMPLES") else { return };
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "kicad_pcb"))
        .collect();
    paths.sort();
    for p in paths {
        let text = std::fs::read_to_string(&p).unwrap();
        let b = Board::read(&text, &Limits::default()).unwrap();
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        // Texts plotted.
        let t = std::time::Instant::now();
        let files = gerber::write_all(&b).unwrap();
        let strokes: usize = b.copper_layers().len() + files.len();
        let shown = b.texts.iter().filter(|t| !t.hidden).count();
        eprintln!("{name}: {} texts shown, gerbers in {:?} ({strokes} files)", shown, t.elapsed());
        // A rename there and back gives the file back.
        if let Some(n) = b.nets.iter().find(|n| !n.name.is_empty()) {
            let mut e = b.clone();
            e.rename_net(&n.name, "GUSSET_TMP", &Limits::default()).unwrap();
            e.rename_net("GUSSET_TMP", &n.name, &Limits::default()).unwrap();
            assert_eq!(e.tree, b.tree, "{name}: rename and back");
        }
        // Pad pairs of up to 30 nets on F.Cu, every other track in place: walkaround, then
        // shove.
        let rules = Rules::for_board(&b);
        let before = drc::check(&b, &rules, 100_000).len();
        let mut pairs = Vec::new();
        for n in b.nets.iter().filter(|n| !n.name.is_empty()) {
            let pads: Vec<_> = b
                .pads
                .iter()
                .filter(|p| b.net_name(&p.net).as_deref() == Some(n.name.as_str()))
                .filter(|p| p.kind != PadKind::Npth && b.on_layer(&p.layers, "F.Cu"))
                .collect();
            if let [a, c, ..] = pads.as_slice() {
                pairs.push((n.name.clone(), a.at, c.at));
            }
            if pairs.len() >= 30 {
                break;
            }
        }
        // Walkaround; walkaround + the phase 3 shove (one chain re-laid); walkaround + the
        // push-and-shove of ADR 0058 (then the phase 3 shove).
        let (mut walked, mut old, mut new, mut new_push) = (0, 0, 0, 0);
        let (mut t_walk, mut t_old, mut t_new) =
            (std::time::Duration::ZERO, std::time::Duration::ZERO, std::time::Duration::ZERO);
        let drc_ok = |nb: &Board| drc::check(nb, &rules, 100_000).len() <= before;
        let lay = |nb: &mut Board, path: &[[f64; 2]], net: &str, w: f64| {
            let segs: Vec<route::Seg> =
                path.windows(2).map(|x| route::Seg { start: x[0], end: x[1], mid: None }).collect();
            nb.add_route(&segs, w, "F.Cu", Some(net), &Limits::default()).unwrap();
        };
        for (net, a, c) in &pairs {
            let w = rules.track_width_of(Some(net));
            let grid = (rules.clearance_of(Some(net)).min(w) / 2.0).clamp(0.05, 0.25);
            let t = std::time::Instant::now();
            let obs = route::Obstacles::new(&b, &rules, "F.Cu", Some(net), w);
            let walk = route::walkaround(&obs, *a, *c, grid, true).is_some();
            t_walk += t.elapsed();
            if walk {
                walked += 1;
                continue;
            }
            let t = std::time::Instant::now();
            let s = shove::shove(&b, &rules, "F.Cu", Some(net), w, *a, *c, grid, &[]);
            let old_ok = s.as_ref().is_some_and(|s| {
                let mut nb = b.clone();
                nb.apply_shove(s, "F.Cu", &Limits::default()).unwrap();
                lay(&mut nb, &s.path, net, w);
                drc_ok(&nb)
            });
            t_old += t.elapsed();
            old += usize::from(old_ok);
            let t = std::time::Instant::now();
            let p = push::push_reason(&b, &rules, "F.Cu", Some(net), w, *a, *c, grid, &[], Some(before));
            if let Err(why) = &p {
                eprintln!("  {name} {net}: no push: {why}");
            }
            if let Ok(p) = &p {
                let mut nb = b.clone();
                nb.apply_push(p, "F.Cu", &Limits::default()).unwrap();
                lay(&mut nb, &p.path, net, w);
                assert!(drc_ok(&nb), "{name}: {net}: the push made the check worse");
                new_push += 1;
                new += 1;
                eprintln!("  {name} {net}: pushed {} lines, {} vias", p.lines.len(), p.vias.len());
            } else if old_ok {
                new += 1;
            }
            t_new += t.elapsed();
        }
        eprintln!(
            "{name}: {} pairs, DRC {before}; walkaround {walked} ({t_walk:?}); + old shove {} ({t_old:?}); \
             + push-and-shove {} ({new_push} by pushing; {t_new:?} incl. the old shove fallback)",
            pairs.len(),
            walked + old,
            walked + new,
        );
    }
}

/// ADR 0054: counts per code with representative custom rules, against the board's own rules
/// (the demo boards ship no `.kicad_dru`).
#[test]
fn local_samples_custom_rules() {
    use std::collections::BTreeMap;
    let Some(dir) = std::env::var_os("GUSSET_KICAD_SAMPLES") else { return };
    let dru = "(version 1)\n\
        (rule \"gnd wide\" (condition \"A.NetName == 'GND' && A.Type == 'track'\") (constraint track_width (min 0.25)))\n\
        (rule \"close pairs\" (condition \"A.Type == 'track' && B.Type == 'track'\") (constraint clearance (min 0.25)))\n\
        (rule \"via size\" (condition \"A.Type == 'via'\") (constraint via_diameter (min 0.7)))\n\
        (rule \"drills\" (constraint hole_size (min 0.35)))\n\
        (rule \"edge\" (condition \"A.Type == 'track'\") (constraint edge_clearance (min 0.5)))\n";
    let custom = crate::dru::CustomRules::parse(dru);
    assert!(custom.warnings.is_empty(), "{:?}", custom.warnings);
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "kicad_pcb"))
        .collect();
    paths.sort();
    let tally = |v: &[drc::Violation]| {
        let mut m: BTreeMap<String, usize> = BTreeMap::new();
        for x in v {
            let key = format!("{}{}", x.code, x.rule.as_ref().map_or(String::new(), |r| format!("[{r}]")));
            *m.entry(key).or_default() += 1;
        }
        m
    };
    for p in paths {
        let b = Board::read(&std::fs::read_to_string(&p).unwrap(), &Limits::default()).unwrap();
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        let base = Rules::for_board(&b);
        let t = std::time::Instant::now();
        let plain = drc::check(&b, &base, 1_000_000);
        let t_plain = t.elapsed();
        let mut rules = base.clone();
        rules.custom = custom.clone();
        let t = std::time::Instant::now();
        let with = drc::check(&b, &rules, 1_000_000);
        eprintln!(
            "{name}: plain {} {:?} ({t_plain:?}); custom {} ({:?})",
            plain.len(),
            tally(&plain),
            with.len(),
            t.elapsed()
        );
        eprintln!("{name}: custom tally {:?}", tally(&with));
        for r in ["gnd wide", "close pairs", "via size", "drills", "edge"] {
            if let Some(v) = with.iter().find(|v| v.rule.as_deref() == Some(r)) {
                eprintln!("  e.g. {} @ {:?}: {}", v.code, v.at, v.message);
            }
        }
    }
}

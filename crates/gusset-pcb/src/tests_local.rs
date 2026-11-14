#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::print_stderr, clippy::float_cmp)]
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
                // Whether the pads alone leave a way (the other nets' tracks and vias gone).
                let mut bare = b.clone();
                let tracks: Vec<usize> = (0..b.tracks.len())
                    .filter(|&i| b.tracks[i].layer == "F.Cu" && b.net_name(&b.tracks[i].net).as_deref() != Some(net))
                    .collect();
                let vias: Vec<usize> =
                    (0..b.vias.len()).filter(|&i| b.net_name(&b.vias[i].net).as_deref() != Some(net)).collect();
                bare.delete_items(&tracks, &vias, &Limits::default()).unwrap();
                let open =
                    route::walkaround(&route::Obstacles::new(&bare, &rules, "F.Cu", Some(net), w), *a, *c, grid, true)
                        .is_some();
                eprintln!("  {name} {net}: no push: {why} (a way without other tracks: {open})");
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

/// ADR 0059: the same pad pairs as [`local_samples_phase3`], now on F.Cu and B.Cu in one
/// search (vias), then push-and-shove and the phase 3 shove on F.Cu. Each pair alone on
/// the board as it is; zones the new copper touches are poured again (as filling on edit
/// does) on the board before and after, and the check must find no more after.
#[test]
fn local_samples_layers() {
    use crate::{layers, refill, zones};
    use std::time::{Duration, Instant};
    let Some(dir) = std::env::var_os("GUSSET_KICAD_SAMPLES") else { return };
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "kicad_pcb"))
        .collect();
    paths.sort();
    let only = std::env::var("GUSSET_LOCAL_ONLY").ok();
    let lim = Limits::default();
    for p in paths {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        if only.as_ref().is_some_and(|o| !name.contains(o.as_str())) {
            continue;
        }
        let b = Board::read(&std::fs::read_to_string(&p).unwrap(), &lim).unwrap();
        let rules = Rules::for_board(&b);
        let t_board = Instant::now();
        let before = drc::check(&b, &rules, 100_000).len();
        let both: Vec<String> = vec!["F.Cu".into(), "B.Cu".into()];
        let mut pairs = Vec::new();
        for n in b.nets.iter().filter(|n| !n.name.is_empty()) {
            let pads: Vec<_> = b
                .pads
                .iter()
                .filter(|p| b.net_name(&p.net).as_deref() == Some(n.name.as_str()))
                .filter(|p| p.kind != PadKind::Npth && b.on_layer(&p.layers, "F.Cu"))
                .collect();
            if let [a, c, ..] = pads.as_slice() {
                let ls = |p: &crate::board::Pad| -> Vec<usize> {
                    (0..both.len()).filter(|&i| b.on_layer(&p.layers, &both[i])).collect()
                };
                pairs.push((n.name.clone(), a.at, ls(a), c.at, ls(c)));
            }
            if pairs.len() >= 30 {
                break;
            }
        }
        let (mut walked, mut layered, mut vias, mut pushed, mut shoved) = (0, 0, 0, 0, 0);
        let (mut t_walk, mut t_lay, mut t_push) = (Duration::ZERO, Duration::ZERO, Duration::ZERO);
        let mut worst = 0usize;
        let mut refilled_before: std::collections::BTreeMap<Vec<usize>, usize> = std::collections::BTreeMap::new();
        for (net, a, la, c, lc) in &pairs {
            let w = rules.track_width_of(Some(net));
            let grid = (rules.clearance_of(Some(net)).min(w) / 2.0).clamp(0.05, 0.25);
            let t = Instant::now();
            let obs = route::Obstacles::new(&b, &rules, "F.Cu", Some(net), w);
            let walk = route::walkaround(&obs, *a, *c, grid, true).is_some();
            t_walk += t.elapsed();
            if walk {
                walked += 1;
                continue;
            }
            let t = Instant::now();
            let lay = layers::Layered::new(&b, &rules, &both, Some(net), w);
            let r = layers::route(&lay, &rules, *a, la, *c, lc, grid, true, None);
            t_lay += t.elapsed();
            match &r {
                Ok(r) => {
                    let mut nb = b.clone();
                    let routes: Vec<crate::edit::NewRoute> = r
                        .legs
                        .iter()
                        .map(|(l, pts)| {
                            let segs = pts
                                .windows(2)
                                .filter(|x| x[0] != x[1])
                                .map(|x| route::Seg { start: x[0], end: x[1], mid: None })
                                .collect::<Vec<_>>();
                            (segs, w, l.clone(), Some(net.clone()))
                        })
                        .filter(|r| !r.0.is_empty())
                        .collect();
                    let vs: Vec<crate::edit::NewVia> =
                        r.vias.iter().map(|&at| (at, lay.via.size, lay.via.drill, Some(net.clone()))).collect();
                    nb.add_batch(&routes, &vs, &lim).unwrap();
                    let (zs, _) = refill::refill_after_edit(&b, &mut nb, &rules, &lim).unwrap();
                    let base = *refilled_before.entry(zs.clone()).or_insert_with(|| {
                        if zs.is_empty() {
                            return before;
                        }
                        let mut rb = b.clone();
                        rb.set_zone_fills(&zones::fill(&b, &rules, Some(&zs)).fills, &lim).unwrap();
                        drc::check(&rb, &rules, 100_000).len()
                    });
                    let after = drc::check(&nb, &rules, 100_000).len();
                    let worse = after.saturating_sub(base);
                    worst = worst.max(worse);
                    if worse > 0 {
                        let old: Vec<String> = drc::check(&b, &rules, 100_000)
                            .into_iter()
                            .map(|x| x.code.to_string() + &x.message)
                            .collect();
                        let v: Vec<String> = drc::check(&nb, &rules, 100_000)
                            .into_iter()
                            .map(|x| x.code.to_string() + &x.message)
                            .filter(|m| !old.contains(m))
                            .collect();
                        eprintln!(
                            "  {name} {net}: check worse by {worse} ({} refilled zones): {:?} {:?}",
                            zs.len(),
                            v,
                            r
                        );
                    } else {
                        layered += 1;
                        vias += r.vias.len();
                        eprintln!("  {name} {net}: layered, {} vias", r.vias.len());
                        continue;
                    }
                }
                Err(why) => eprintln!("  {name} {net}: no layered route: {why}"),
            }
            let t = Instant::now();
            if let Ok(p) = push::push_reason(&b, &rules, "F.Cu", Some(net), w, *a, *c, grid, &[], Some(before)) {
                let _ = p;
                pushed += 1;
            } else if shove::shove(&b, &rules, "F.Cu", Some(net), w, *a, *c, grid, &[]).is_some_and(|s| {
                let mut nb = b.clone();
                nb.apply_shove(&s, "F.Cu", &lim).unwrap();
                let segs: Vec<route::Seg> =
                    s.path.windows(2).map(|x| route::Seg { start: x[0], end: x[1], mid: None }).collect();
                nb.add_route(&segs, w, "F.Cu", Some(net), &lim).unwrap();
                drc::check(&nb, &rules, 100_000).len() <= before
            }) {
                shoved += 1;
            }
            t_push += t.elapsed();
        }
        eprintln!(
            "{name}: {} pairs, DRC {before} (worst after {worse}); walkaround {walked} ({t_walk:?}); layered +{layered} \
             ({vias} vias, {t_lay:?}); push +{pushed}, shove +{shoved} ({t_push:?}); total {} in {:?}",
            pairs.len(),
            walked + layered + pushed + shoved,
            t_board.elapsed(),
            worse = before + worst,
        );
        assert_eq!(worst, 0, "{name}: a layered route made the check worse");
    }
}

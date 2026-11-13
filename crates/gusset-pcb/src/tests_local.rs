#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::print_stderr)]
//! A local check over KiCad sample boards the user has (never in the repository: KiCad's
//! demo boards are CC-BY-SA). Runs only with `GUSSET_KICAD_SAMPLES=<folder of .kicad_pcb>`;
//! use a release build for the larger boards.

use crate::board::{Board, Limits, PadKind};
use crate::rules::Rules;
use crate::{drc, gerber, route, shove};

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
        let (mut walked, mut shoved, mut clean) = (0, 0, 0);
        let t = std::time::Instant::now();
        for (net, a, c) in &pairs {
            let w = rules.track_width_of(Some(net));
            let grid = (rules.clearance_of(Some(net)).min(w) / 2.0).clamp(0.05, 0.25);
            let obs = route::Obstacles::new(&b, &rules, "F.Cu", Some(net), w);
            if route::walkaround(&obs, *a, *c, grid, true).is_some() {
                walked += 1;
                continue;
            }
            if let Some(s) = shove::shove(&b, &rules, "F.Cu", Some(net), w, *a, *c, grid, &[]) {
                shoved += 1;
                let mut nb = b.clone();
                nb.apply_shove(&s, "F.Cu", &Limits::default()).unwrap();
                let segs: Vec<route::Seg> =
                    s.path.windows(2).map(|x| route::Seg { start: x[0], end: x[1], mid: None }).collect();
                nb.add_route(&segs, w, "F.Cu", Some(net), &Limits::default()).unwrap();
                if drc::check(&nb, &rules, 100_000).len() <= before {
                    clean += 1;
                }
            }
        }
        eprintln!(
            "{name}: {} pairs, {walked} walked around, {shoved} shoved ({clean} DRC no worse), {:?}",
            pairs.len(),
            t.elapsed()
        );
    }
}

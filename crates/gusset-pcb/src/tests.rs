#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

use crate::board::{Board, Limits, PadKind, PadShape, Shape};
use crate::sexpr::{self, Node};
use crate::{board3d, check, drill, geom, gerber};

/// A small two-layer board written for these tests (KiCad 8 layout): a 50 × 30 mm outline
/// with a round cut-out, an SMD resistor turned 90°, a two-pin header (one oval hole), a
/// mounting hole, tracks (one an arc), a via and a filled zone. It also carries nodes this
/// reader does not know, which must survive a round trip.
pub(crate) const BOARD: &str = r#"(kicad_pcb (version 20240108) (generator "pcbnew") (generator_version "8.0")
  (general (thickness 1.6) (legacy_teardrops no))
  (paper "A4")
  (layers
    (0 "F.Cu" signal)
    (31 "B.Cu" signal)
    (36 "B.SilkS" user "B.Silkscreen")
    (37 "F.SilkS" user "F.Silkscreen")
    (38 "B.Mask" user)
    (39 "F.Mask" user)
    (34 "B.Paste" user)
    (35 "F.Paste" user)
    (44 "Edge.Cuts" user)
    (49 "F.Fab" user)
  )
  (setup
    (stackup
      (layer "F.SilkS" (type "Top Silk Screen"))
      (layer "F.Cu" (type "copper") (thickness 0.035))
      (layer "dielectric 1" (type "core") (thickness 1.53) (material "FR4") (epsilon_r 4.5) (loss_tangent 0.02))
      (layer "B.Cu" (type "copper") (thickness 0.035))
      (copper_finish "None")
    )
    (pad_to_mask_clearance 0.05)
    (future_setting "kept \"as is\"" (nested 1 2 3))
    (pcbplotparams (layerselection 0x00010fc_ffffffff) (outputdirectory ""))
  )
  (net 0 "")
  (net 1 "GND")
  (net 2 "VCC")
  (footprint "Resistor_SMD:R_0603_1608Metric" (layer "F.Cu") (uuid "a1") (at 110 110 90)
    (property "Reference" "R1" (at 0 -1.43 90) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))))
    (property "Value" "10k" (at 0 1.43 90) (layer "F.Fab"))
    (attr smd)
    (fp_line (start -0.8 -0.4) (end 0.8 -0.4) (stroke (width 0.1) (type solid)) (layer "F.Fab"))
    (fp_line (start 0.8 -0.4) (end 0.8 0.4) (stroke (width 0.1) (type solid)) (layer "F.Fab"))
    (fp_line (start 0.8 0.4) (end -0.8 0.4) (stroke (width 0.1) (type solid)) (layer "F.Fab"))
    (fp_line (start -0.8 0.4) (end -0.8 -0.4) (stroke (width 0.1) (type solid)) (layer "F.Fab"))
    (fp_line (start -0.24 -0.5) (end 0.24 -0.5) (stroke (width 0.12) (type solid)) (layer "F.SilkS"))
    (pad "1" smd roundrect (at -0.825 0 90) (size 0.8 0.95) (layers "F.Cu" "F.Paste" "F.Mask") (roundrect_rratio 0.25) (net 2 "VCC"))
    (pad "2" smd roundrect (at 0.825 0 90) (size 0.8 0.95) (layers "F.Cu" "F.Paste" "F.Mask") (roundrect_rratio 0.25) (net 1 "GND"))
    (model "${KICAD8_3DMODEL_DIR}/Resistor_SMD.3dshapes/R_0603_1608Metric.wrl" (offset (xyz 0 0 0)) (scale (xyz 1 1 1)) (rotate (xyz 0 0 0)))
  )
  (footprint "Connector:Header_1x02" (layer "F.Cu") (at 120 120)
    (property "Reference" "J1" (at 0 -2) (layer "F.SilkS"))
    (attr through_hole)
    (pad "1" thru_hole rect (at 0 0) (size 1.7 1.7) (drill 1) (layers "*.Cu" "*.Mask") (net 2 "VCC"))
    (pad "2" thru_hole oval (at 2.54 0) (size 1.7 2.5) (drill oval 1 1.8) (layers "*.Cu" "*.Mask") (net 1 "GND"))
  )
  (footprint "MountingHole:MountingHole_3.2mm" (layer "F.Cu") (at 105 125)
    (property "Reference" "H1" (at 0 -4) (layer "F.SilkS"))
    (attr exclude_from_pos_files exclude_from_bom)
    (pad "" np_thru_hole circle (at 0 0) (size 3.2 3.2) (drill 3.2) (layers "*.Cu" "*.Mask"))
  )
  (gr_line (start 100 100) (end 150 100) (stroke (width 0.1) (type default)) (layer "Edge.Cuts"))
  (gr_line (start 150 100) (end 150 130) (stroke (width 0.1) (type default)) (layer "Edge.Cuts"))
  (gr_line (start 100 130) (end 150 130) (stroke (width 0.1) (type default)) (layer "Edge.Cuts"))
  (gr_line (start 100 130) (end 100 100) (stroke (width 0.1) (type default)) (layer "Edge.Cuts"))
  (gr_circle (center 140 110) (end 142 110) (stroke (width 0.1) (type default)) (fill none) (layer "Edge.Cuts"))
  (gr_text "Gusset" (at 130 125) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))))
  (segment (start 110 109.175) (end 120 109.175) (width 0.25) (layer "F.Cu") (net 2))
  (segment (start 120 109.175) (end 120 120) (width 0.25) (layer "F.Cu") (net 2))
  (arc (start 110 110.825) (mid 116 116) (end 122.54 120) (width 0.3) (layer "F.Cu") (net 1))
  (via (at 130 115) (size 0.8) (drill 0.4) (layers "F.Cu" "B.Cu") (net 1))
  (zone (net 1) (net_name "GND") (layer "B.Cu") (hatch edge 0.5) (connect_pads (clearance 0.3)) (min_thickness 0.25)
    (fill yes (thermal_gap 0.5) (thermal_bridge_width 0.5))
    (polygon (pts (xy 101 101) (xy 149 101) (xy 149 129) (xy 101 129)))
    (filled_polygon (layer "B.Cu") (pts (xy 101 101) (xy 149 101) (xy 149 129) (xy 101 129)))
  )
  (future_item (with "unknown" (structure)) bare-symbol 1.000)
)
"#;

fn board() -> Board {
    Board::read(BOARD, &Limits::default()).unwrap()
}

#[test]
fn sexpr_reads_strings_escapes_and_atoms_verbatim() {
    let t = sexpr::parse(r#"(a "x \"y\" \\ z" 1.000 -0 sym (b))"#, &sexpr::Limits::default()).unwrap();
    assert_eq!(t.head(), Some("a"));
    assert_eq!(t.arg(0), Some("x \"y\" \\ z"));
    // Numbers keep their exact text.
    assert_eq!(t.arg(1), Some("1.000"));
    assert_eq!(t.arg(2), Some("-0"));
    let again = sexpr::parse(&sexpr::write(&t), &sexpr::Limits::default()).unwrap();
    assert_eq!(again, t);
}

#[test]
fn sexpr_refuses_malformed_and_oversized_input() {
    let l = sexpr::Limits::default();
    for bad in ["", "(a", "(a))", "a", "(a) (b)", "(a \"open)", ")"] {
        assert!(sexpr::parse(bad, &l).is_err(), "{bad:?}");
    }
    let deep = "(".repeat(200) + &")".repeat(200);
    assert!(matches!(sexpr::parse(&deep, &l), Err(sexpr::Error::TooDeep { .. })));
    let small = sexpr::Limits { max_bytes: 10, ..l };
    assert_eq!(sexpr::parse("(abcdefghijkl)", &small), Err(sexpr::Error::TooLarge));
    let few = sexpr::Limits { max_nodes: 3, ..l };
    assert_eq!(sexpr::parse("(a b c d)", &few), Err(sexpr::Error::TooManyNodes));
    // A deep but legal file still writes without recursion.
    let ok = "(".repeat(100) + &")".repeat(100);
    let t = sexpr::parse(&ok, &l).unwrap();
    assert_eq!(sexpr::parse(&sexpr::write(&t), &l).unwrap(), t);
}

#[test]
fn number_formatting_matches_kicad() {
    assert_eq!(sexpr::fmt_num(1.0), "1");
    assert_eq!(sexpr::fmt_num(0.1 + 0.2), "0.3");
    assert_eq!(sexpr::fmt_num(-0.0000001), "0");
    assert_eq!(sexpr::fmt_num(-12.5), "-12.5");
}

#[test]
fn board_round_trip_keeps_the_tree() {
    let b = board();
    let text = b.write();
    let again = Board::read(&text, &Limits::default()).unwrap();
    assert_eq!(again.tree, b.tree, "the tree changed in a round trip");
    // The unknown nodes are there, verbatim.
    assert!(text.contains("(future_setting \"kept \\\"as is\\\"\" (nested 1 2 3))"), "{text}");
    assert!(b.tree.child("future_item").is_some_and(|n| n.arg(1) == Some("bare-symbol") && n.arg(2) == Some("1.000")));
    // Writing is stable.
    assert_eq!(again.write(), text);
}

#[test]
fn board_model_reads_layers_stackup_nets_and_items() {
    let b = board();
    assert_eq!(b.version, Some(20240108));
    assert_eq!(b.copper_layers(), vec!["F.Cu", "B.Cu"]);
    assert_eq!(b.thickness, 1.6);
    assert_eq!(b.stackup.len(), 4);
    assert_eq!(b.stackup[2].material.as_deref(), Some("FR4"));
    assert_eq!(b.mask_margin, 0.05);
    assert_eq!(b.nets.len(), 3);
    assert_eq!(b.footprints.len(), 3);
    assert_eq!(b.pads.len(), 5);
    assert_eq!(b.tracks.len(), 3);
    assert_eq!(b.vias.len(), 1);
    assert_eq!(b.zones.len(), 1);
    assert_eq!(b.zones[0].fills.len(), 1);
    assert_eq!(b.texts.len(), 1);
    let r1 = &b.footprints[0];
    assert_eq!((r1.reference.as_str(), r1.value.as_str()), ("R1", "10k"));
    assert_eq!(r1.models.len(), 1);
    assert!(r1.models[0].path.ends_with("R_0603_1608Metric.wrl"));
    // Pad 1 at (-0.825, 0) in a footprint turned 90° counter-clockwise on screen (Y down)
    // lands above the centre: (110, 110.825).
    let p1 = &b.pads[r1.pads[0]];
    assert!((p1.at[0] - 110.0).abs() < 1e-9 && (p1.at[1] - 110.825).abs() < 1e-9, "{:?}", p1.at);
    assert!(matches!(p1.shape, PadShape::RoundRect { ratio } if ratio == 0.25));
    assert_eq!(b.net_name(&p1.net).as_deref(), Some("VCC"));
    let j1 = &b.footprints[1];
    let p2 = &b.pads[j1.pads[1]];
    assert_eq!(p2.kind, PadKind::Tht);
    assert_eq!(p2.drill.unwrap().size, [1.0, 1.8]);
    assert!(b.on_layer(&p2.layers, "B.Cu") && b.on_layer(&p2.layers, "F.Mask") && !b.on_layer(&p2.layers, "F.Paste"));
    assert_eq!(b.pads[b.footprints[2].pads[0]].kind, PadKind::Npth);
    // The outline: four lines and the round cut-out.
    assert_eq!(b.outline().count(), 5);
    assert!(b.tracks[2].mid.is_some());
}

#[test]
fn a_footprint_file_reads_as_one_footprint() {
    let fp = r#"(footprint "R_0603" (version 20240108) (generator "pcbnew") (layer "F.Cu")
      (attr smd)
      (pad "1" smd rect (at -0.8 0) (size 0.8 0.9) (layers "F.Cu" "F.Paste" "F.Mask"))
      (pad "2" smd rect (at 0.8 0) (size 0.8 0.9) (layers "F.Cu" "F.Paste" "F.Mask"))
      (model "R_0603.step"))"#;
    let b = Board::read(fp, &Limits::default()).unwrap();
    assert_eq!(b.footprints.len(), 1);
    assert_eq!(b.pads.len(), 2);
    assert_eq!(b.footprints[0].models[0].path, "R_0603.step");
    assert_eq!(b.copper_layers(), vec!["F.Cu", "B.Cu"]);
    assert_eq!(Board::read(&b.write(), &Limits::default()).unwrap().tree, b.tree);
    assert!(matches!(Board::read("(sym_lib)", &Limits::default()), Err(crate::board::Error::NotABoard(_))));
}

#[test]
fn kicad5_layouts_read() {
    // `module`, `(width)` without `stroke`, arcs as centre + angle, `F&B.Cu`.
    let src = r#"(kicad_pcb (version 20171130) (host pcbnew 5.1.9)
      (general (thickness 1.6))
      (layers (0 F.Cu signal) (31 B.Cu signal) (44 Edge.Cuts user))
      (net 0 "") (net 1 GND)
      (module Pin (layer F.Cu) (at 10 10)
        (fp_text reference P1 (at 0 2) (layer F.SilkS))
        (pad 1 thru_hole circle (at 0 0) (size 2 2) (drill 1) (layers *.Cu *.Mask) (net 1 GND)))
      (gr_arc (start 0 0) (end 5 0) (angle 90) (layer Edge.Cuts) (width 0.1)))"#;
    let b = Board::read(src, &Limits::default()).unwrap();
    assert_eq!(b.footprints[0].reference, "P1");
    assert_eq!(b.footprints[0].attrs, vec!["through_hole"]);
    let Shape::Arc { start, end, .. } = b.graphics.iter().find(|g| g.layer == "Edge.Cuts").unwrap().shape else {
        panic!("not an arc")
    };
    assert_eq!(start, [5.0, 0.0]);
    assert!((end[0]).abs() < 1e-9 && (end[1] - 5.0).abs() < 1e-9, "{end:?}");
    assert_eq!(b.graphics.iter().find(|g| g.layer == "Edge.Cuts").unwrap().width, 0.1);
}

#[test]
fn nets_named_on_items_only_are_listed() {
    // Newer files have no net table: pads and tracks carry `(net "NAME")`.
    let src = r#"(kicad_pcb (version 20260206)
      (footprint "X" (layer "F.Cu") (at 0 0)
        (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu") (net "VCC"))
        (pad "2" smd rect (at 2 0) (size 1 1) (layers "F.Cu") (net "GND"))
        (pad "" smd rect (at 4 0) (size 1 1) (layers "F.Paste")))
      (segment (start 0 0) (end 2 0) (width 0.2) (layer "F.Cu") (net "VCC")))"#;
    let b = Board::read(src, &Limits::default()).unwrap();
    let names: Vec<&str> = b.nets.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(names, vec!["VCC", "GND"]);
    assert_eq!(b.net_name(&b.tracks[0].net).as_deref(), Some("VCC"));
    // The paste-only pad (no copper, no net) is an aperture pad, not a problem.
    let codes: Vec<&str> = check::check(&b, 10).iter().map(|i| i.code).collect();
    assert_eq!(codes, vec!["no_outline"]);
}

#[test]
fn moving_a_footprint_moves_its_pads_and_writes_back() {
    let mut b = board();
    b.move_footprint(1, [130.0, 120.0], 90.0, &Limits::default()).unwrap();
    let j1 = &b.footprints[1];
    assert_eq!(j1.at, [130.0, 120.0]);
    // Pad 2 at local (2.54, 0) turned 90° (counter-clockwise on screen): (130, 117.46).
    let p2 = &b.pads[j1.pads[1]];
    assert!((p2.at[0] - 130.0).abs() < 1e-9 && (p2.at[1] - 117.46).abs() < 1e-9, "{:?}", p2.at);
    assert_eq!(p2.angle, 90.0);
    let again = Board::read(&b.write(), &Limits::default()).unwrap();
    assert_eq!(again.footprints[1].at, [130.0, 120.0]);
    // Everything else is unchanged.
    assert_eq!(again.tracks, board().tracks);
}

#[test]
fn outline_chains_into_closed_loops() {
    let b = board();
    let (loops, gaps) = geom::outline_loops(&b, check::OUTLINE_TOL);
    assert!(gaps.is_empty());
    assert_eq!(loops.len(), 2);
    let areas: Vec<f64> = loops.iter().map(|l| geom::area(&l.polygon(0.001)).abs()).collect();
    assert!(areas.iter().any(|a| (a - 1500.0).abs() < 1e-6), "{areas:?}");
    assert!(areas.iter().any(|a| (a - std::f64::consts::PI * 4.0).abs() < 0.01), "{areas:?}");
}

#[test]
fn sanity_check_passes_and_finds_problems() {
    assert_eq!(check::check(&board(), 50), vec![]);
    // An open outline and a pad on no copper layer.
    let broken = BOARD
        .replace("(gr_line (start 100 130) (end 100 100)", "(gr_line (start 100 130) (end 100 101)")
        .replace(r#"(layers "*.Cu" "*.Mask") (net 2 "VCC"))"#, r#"(layers "F.Mask") (net 7 "X"))"#);
    let b = Board::read(&broken, &Limits::default()).unwrap();
    let codes: Vec<&str> = check::check(&b, 50).iter().map(|i| i.code).collect();
    assert!(codes.contains(&"outline_open"), "{codes:?}");
    assert!(codes.contains(&"pad_no_copper"), "{codes:?}");
    assert!(codes.contains(&"unknown_net"), "{codes:?}");
}

#[test]
fn gerber_copper_layer_has_pads_tracks_and_vias() {
    let b = board();
    let f = gerber::write_layer(&b, "F.Cu").unwrap();
    assert!(f.contains("%TF.FileFunction,Copper,L1,Top*%"), "{f}");
    assert!(f.contains("%FSLAX46Y46*%") && f.contains("%MOMM*%") && f.ends_with("M02*\n"), "{f}");
    // J1 pad 1: a 1.7 mm square flashed at (120, -120).
    assert!(f.contains("%ADD") && f.contains("R,1.7X1.7*%"), "{f}");
    assert!(f.contains("X120000000Y-120000000D03*"), "{f}");
    // The oval pad as an obround, the via as a 0.8 mm circle.
    assert!(f.contains("O,1.7X2.5*%") && f.contains("X130000000Y-115000000D03*"), "{f}");
    // Tracks: 0.25 mm draws; the arc with an I/J centre offset.
    assert!(f.contains("X110000000Y-109175000D02*") && f.contains("X120000000Y-109175000D01*"), "{f}");
    assert!(f.contains("G02*") || f.contains("G03*"), "{f}");
    // The roundrect pads turned 90° are regions.
    assert!(f.contains("G36*") && f.contains("G37*"), "{f}");
    // X2 aperture functions.
    assert!(f.contains("%TA.AperFunction,SMDPad,CuDef*%") && f.contains("%TA.AperFunction,ViaPad*%"), "{f}");
    let back = gerber::write_layer(&b, "B.Cu").unwrap();
    assert!(back.contains("%TF.FileFunction,Copper,L2,Bot*%"), "{back}");
    // The zone fill and the through-hole pads, not the SMD pads.
    assert!(back.contains("G36*"), "{back}");
    assert!(!back.contains("SMDPad"), "{back}");
}

#[test]
fn gerber_mask_paste_silk_and_outline() {
    let b = board();
    let mask = gerber::write_layer(&b, "F.Mask").unwrap();
    // Grown by the board's 0.05 mm: J1's square pad opening is 1.8 mm.
    assert!(mask.contains("%TF.FileFunction,Soldermask,Top*%") && mask.contains("R,1.8X1.8*%"), "{mask}");
    let paste = gerber::write_layer(&b, "F.Paste").unwrap();
    assert!(paste.contains("%TF.FileFunction,Paste,Top*%") && paste.contains("G36*"), "{paste}");
    assert!(!paste.contains("R,1.7X1.7"), "through-hole pads have no paste: {paste}");
    let silk = gerber::write_layer(&b, "F.SilkS").unwrap();
    assert!(silk.contains("%TF.FileFunction,Legend,Top*%") && silk.contains("C,0.12*%"), "{silk}");
    let edge = gerber::write_layer(&b, "Edge.Cuts").unwrap();
    assert!(edge.contains("%TF.FileFunction,Profile,NP*%") && edge.contains("X150000000Y-130000000D01*"), "{edge}");
    let all = gerber::write_all(&b).unwrap();
    assert_eq!(all.len(), 9);
    assert_eq!(all[0].suffix, "F_Cu");
    // Deterministic output.
    assert_eq!(gerber::write_all(&b).unwrap(), all);
}

#[test]
fn gerber_refuses_coordinates_out_of_range() {
    let src = BOARD.replace("(at 130 115) (size 0.8)", "(at 9999.5 115) (size 0.8)");
    let b = Board::read(&src, &Limits::default()).unwrap();
    assert_eq!(gerber::write_layer(&b, "F.Cu"), Err(gerber::Error::OutOfRange));
}

#[test]
fn excellon_plated_and_non_plated() {
    let b = board();
    let (plated, npth) = drill::holes(&b);
    assert_eq!(plated.len(), 3);
    assert_eq!(npth.len(), 1);
    let p = drill::write(&b, &plated, true);
    assert!(p.starts_with("M48\n") && p.ends_with("M30\n"), "{p}");
    assert!(p.contains("TF.FileFunction,Plated,1,2,PTH"), "{p}");
    assert!(p.contains("METRIC") && p.contains("T1C0.400") && p.contains("T2C1.000"), "{p}");
    assert!(p.contains("X130.0000Y-115.0000"), "{p}");
    // The oval hole as a slot along the pad's Y: (122.54, 119.6) to (122.54, 120.4).
    assert!(p.contains("X122.5400Y-119.6000G85X122.5400Y-120.4000"), "{p}");
    let n = drill::write(&b, &npth, false);
    assert!(n.contains("NonPlated,1,2,NPTH") && n.contains("T1C3.200") && n.contains("X105.0000Y-125.0000"), "{n}");
}

#[test]
fn board_profile_has_the_cutout_and_holes() {
    let b = board();
    let opts = board3d::Options::default();
    let (p, notes) = board3d::board_profile(&b, &opts).unwrap();
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(p.regions.len(), 1);
    // The round cut-out, two header holes (one a slot), the mounting hole; not the via.
    assert_eq!(p.regions[0].holes.len(), 4);
    let with_vias = board3d::Options { vias: true, ..opts };
    assert_eq!(board3d::board_profile(&b, &with_vias).unwrap().0.regions[0].holes.len(), 5);
    // Component boxes: R1 from its fabrication drawing (1.6 × 0.8 turned 90°).
    let bx = board3d::component_box(&b, 0).unwrap();
    let xs: Vec<f64> = bx.iter().map(|p| p[0]).collect();
    let ys: Vec<f64> = bx.iter().map(|p| p[1]).collect();
    let w = xs.iter().cloned().fold(f64::MIN, f64::max) - xs.iter().cloned().fold(f64::MAX, f64::min);
    let h = ys.iter().cloned().fold(f64::MIN, f64::max) - ys.iter().cloned().fold(f64::MAX, f64::min);
    assert!((w - 0.8).abs() < 1e-9 && (h - 1.6).abs() < 1e-9, "{w} {h}");
}

#[test]
fn board_builds_with_the_mock_kernel() {
    // The mock kernel cannot cut holes: the board comes in without them, with a note, and
    // the component boxes stand on its top.
    let k = gusset_kernel::mock::MockKernel;
    let b = board();
    let (bodies, notes) = board3d::build(&b, &k, &board3d::Options::default()).unwrap();
    assert_eq!(bodies[0].name, "Board");
    assert!(notes.iter().any(|n| n.contains("holes could not be cut")), "{notes:?}");
    let bb = gusset_kernel::Kernel::bbox(&k, &bodies[0].shape).unwrap();
    assert!((bb.min[0] - 100.0).abs() < 1e-9 && (bb.max[1] + 100.0).abs() < 1e-9, "{bb:?}");
    assert!((bb.max[2] - 1.6).abs() < 1e-9 && bb.min[2].abs() < 1e-9, "{bb:?}");
    let names: Vec<&str> = bodies.iter().map(|b| b.name.as_str()).collect();
    // The mounting hole has no part to box.
    assert_eq!(names, vec!["Board", "R1", "J1"]);
    let r1 = gusset_kernel::Kernel::bbox(&k, &bodies[1].shape).unwrap();
    assert!((r1.min[2] - 1.6).abs() < 1e-9 && (r1.max[2] - 3.6).abs() < 1e-9, "{r1:?}");
}

#[test]
fn model_placement_steps() {
    let b = board();
    let fp = &b.footprints[0];
    let steps = board3d::model_steps(&b, fp, &fp.models[0]);
    // Rotation by the footprint's 90° and the move to (110, -110) on the top face.
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[1], gusset_kernel::Transform::Translate([110.0, -110.0, 1.6]));
}

#[test]
fn hostile_inputs_do_not_panic() {
    let l = Limits::default();
    for src in [
        "(kicad_pcb (footprint))",
        "(kicad_pcb (footprint \"x\" (pad)))",
        "(kicad_pcb (footprint \"x\" (pad 1 smd rect (at nan inf) (size -1 1e300))))",
        "(kicad_pcb (segment (start 1) (end)))",
        "(kicad_pcb (zone (polygon (pts (xy 1)))))",
        "(kicad_pcb (gr_arc (start 0 0) (mid 0 0) (end 0 0) (layer Edge.Cuts)))",
        "(kicad_pcb (gr_poly (pts) (layer Edge.Cuts)))",
        "(kicad_pcb (via (at 0 0) (size 1) (drill 0.5) (layers)))",
        "(kicad_pcb (net) (net x) (layers (x)))",
        "(kicad_pcb (gr_circle (center 0 0) (end 0 0) (layer Edge.Cuts)))",
    ] {
        let b = Board::read(src, &l).unwrap();
        let _ = check::check(&b, 10);
        let _ = gerber::write_all(&b);
        let (p, n) = drill::holes(&b);
        let _ = drill::write(&b, &p, true);
        let _ = drill::write(&b, &n, false);
        let _ = board3d::board_profile(&b, &board3d::Options::default());
        let _ = b.write();
    }
    let few = Limits { max_items: 2, ..l };
    let many = "(kicad_pcb (via (at 0 0)) (via (at 1 0)) (via (at 2 0)))";
    assert_eq!(Board::read(many, &few).unwrap_err(), crate::board::Error::TooMany("vias"));
    let n: Node = sexpr::parse("(kicad_pcb)", &sexpr::Limits::default()).unwrap();
    assert_eq!(n.count(), 2);
}

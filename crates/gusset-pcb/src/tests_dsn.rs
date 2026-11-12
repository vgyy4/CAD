#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

use crate::board::{Board, Limits};
use crate::rules::Rules;
use crate::specctra::{self, Node};
use std::collections::BTreeMap;

/// Two resistors (R1 front, R2 on the back, turned), a header with a through-hole pad,
/// a mounting hole, routed tracks and a via.
const BOARD: &str = r#"(kicad_pcb (version 20240108) (generator "test")
  (general (thickness 1.6))
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (net 0 "") (net 1 "GND") (net 2 "A B")
  (gr_rect (start 0 0) (end 40 20) (stroke (width 0.05) (type default)) (fill none) (layer "Edge.Cuts"))
  (footprint "R" (layer "F.Cu") (at 5 10)
    (property "Reference" "R1" (at 0 0) (layer "F.Fab"))
    (property "Value" "10k" (at 0 0) (layer "F.Fab"))
    (pad "1" smd rect (at -0.825 0) (size 0.8 0.95) (layers "F.Cu") (net 1 "GND"))
    (pad "2" smd roundrect (at 0.825 0) (size 0.8 0.95) (layers "F.Cu") (roundrect_rratio 0.25) (net 2 "A B")))
  (footprint "R" (layer "B.Cu") (at 30 10 90)
    (property "Reference" "R2" (at 0 0) (layer "B.Fab"))
    (pad "1" smd rect (at -0.825 0 90) (size 0.8 0.95) (layers "B.Cu") (net 2 "A B"))
    (pad "2" smd rect (at 0.825 0 90) (size 0.8 0.95) (layers "B.Cu") (net 1 "GND")))
  (footprint "H" (layer "F.Cu") (at 20 4)
    (property "Reference" "J1" (at 0 0) (layer "F.Fab"))
    (pad "1" thru_hole circle (at 0 0) (size 1.7 1.7) (drill 1) (layers "*.Cu") (net 1 "GND"))
    (pad "" np_thru_hole circle (at 0 4) (size 2 2) (drill 2) (layers "*.Cu")))
  (segment (start 5.825 10) (end 18 10) (width 0.25) (layer "F.Cu") (net 2))
  (segment (start 18 10) (end 18 12) (width 0.25) (layer "F.Cu") (net 2))
  (via (at 18 12) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 2))
  (segment (start 18 12) (end 30 12) (width 0.25) (layer "B.Cu") (net 2))
  (segment (start 30 12) (end 30 10.825) (width 0.25) (layer "B.Cu") (net 2))
)"#;

fn board() -> Board {
    Board::read(BOARD, &Limits::default()).unwrap()
}

fn sl() -> specctra::Limits {
    specctra::Limits::default()
}

fn find<'a>(n: &'a Node, head: &str) -> Vec<&'a Node> {
    match n {
        Node::List(v) => {
            let mut out = Vec::new();
            if matches!(v.first(), Some(Node::Atom(a)) if a == head) {
                out.push(n);
            }
            for c in v {
                out.extend(find(c, head));
            }
            out
        }
        Node::Atom(_) => Vec::new(),
    }
}

fn atoms(n: &Node) -> Vec<String> {
    match n {
        Node::List(v) => v.iter().filter_map(|x| if let Node::Atom(a) = x { Some(a.clone()) } else { None }).collect(),
        Node::Atom(a) => vec![a.clone()],
    }
}

#[test]
fn the_dsn_carries_layers_outline_parts_nets_and_wiring() {
    let b = board();
    let dsn = specctra::write_dsn(&b, &Rules::for_board(&b), "t.dsn");
    let root = specctra::parse(&dsn, &sl()).unwrap();
    let layers: Vec<String> = find(&root, "layer").iter().map(|l| atoms(l)[1].clone()).collect();
    assert_eq!(layers, ["F.Cu", "B.Cu"]);
    let boundary = &find(&root, "boundary")[0];
    let path = atoms(find(boundary, "path")[0]);
    // A closed rectangle, Y up: (0,0) (40,0) (40,-20) (0,-20) and back, in µm.
    assert_eq!(path.len(), 3 + 10, "{path:?}");
    assert!(path.contains(&"40000".to_string()) && path.contains(&"-20000".to_string()));
    let places: Vec<Vec<String>> = find(&root, "place").iter().map(|p| atoms(p)).collect();
    assert_eq!(places.len(), 3);
    assert_eq!(places[0][1..5], ["R1", "5000", "-10000", "front"]);
    // Every pin with a net is listed; the space in "A B" is quoted, the mounting hole is
    // a keep-out, not a pin.
    let nets: BTreeMap<String, Vec<String>> = find(&root, "net")
        .iter()
        .filter(|n| !find(n, "pins").is_empty())
        .map(|n| (atoms(n)[1].clone(), atoms(find(n, "pins")[0])[1..].to_vec()))
        .collect();
    assert_eq!(nets["GND"], ["R1-1", "R2-2", "J1-1"]);
    assert_eq!(nets["A B"], ["R1-2", "R2-1"]);
    assert!(dsn.contains("(net \"A B\""));
    assert!(dsn.contains("(keepout \"\" (circle signal 2000 20000 -8000))"), "{dsn}");
    // R2's pins sit on B.Cu, where the board has them.
    let r2 = find(&root, "image").into_iter().find(|i| atoms(i)[1] == "R2_1").unwrap();
    let pins: Vec<Vec<String>> = find(r2, "pin").iter().map(|p| atoms(p)).collect();
    assert_eq!(pins[0][2..], ["1", "0", "-825"]);
    let stack = pins[0][1].clone();
    let ps = find(&root, "padstack").into_iter().find(|p| atoms(p)[1] == stack).unwrap();
    assert_eq!(atoms(find(ps, "rect")[0])[1], "B.Cu");
    // The through-hole pad has a shape on each layer; the via padstack is named by size.
    assert!(dsn.contains("(via Via[0-1]_600:300_um)") || dsn.contains("(via \"Via[0-1]_600:300_um\")"), "{dsn}");
    let wires = find(&root, "wire");
    assert_eq!(wires.len(), 4);
    assert_eq!(atoms(find(wires[0], "path")[0])[1..], ["F.Cu", "250", "5825", "-10000", "18000", "-10000"]);
}

/// A session file as an autorouter writes it, from the DSN's wiring (the routes of each
/// net), at another resolution (mil / 10).
fn session_from(dsn: &str) -> String {
    let root = specctra::parse(dsn, &sl()).unwrap();
    let mut by_net: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mil = |v: &str| format!("{:.4}", v.parse::<f64>().unwrap() / 25.4 * 10.0);
    for w in find(&root, "wire") {
        let p = atoms(find(w, "path")[0]);
        let net = atoms(find(w, "net")[0])[1].clone();
        let coords: Vec<String> = p[2..].iter().map(|v| mil(v)).collect();
        by_net.entry(net).or_default().push(format!("(wire (path {} {} {}))", p[1], mil(&p[2]), coords[1..].join(" ")));
    }
    for v in find(&root, "via").into_iter().filter(|v| atoms(v).len() == 4) {
        let a = atoms(v);
        let net = atoms(find(v, "net")[0])[1].clone();
        by_net.entry(net).or_default().push(format!("(via \"{}\" {} {})", a[1], mil(&a[2]), mil(&a[3])));
    }
    let nets: Vec<String> = by_net.iter().map(|(n, items)| format!("(net \"{n}\" {})", items.join(" "))).collect();
    format!(
        "(session \"t.ses\" (base_design \"t.dsn\") (placement (resolution mil 10)) (was_is)
  (routes (resolution mil 10) (parser (host_cad \"x\") (host_version \"1\"))
    (library_out (padstack \"Via[0-1]_600:300_um\" (shape (circle F.Cu 236.2205 0 0)) (shape (circle B.Cu 236.2205 0 0)) (attach off)))
    (network_out {})))",
        nets.join(" ")
    )
}

#[test]
fn routes_come_back_from_a_session() {
    let b = board();
    let rules = Rules::for_board(&b);
    let dsn = specctra::write_dsn(&b, &rules, "t.dsn");
    let ses_text = session_from(&dsn);
    let ses = specctra::read_ses(&ses_text, &sl()).unwrap();
    assert_eq!(ses.nets.len(), 1);
    assert_eq!((ses.nets[0].wires.len(), ses.nets[0].vias.len()), (4, 1));
    // Applied to the board, replacing its routes: the same tracks and via.
    let mut back = b.clone();
    let (nt, nv) = back.apply_session(&ses, &rules, true, &Limits::default()).unwrap();
    assert_eq!((nt, nv), (4, 1));
    assert_eq!(back.tracks.len(), b.tracks.len());
    for (x, y) in b.tracks.iter().zip(&back.tracks) {
        for (p, q) in [(x.start, y.start), (x.end, y.end)] {
            assert!((p[0] - q[0]).abs() < 1e-4 && (p[1] - q[1]).abs() < 1e-4, "{p:?} {q:?}");
        }
        assert_eq!((x.layer.as_str(), back.net_name(&y.net)), (y.layer.as_str(), b.net_name(&x.net)));
        assert!((x.width - y.width).abs() < 1e-4);
    }
    let v = &back.vias[0];
    assert!((v.size - 0.6).abs() < 1e-4 && (v.drill - 0.3).abs() < 1e-9 && (v.at[1] - 12.0).abs() < 1e-4, "{v:?}");
    assert_eq!(crate::ratsnest::ratsnest(&back).unrouted(), crate::ratsnest::ratsnest(&b).unrouted());
    // Kept routes: the session's are added to them.
    let mut both = b.clone();
    both.apply_session(&ses, &rules, false, &Limits::default()).unwrap();
    assert_eq!(both.tracks.len(), 2 * b.tracks.len());
    // A layer the board lacks is refused.
    let bad = ses_text.replace("(path B.Cu", "(path In1.Cu");
    let ses = specctra::read_ses(&bad, &sl()).unwrap();
    assert!(b.clone().apply_session(&ses, &rules, true, &Limits::default()).is_err());
}

#[test]
fn hostile_session_files_are_refused() {
    let l = sl();
    for bad in ["", "(", "(session", "(session))", "(pcb x)", "(session \"open)", "(session x) y", ")"] {
        assert!(specctra::read_ses(bad, &l).is_err(), "{bad:?}");
    }
    let deep = format!("(session {}{}", "(".repeat(100), ")".repeat(101));
    assert_eq!(specctra::read_ses(&deep, &l), Err(specctra::Error::TooDeep));
    let small = specctra::Limits { max_bytes: 10, ..l };
    assert_eq!(specctra::read_ses("(session x x x x x)", &small), Err(specctra::Error::TooLarge));
    let few = specctra::Limits { max_nodes: 5, ..l };
    assert_eq!(specctra::read_ses("(session a b c d e f g)", &few), Err(specctra::Error::TooMany));
    // Odd values are skipped, not trusted.
    let s = specctra::read_ses(
        "(session x (routes (resolution parsec 1) (network_out (net A (wire (path F.Cu nan 0 0 1e300 5)) (via v x y)))))",
        &l,
    )
    .unwrap();
    assert!(s.nets[0].wires.is_empty() && s.nets[0].vias.is_empty());
    // The quote character can be changed.
    let n = specctra::parse("(pcb (parser (string_quote ')) (x 'a b'))", &l).unwrap();
    assert_eq!(atoms(find(&n, "x")[0]), ["x", "a b"]);
    assert_eq!(specctra::via_from_name("Via[0-1]_800:400_um"), Some((0.8, 0.4)));
    assert_eq!(specctra::via_from_name("Via[0-1]_400:800_um"), None);
}

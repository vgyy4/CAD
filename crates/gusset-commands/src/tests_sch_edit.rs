//! Schematic editing options (ADR 0052): orthogonal following, moving a selection in one
//! undo step, annotation orders, ranges and units.

use crate::tests_pcb::{run, session};
use crate::*;
use serde_json::json;

fn u(t: u64) -> CmdCtx {
    CmdCtx::user(t)
}

const LIB: &str = r##"(kicad_symbol_lib (version 20241209) (generator "hand")
  (symbol "R" (pin_names (offset 0)) (in_bom yes) (on_board yes)
    (property "Reference" "R" (at 2.032 0 90) (effects (font (size 1.27 1.27))))
    (property "Value" "10k" (at 0 0 90) (effects (font (size 1.27 1.27))))
    (symbol "R_0_1" (rectangle (start -1.016 -2.54) (end 1.016 2.54) (stroke (width 0.254) (type default)) (fill (type none))))
    (symbol "R_1_1"
      (pin passive line (at 0 3.81 270) (length 1.27) (name "~" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))
      (pin passive line (at 0 -3.81 90) (length 1.27) (name "~" (effects (font (size 1.27 1.27)))) (number "2" (effects (font (size 1.27 1.27)))))))
  (symbol "AMP" (in_bom yes) (on_board yes)
    (property "Reference" "U" (at 0 5.08 0) (effects (font (size 1.27 1.27))))
    (property "Value" "AMP" (at 0 -5.08 0) (effects (font (size 1.27 1.27))))
    (symbol "AMP_1_1"
      (pin output line (at 7.62 0 180) (length 2.54) (name "~" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27))))))
    (symbol "AMP_2_1"
      (pin output line (at 7.62 0 180) (length 2.54) (name "~" (effects (font (size 1.27 1.27)))) (number "7" (effects (font (size 1.27 1.27)))))))
)
"##;

fn setup() -> Session {
    let s = session();
    std::fs::write(s.project_dir.clone().unwrap().join("hand.kicad_sym"), LIB).unwrap();
    s
}

fn open(s: &Session) -> std::sync::Arc<sch_session::OpenSchematic> {
    sch_session::open_schematic(s, &gusset_history::Actor::User).unwrap().unwrap()
}

fn add(s: &mut Session, t: u64, symbol: &str, at: [f64; 2], unit: u32) {
    run(s, &u(t), "sch.add_symbol", json!({"library": "hand.kicad_sym", "symbol": symbol, "at": at, "unit": unit}));
}

fn refs(s: &Session) -> Vec<String> {
    open(s).design.sheets[0].1.symbols.iter().map(|x| x.reference().to_string()).collect()
}

#[test]
fn wires_follow_orthogonally_and_a_selection_moves_in_one_undo_step() {
    let mut s = setup();
    run(&mut s, &u(1), "sch.new", json!({"name": "demo"}));
    add(&mut s, 2, "R", [100.0, 50.0], 1);
    add(&mut s, 3, "R", [110.0, 50.0], 1);
    run(&mut s, &u(4), "sch.add_wire", json!({"points": [[100, 46.19], [110, 46.19]]}));
    run(&mut s, &u(5), "sch.add_wire", json!({"points": [[100, 53.81], [100, 60]]}));
    run(&mut s, &u(6), "sch.add_label", json!({"text": "OUT", "at": [110, 53.81]}));
    let wires = |s: &Session| open(s).design.sheets[0].1.wires.iter().map(|w| (w.a, w.b)).collect::<Vec<_>>();
    let before = wires(&s);
    // R2 moves diagonally: the wire from R1 stays horizontal, a vertical segment follows.
    run(&mut s, &u(30), "sch.move", json!({"item": "R2", "at": [115, 60]}));
    let w = wires(&s);
    assert!(w.iter().all(|(a, b)| a[0] == b[0] || a[1] == b[1]), "{w:?}");
    assert!(w.contains(&([100.0, 46.19], [115.0, 46.19])), "{w:?}");
    run(&mut s, &u(31), "history.undo", json!({}));
    assert_eq!(wires(&s), before);
    // A selection (both resistors, the wire between them and the label) shifts as one step.
    let r = run(&mut s, &u(32), "sch.move", json!({"item": "R1", "by": [0, 10], "with": ["R2", "wire#0", "label#0"]}));
    assert_eq!(r["moved"], 4, "{r}");
    let o = open(&s);
    let sch = &o.design.sheets[0].1;
    assert_eq!((sch.symbols[0].at, sch.symbols[1].at), ([100.0, 60.0], [110.0, 60.0]));
    assert_eq!(sch.labels[0].at, [110.0, 63.81]);
    // The wire that was left behind follows R1's pin 2 without a diagonal.
    assert!(sch.wires.iter().all(|w| w.a[0] == w.b[0] || w.a[1] == w.b[1]));
    run(&mut s, &u(33), "history.undo", json!({}));
    assert_eq!(open(&s).design.sheets[0].1.symbols[0].at, [100.0, 50.0]);
    assert_eq!(wires(&s), before);
    // Deleting a selection is one step too.
    run(&mut s, &u(34), "sch.delete", json!({"items": ["R1", "R2", "wire#0", "label#0"]}));
    assert!(open(&s).design.sheets[0].1.symbols.is_empty());
    run(&mut s, &u(35), "history.undo", json!({}));
    assert_eq!(open(&s).design.sheets[0].1.symbols.len(), 2);
    // Bad uses.
    for args in [
        json!({"item": "R1", "by": [1, 1], "angle": 90}),
        json!({"item": "wire#0", "at": [1, 1], "with": ["R1"]}),
        json!({"item": "R1", "by": [1, 1], "with": ["R9"]}),
        json!({"item": "R1", "by": [1, 1], "at": [1, 1], "with": ["R2"]}),
    ] {
        assert!(execute(&mut s, &u(36), "sch.move", args.clone()).is_err(), "{args}");
    }
}

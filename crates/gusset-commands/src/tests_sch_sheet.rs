//! Hierarchical sheets through the registry (ADR 0057): `sch.add_sheet`, `sch.sheet_pin`,
//! deleting and moving sheets, each one undo step, saving the sub-sheet file.

#![allow(clippy::float_cmp)]

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

fn mismatches(s: &Session) -> usize {
    open(s).erc.iter().filter(|i| i.code == "hier_label_mismatch").count()
}

/// R1 on the root and R2 with a hierarchical label SIG on the sub-sheet `power.kicad_sch`.
fn two_sheets(s: &mut Session) {
    run(s, &u(1), "sch.new", json!({"name": "demo"}));
    let r = run(s, &u(2), "sch.add_sheet", json!({"name": "Power", "file": "power", "at": [120, 40]}));
    assert_eq!((r["sheet"].clone(), r["new_file"].clone()), (json!(0), json!(true)), "{r}");
    run(s, &u(3), "sch.add_symbol", json!({"library": "hand.kicad_sym", "symbol": "R", "at": [100, 50]}));
    run(
        s,
        &u(4),
        "sch.add_symbol",
        json!({"library": "hand.kicad_sym", "symbol": "R", "at": [50, 50], "sheet": "power.kicad_sch"}),
    );
    run(s, &u(5), "sch.add_wire", json!({"points": [[50, 46.19], [50, 40]], "sheet": "power.kicad_sch"}));
    run(
        s,
        &u(6),
        "sch.add_label",
        json!({"text": "SIG", "at": [50, 40], "kind": "hierarchical", "shape": "input", "sheet": "power.kicad_sch"}),
    );
}

#[test]
fn a_sheet_with_a_pin_joins_the_sub_sheet_and_every_step_undoes() {
    let mut s = setup();
    two_sheets(&mut s);
    let o = open(&s);
    assert_eq!(o.design.sheets.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["demo.kicad_sch", "power.kicad_sch"]);
    assert_eq!(o.design.sheets[0].1.sheets.len(), 1);
    // The pin is unmatched until the label inside has its partner.
    assert_eq!(mismatches(&s), 1);
    let r = run(&mut s, &u(7), "sch.sheet_pin", json!({"item": "Power", "op": "add", "name": "SIG", "kind": "input"}));
    assert_eq!((r["pin"].clone(), r["at"].clone()), (json!(0), json!([120.0, 42.54])), "{r}");
    assert_eq!(mismatches(&s), 0);
    run(&mut s, &u(8), "sch.add_wire", json!({"points": [[100, 46.19], [100, 42.54], [120, 42.54]]}));
    let nets = |s: &Session| {
        open(s)
            .netlist
            .nets
            .iter()
            .filter(|n| n.nodes.iter().any(|p| p.reference == "R1") && n.nodes.iter().any(|p| p.reference == "R2"))
            .count()
    };
    assert_eq!(nets(&s), 1);
    // Rename: one undo step breaks and restores the join; ERC shows both ends meanwhile.
    run(&mut s, &u(9), "sch.sheet_pin", json!({"item": "sheet#0", "op": "rename", "name": "SIG", "new_name": "OTHER"}));
    assert_eq!((nets(&s), mismatches(&s)), (0, 2));
    run(&mut s, &u(10), "history.undo", json!({}));
    assert_eq!((nets(&s), mismatches(&s)), (1, 0));
    // Remove, and undo.
    run(&mut s, &u(11), "sch.sheet_pin", json!({"item": "Power", "op": "remove", "name": "SIG"}));
    assert_eq!(open(&s).design.sheets[0].1.sheets[0].pins.len(), 0);
    run(&mut s, &u(12), "history.undo", json!({}));
    assert_eq!(open(&s).design.sheets[0].1.sheets[0].pins.len(), 1);
    // Undoing the sheet pin and the sheet takes the sub-sheet file out again.
    for t in [13, 14, 15] {
        run(&mut s, &u(t), "history.undo", json!({}));
    }
    assert_eq!(open(&s).design.sheets[0].1.sheets[0].pins.len(), 0);
    // Further back: steps 6..3 are edits of the sub-sheet and R1, then the sheet itself.
    for t in 16..=19 {
        run(&mut s, &u(t), "history.undo", json!({}));
    }
    let o = open(&s);
    assert_eq!((o.design.sheets.len(), o.design.sheets[0].1.sheets.len()), (1, 0));
}

#[test]
fn sheet_files_are_checked_reused_and_saved() {
    let mut s = setup();
    two_sheets(&mut s);
    // A second sheet on the same file reuses it; no second file is started.
    let r = run(
        &mut s,
        &u(7),
        "sch.add_sheet",
        json!({"name": "Power B", "file": "power.kicad_sch", "at": [160, 40], "size": [20, 20]}),
    );
    assert_eq!((r["sheet"].clone(), r["new_file"].clone()), (json!(1), json!(false)), "{r}");
    assert_eq!(open(&s).design.sheets.len(), 2);
    // Refused: itself, the root file, a cycle, unsafe names, a bad size.
    for args in [
        json!({"name": "x", "file": "power.kicad_sch", "at": [0, 0], "sheet": "power.kicad_sch"}),
        json!({"name": "x", "file": "demo.kicad_sch", "at": [0, 0]}),
        json!({"name": "x", "file": "../up.kicad_sch", "at": [0, 0]}),
        json!({"name": "x", "file": "/abs.kicad_sch", "at": [0, 0]}),
        json!({"name": "x", "file": "a:b.kicad_sch", "at": [0, 0]}),
        json!({"name": "x", "file": "ok", "at": [0, 0], "size": [1, 1]}),
        json!({"name": "", "file": "ok", "at": [0, 0]}),
    ] {
        assert!(execute(&mut s, &u(8), "sch.add_sheet", args.clone()).is_err(), "{args}");
    }
    // A sheet inside the sub-sheet, then the root file cannot be put in that one's way.
    run(
        &mut s,
        &u(9),
        "sch.add_sheet",
        json!({"name": "Deep", "file": "deep", "at": [10, 10], "sheet": "power.kicad_sch"}),
    );
    assert_eq!(open(&s).design.sheets.len(), 3);
    assert!(
        execute(
            &mut s,
            &u(10),
            "sch.add_sheet",
            json!({"name": "Loop", "file": "power", "at": [0, 0], "sheet": "deep.kicad_sch"})
        )
        .is_err()
    );
    // Pin errors.
    for args in [
        json!({"item": "Nope", "op": "add", "name": "A"}),
        json!({"item": "R1", "op": "add", "name": "A"}),
        json!({"item": "Power", "op": "spin", "name": "A"}),
        json!({"item": "Power", "op": "rename", "name": "A"}),
        json!({"item": "Power", "op": "add", "name": "A", "side": "up"}),
        json!({"item": "Power", "op": "remove", "name": "missing"}),
    ] {
        assert!(execute(&mut s, &u(11), "sch.sheet_pin", args.clone()).is_err(), "{args}");
    }
    // Save writes the sub-sheet files; opening the root reads them back.
    let dir = s.project_dir.clone().unwrap();
    let root = dir.join("demo.kicad_sch").to_string_lossy().into_owned();
    let r = run(&mut s, &u(12), "sch.save", json!({"path": root, "overwrite": true}));
    assert_eq!(r["written"].as_array().map(Vec::len), Some(3), "{r}");
    for f in ["demo.kicad_sch", "power.kicad_sch", "deep.kicad_sch"] {
        assert!(dir.join(f).is_file(), "{f}");
    }
    let before = open(&s).design.sheets.iter().map(|x| x.1.write()).collect::<Vec<_>>();
    run(&mut s, &u(13), "sch.open", json!({"path": root}));
    let o = open(&s);
    assert_eq!(o.design.sheets.len(), 3);
    assert_eq!(o.design.sheets.iter().map(|x| x.1.write()).collect::<Vec<_>>(), before);
}

#[test]
fn deleting_and_moving_a_sheet_are_single_undoable_steps() {
    let mut s = setup();
    two_sheets(&mut s);
    run(&mut s, &u(7), "sch.sheet_pin", json!({"item": "Power", "op": "add", "name": "SIG", "kind": "input"}));
    let r = run(&mut s, &u(8), "sch.move", json!({"item": "sheet#0", "by": [5.08, 2.54]}));
    assert_eq!(r["moved"], 1, "{r}");
    let sh = &open(&s).design.sheets[0].1.sheets[0].clone();
    assert_eq!((sh.at, sh.pins[0].at), ([125.08, 42.54], [125.08, 45.08]));
    run(&mut s, &u(9), "sch.move", json!({"item": "sheet#0", "at": [120, 40]}));
    assert_eq!(open(&s).design.sheets[0].1.sheets[0].at, [120.0, 40.0]);
    // Deleting the only sheet that uses power.kicad_sch drops the file; undo brings it back.
    let r = run(&mut s, &u(10), "sch.delete", json!({"items": ["sheet#0"]}));
    assert_eq!(r["deleted"], 1);
    let o = open(&s);
    assert_eq!((o.design.sheets.len(), o.design.sheets[0].1.sheets.len()), (1, 0));
    run(&mut s, &u(11), "history.undo", json!({}));
    let o = open(&s);
    assert_eq!((o.design.sheets.len(), o.design.sheets[0].1.sheets.len()), (2, 1));
    assert_eq!(o.design.sheets[1].1.symbols.len(), 1);
}

#[test]
fn a_symbol_on_a_sub_sheet_has_the_sub_sheets_instance_path() {
    let mut s = setup();
    two_sheets(&mut s);
    let o = open(&s);
    let uses = o.design.uses();
    let sub = o.design.sheet_named("power.kicad_sch").unwrap();
    let want: Vec<String> = uses.iter().filter(|u| u.sheet == sub).map(|u| u.path.clone()).collect();
    assert_eq!(want.len(), 1);
    assert_eq!(want[0].matches('/').count(), 2, "{want:?}");
    let (_, sch) = o.design.sheets.get(sub).unwrap();
    let r2 = sch.symbols.last().unwrap();
    let paths: Vec<&str> = r2.instances.iter().map(|i| i.path.as_str()).collect();
    assert_eq!(paths, [want[0].as_str()]);
    assert_eq!(r2.reference_in(&want[0]), "R2");
    // On the root, the root's own path.
    let (_, root) = o.design.sheets.first().unwrap();
    let r1 = root.symbols.first().unwrap();
    assert_eq!(r1.instances.iter().map(|i| i.path.clone()).collect::<Vec<_>>(), [uses[0].path.clone()]);
}

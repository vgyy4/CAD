//! The symbol library browser (ADR 0057): folder scan on a worker, cache, filter, preview,
//! and placing the picked symbol through `sch.add_symbol`.

use crate::sch_view::{self, State, Tool};
use crate::symbol_browser::{self, Browser};
use crate::tests::{app, harness};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

const PASSIVES: &str = r##"(kicad_symbol_lib (version 20241209) (generator "hand")
  (symbol "R" (in_bom yes) (on_board yes)
    (property "Reference" "R" (at 2 0 90) (effects (font (size 1.27 1.27))))
    (property "Value" "R" (at 0 0 90) (effects (font (size 1.27 1.27))))
    (property "Description" "Resistor" (at 0 0 0) (effects (font (size 1.27 1.27)) (hide yes)))
    (property "ki_keywords" "R res resistor" (at 0 0 0) (effects (font (size 1.27 1.27)) (hide yes)))
    (symbol "R_0_1" (rectangle (start -1 -2.5) (end 1 2.5) (stroke (width 0.25) (type default)) (fill (type background))))
    (symbol "R_1_1"
      (pin passive line (at 0 3.8 270) (length 1.3) (name "~" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))
      (pin passive line (at 0 -3.8 90) (length 1.3) (name "~" (effects (font (size 1.27 1.27)))) (number "2" (effects (font (size 1.27 1.27)))))))
  (symbol "C" (in_bom yes) (on_board yes)
    (property "Reference" "C" (at 2 0 90) (effects (font (size 1.27 1.27))))
    (property "Value" "C" (at 0 0 90) (effects (font (size 1.27 1.27))))
    (property "Description" "Unpolarized capacitor" (at 0 0 0) (effects (font (size 1.27 1.27)) (hide yes)))
    (property "ki_keywords" "cap capacitor" (at 0 0 0) (effects (font (size 1.27 1.27)) (hide yes)))
    (symbol "C_1_1"
      (pin passive line (at 0 3.8 270) (length 3) (name "~" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))
      (pin passive line (at 0 -3.8 90) (length 3) (name "~" (effects (font (size 1.27 1.27)))) (number "2" (effects (font (size 1.27 1.27))))))))
"##;

const CHIPS: &str = r##"(kicad_symbol_lib (version 20241209) (generator "hand")
  (symbol "Dual_Opamp" (in_bom yes) (on_board yes)
    (property "Reference" "U" (at 0 5 0) (effects (font (size 1.27 1.27))))
    (property "Value" "Dual_Opamp" (at 0 -5 0) (effects (font (size 1.27 1.27))))
    (property "Description" "Dual operational amplifier" (at 0 0 0) (effects (font (size 1.27 1.27)) (hide yes)))
    (symbol "Dual_Opamp_1_1"
      (polyline (pts (xy -5 5) (xy 5 0) (xy -5 -5) (xy -5 5)) (stroke (width 0.25) (type default)) (fill (type background)))
      (pin output line (at 7.5 0 180) (length 2.5) (name "~" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27))))))
    (symbol "Dual_Opamp_2_1"
      (circle (center 0 0) (radius 5) (stroke (width 0.25) (type default)) (fill (type none)))
      (pin output line (at 7.5 0 180) (length 2.5) (name "~" (effects (font (size 1.27 1.27)))) (number "7" (effects (font (size 1.27 1.27))))))))
"##;

/// A temp folder with two libraries, a broken one and files that are not libraries.
fn folder(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gusset-symlib-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Passives.kicad_sym"), PASSIVES).unwrap();
    std::fs::write(dir.join("Chips.kicad_sym"), CHIPS).unwrap();
    std::fs::write(dir.join("Broken.kicad_sym"), "(kicad_symbol_lib (symbol").unwrap();
    std::fs::write(dir.join("notes.txt"), "not a library").unwrap();
    dir
}

/// Polls the browser until its scan (and the preview) are done.
fn settle(b: &mut Browser) {
    for _ in 0..1000 {
        if !b.poll() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the browser did not settle");
}

#[test]
fn the_folder_is_scanned_off_thread_filtered_and_cached() {
    let dir = folder("scan");
    let files = symbol_browser::library_files(&dir);
    let names: Vec<_> = files.iter().filter_map(|f| f.file_name()?.to_str()).collect();
    assert_eq!(names, ["Broken.kicad_sym", "Chips.kicad_sym", "Passives.kicad_sym"]);
    // A single library file works as the "folder", anything else lists nothing.
    assert_eq!(symbol_browser::library_files(&dir.join("Chips.kicad_sym")).len(), 1);
    assert!(symbol_browser::library_files(&dir.join("notes.txt")).is_empty());
    assert!(symbol_browser::library_files(&dir.join("missing")).is_empty());

    let mut b = Browser::default();
    assert!(!b.poll(), "no folder: nothing to do");
    b.folder = dir.to_string_lossy().into_owned();
    settle(&mut b);
    assert_eq!((b.libraries(), b.row_count()), (2, 3));
    let errs = b.errors();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs.first().map(|e| e.0.as_str()), Some("Broken.kicad_sym"));
    let (handled, total, done) = b.progress();
    assert_eq!((handled, total, done), (3, 3, true));
    // The filter looks at names, descriptions and keywords.
    b.filter = "capacitor".into();
    settle(&mut b);
    assert_eq!(b.rows().map(|(_, s)| s.name.as_str()).collect::<Vec<_>>(), ["C"]);
    b.filter = "dual amp".into();
    settle(&mut b);
    assert_eq!(b.rows().map(|(l, s)| format!("{}:{}", l.nickname(), s.name)).collect::<Vec<_>>(), ["Chips:Dual_Opamp"]);
    b.filter = "zzz".into();
    settle(&mut b);
    assert_eq!(b.row_count(), 0);
    b.filter.clear();

    // A second scan takes unchanged files from the cache and re-reads a changed one.
    let cache = std::sync::Mutex::new(symbol_browser::Cache::new());
    let chips = dir.join("Chips.kicad_sym");
    let first = symbol_browser::index_file(&chips, &cache).unwrap();
    let again = symbol_browser::index_file(&chips, &cache).unwrap();
    assert!(Arc::ptr_eq(&first, &again));
    std::fs::write(&chips, format!("{CHIPS}\n\n")).unwrap();
    let changed = symbol_browser::index_file(&chips, &cache).unwrap();
    assert!(!Arc::ptr_eq(&first, &changed));
    // Changing the folder starts a new scan and drops the old list.
    let other = folder("scan2");
    std::fs::remove_file(other.join("Passives.kicad_sym")).unwrap();
    b.folder = other.to_string_lossy().into_owned();
    settle(&mut b);
    assert_eq!(b.libraries(), 1);
    b.folder = String::new();
    settle(&mut b);
    assert_eq!((b.libraries(), b.row_count()), (0, 0));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&other);
}

#[test]
fn big_and_odd_files_are_refused_not_read() {
    let dir = std::env::temp_dir().join(format!("gusset-symlib-big-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let big = dir.join("Big.kicad_sym");
    let f = std::fs::File::create(&big).unwrap();
    f.set_len(symbol_browser::MAX_FILE_BYTES + 1).unwrap();
    drop(f);
    assert_eq!(symbol_browser::read_library(&big).unwrap_err(), "file too large");
    let bin = dir.join("Bin.kicad_sym");
    std::fs::write(&bin, [0xff, 0xfe, 0x00]).unwrap();
    assert_eq!(symbol_browser::read_library(&bin).unwrap_err(), "not UTF-8 text");
    assert!(symbol_browser::read_library(&dir).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_picked_symbol_previews_with_units_and_places_through_the_registry() {
    let dir = folder("place");
    let mut b = Browser::default();
    b.folder = dir.to_string_lossy().into_owned();
    settle(&mut b);
    assert!(b.placement().is_none());
    b.pick("Chips.kicad_sym", "Dual_Opamp");
    assert!(b.preview().is_none());
    settle(&mut b);
    let Some(Ok(lib)) = b.preview() else { panic!("{:?}", b.preview()) };
    assert_eq!((lib.name.as_str(), lib.unit_count()), ("Dual_Opamp", 2));
    assert_eq!(b.units(), 2);
    // Picking another symbol resets the unit; picking the same keeps it.
    b.unit = 2;
    b.pick("Chips.kicad_sym", "Dual_Opamp");
    assert_eq!(b.unit, 2);
    let p = b.placement().unwrap();
    assert_eq!((p.symbol.as_str(), p.unit), ("Dual_Opamp", 2));
    assert!(p.library.ends_with("Chips.kicad_sym"));
    // A missing symbol reports its error instead of a preview.
    b.pick("Chips.kicad_sym", "Nope");
    settle(&mut b);
    assert!(matches!(b.preview(), Some(Err(_))));
    b.pick("Chips.kicad_sym", "Dual_Opamp");
    b.unit = 2;
    settle(&mut b);

    // Place arms the tool; the click sends sch.add_symbol with the unit.
    let mut st = State::default();
    sch_view::arm_place(&mut st, &p);
    assert_eq!(st.tool, Tool::Place);
    let (id, args) = sch_view::place_command(&st, [100.0, 50.0]).unwrap();
    assert_eq!((id, args["unit"].clone(), args["symbol"].clone()), ("sch.add_symbol", json!(2), json!("Dual_Opamp")));
    st.unit = 1;
    assert!(sch_view::place_command(&st, [0.0, 0.0]).unwrap().1.get("unit").is_none());

    let mut h = harness(app());
    h.run();
    h.state_mut().run("sch.new", json!({})).unwrap();
    h.state_mut().wait_idle();
    sch_view::arm_place(&mut h.state_mut().sch_state, &p);
    let st = &h.state().sch_state;
    let out =
        crate::pcb_view::Out { commands: vec![sch_view::place_command(st, [100.0, 50.0]).unwrap()], blocked: None };
    h.state_mut().pcb_commands(out);
    h.state_mut().wait_idle();
    let o = h.state().snapshot.sch.clone().unwrap();
    let s = &o.design.sheets[0].1;
    assert_eq!((s.symbols.len(), s.symbols[0].unit), (1, 2));
    assert_eq!(s.symbols[0].lib_id, "Chips:Dual_Opamp");
    h.state_mut().run("history.undo", json!({})).unwrap();
    h.state_mut().wait_idle();
    assert!(h.state().snapshot.sch.as_ref().unwrap().design.sheets[0].1.symbols.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_panel_draws_the_list_and_the_preview() {
    let dir = folder("panel");
    let mut h = harness(app());
    h.run();
    h.state_mut().ribbon_action(crate::ribbon::Action::SchTab);
    h.state_mut().run("sch.new", json!({})).unwrap();
    h.state_mut().wait_idle();
    h.state_mut().sch_state.browser.folder = dir.to_string_lossy().into_owned();
    // The scan runs on its own thread and the panel repaints until it is done: step frames
    // until then (a slow runner needs more than `run`'s settle limit), at most 10 s.
    for _ in 0..1000 {
        h.step();
        if h.state().sch_state.browser.progress().2 && h.state().sch_state.browser.libraries() == 2 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(h.state().sch_state.browser.libraries(), 2);
    h.state_mut().sch_state.browser.pick("Passives.kicad_sym", "R");
    for _ in 0..1000 {
        h.step();
        if h.state().sch_state.browser.preview().is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    h.step();
    assert!(matches!(h.state().sch_state.browser.preview(), Some(Ok(_))));
    let _ = std::fs::remove_dir_all(&dir);
}

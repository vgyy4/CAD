#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::print_stderr)]
//! Gerber X2 object attributes, checked by re-reading the output (ADR 0051).

use crate::board::{Board, Limits};
use crate::gerber::{self, escape_field};
use crate::gerber_read::{self, Kind};
use crate::tests::BOARD;

fn board() -> Board {
    Board::read(BOARD, &Limits::default()).unwrap()
}

#[test]
fn escapes_delimiters_and_non_ascii() {
    assert_eq!(escape_field("Net-1 (A)"), "Net-1 (A)");
    assert_eq!(escape_field("A,B*C%D\\E"), "A\\u002CB\\u002AC\\u0025D\\u005CE");
    assert_eq!(escape_field("é€😀\n"), "\\u00E9\\u20AC\\uD83D\\uDE00\\u000A");
    for s in ["a,b", "é€😀", "100%*\\", "plain", "tab\there"] {
        assert_eq!(gerber_read::unescape(&escape_field(s)).unwrap(), s);
    }
    assert!(gerber_read::unescape("a,b\\x").is_err());
    assert!(gerber_read::unescape("a*b").is_err());
}

#[test]
fn copper_objects_carry_net_pad_and_component() {
    let b = board();
    let f = gerber::write_layer(&b, "F.Cu").unwrap();
    assert!(f.contains("%TO.N,VCC*%") && f.contains("%TO.N,GND*%"), "{f}");
    assert!(f.contains("%TO.P,R1,1*%") && f.contains("%TO.C,R1*%") && f.contains("%TO.P,J1,2*%"), "{f}");
    // A change of owner deletes what no longer applies.
    assert!(f.contains("%TD.P*%"), "{f}");
    let p = gerber_read::check_file(&f).unwrap();
    let pad = p.objects.iter().find(|o| o.pin() == Some(("J1", "1"))).unwrap();
    assert_eq!((pad.net(), pad.comp()), (Some("VCC"), Some("J1")));
    assert!(matches!(pad.kind, Kind::Flash { .. }));
    // Zone fills sit on their net, with no pad attributes.
    let back = gerber_read::check_file(&gerber::write_layer(&b, "B.Cu").unwrap()).unwrap();
    let fill = back
        .objects
        .iter()
        .find(|o| matches!(o.kind, Kind::Region { .. }) && o.func.as_deref() == Some("Conductor"))
        .unwrap();
    assert_eq!((fill.net(), fill.pin(), fill.comp()), (Some("GND"), None, None));
    // Other layers have footprint attributes only.
    let silk = gerber::write_layer(&b, "F.SilkS").unwrap();
    assert!(silk.contains("%TO.C,") && !silk.contains("%TO.N") && !silk.contains("%TO.P"), "{silk}");
}

#[test]
fn net_names_with_commas_and_unicode_are_escaped() {
    let mut b = board();
    b.rename_net("VCC", "+5V,Ünit*", &Limits::default()).unwrap();
    let f = gerber::write_layer(&b, "F.Cu").unwrap();
    assert!(f.contains("%TO.N,+5V\\u002C\\u00DCnit\\u002A*%"), "{f}");
    let p = gerber_read::check_file(&f).unwrap();
    assert!(p.objects.iter().any(|o| o.net() == Some("+5V,Ünit*")));
}

#[test]
fn every_layer_of_the_fab_set_re_reads() {
    let b = board();
    for l in gerber::write_all(&b).unwrap() {
        let p = gerber_read::check_file(&l.text).unwrap();
        assert!(p.ended && (p.scale - 1e-6).abs() < 1e-12, "{}", l.layer);
        assert!(!p.objects.is_empty() || l.layer.contains("Paste") || l.layer == "B.SilkS", "{} is empty", l.layer);
    }
}

#[test]
fn the_re_reader_rejects_broken_files() {
    let head = "%FSLAX46Y46*%\n%MOMM*%\n%TF.FileFunction,Copper,L1,Top*%\nG75*\n%ADD10C,0.2*%\n";
    let ok = format!("{head}D10*\nX0Y0D02*\nX1000000Y0D01*\nM02*\n");
    assert!(gerber_read::check_file(&ok).is_ok());
    let open = format!("{head}G36*\nX0Y0D02*\nX1000000Y0D01*\nX1000000Y1000000D01*\nG37*\nM02*\n");
    assert!(gerber_read::check_file(&open).unwrap_err().contains("not closed"));
    let undefined = format!("{head}D11*\nM02*\n");
    assert!(gerber_read::check_file(&undefined).unwrap_err().contains("not defined"));
    let no_g75 = ok.replace("G75*\n", "").replace("D01*", "D01*\nG03*\nX0Y0I1D01*");
    assert!(gerber_read::check_file(&no_g75).is_err());
    let far = format!("{head}D10*\nX0Y0D02*\nX99990000000Y0D01*\nM02*\n");
    assert!(gerber_read::check_file(&far).unwrap_err().contains("range"));
    assert!(gerber_read::check_file(&ok.replace("M02*\n", "")).is_err());
}

/// Every KiCad sample the user has (`GUSSET_KICAD_SAMPLES`): all layers re-read and match
/// the board (`write_layer` checks them itself in tests).
#[test]
fn local_samples_gerbers_re_read() {
    let Some(dir) = std::env::var_os("GUSSET_KICAD_SAMPLES") else { return };
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "kicad_pcb"))
        .collect();
    paths.sort();
    for p in paths {
        let b = Board::read(&std::fs::read_to_string(&p).unwrap(), &Limits::default()).unwrap();
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        let (mut files, mut objects) = (0, 0);
        for l in gerber::write_all(&b).unwrap() {
            let p = gerber_read::check_file(&l.text).unwrap_or_else(|e| panic!("{name} {}: {e}", l.layer));
            files += 1;
            objects += p.objects.len();
        }
        eprintln!("{name}: {files} files, {objects} objects re-read");
    }
}

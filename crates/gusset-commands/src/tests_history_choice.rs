//! The history choice of `import.file` (ADR 0042 amendment): a native file's history is
//! perfect, imperfect or none, and `history` says what comes in.

use crate::*;
use gusset_doc::Document;
use gusset_kernel::Kernel;
use gusset_kernel::mock::MockKernel;
use serde_json::{Value, json};
use std::sync::Arc;

/// An import worker whose every shape is a 10 × 10 × `height` mm box at the origin.
struct BoxWorker(f64);

impl session::Importer for BoxWorker {
    fn import(
        &self,
        format: &str,
        _: &[u8],
        _: &std::sync::atomic::AtomicBool,
    ) -> Result<Vec<(String, Vec<u8>)>, String> {
        assert_eq!(format, "brp");
        let k = MockKernel;
        let sq = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let edges = (0..4)
            .map(|i| gusset_kernel::ProfileEdge {
                seg: gusset_kernel::Seg2::Line { a: sq[i], b: sq[(i + 1) % 4] },
                tag: i as u64 + 1,
            })
            .collect();
        let p = gusset_kernel::Profile {
            frame: gusset_kernel::Frame::XY,
            regions: vec![gusset_kernel::Region2 { outer: gusset_kernel::Loop2 { edges }, holes: vec![] }],
        };
        let shape = k.extrude(&p, &gusset_kernel::ExtrudeSpec { distance: self.0, symmetric: false }).unwrap().shape;
        Ok(vec![("box".into(), k.write_native(&shape).unwrap())])
    }
}

/// A FreeCAD document of one object of type `kind` (a 10 mm `Part::Box` has a history).
fn fcstd(kind: &str) -> Vec<u8> {
    let doc = format!(
        r#"<Document SchemaVersion="4"><Objects Count="1"><Object type="{kind}" name="Box"/></Objects>
<ObjectData Count="1"><Object name="Box"><Properties>
<Property name="Length" type="App::PropertyLength"><Float value="10"/></Property>
<Property name="Width" type="App::PropertyLength"><Float value="10"/></Property>
<Property name="Height" type="App::PropertyLength"><Float value="10"/></Property>
<Property name="Shape" type="Part::PropertyPartShape"><Part file="PartShape.brp"/></Property>
</Properties></Object></ObjectData></Document>"#
    );
    gusset_meshio::archive::write(&[("Document.xml", doc.as_bytes()), ("PartShape.brp", b"CASCADE")]).unwrap()
}

/// A session whose worker reads every shape as a box `height` mm high, with `part.FCStd` of
/// object type `kind` in its folder.
fn session(height: f64, kind: &str) -> Session {
    let doc = Document::from_text("gusset 1\nunits mm\n").unwrap();
    let mut s = Session::new(&doc, Arc::new(MockKernel), &CmdCtx::user(0));
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("gusset-hchoice-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("part.FCStd"), fcstd(kind)).unwrap();
    s.project_dir = Some(dir);
    s.importer = Some(Arc::new(BoxWorker(height)));
    s
}

fn import(s: &mut Session, history: Option<&str>) -> Value {
    let mut args = json!({"path": "part.FCStd"});
    if let Some(h) = history {
        args["history"] = json!(h);
    }
    execute(s, &CmdCtx::user(1), "import.file", args).unwrap_or_else(|e| panic!("{e:?}"))
}

fn text(s: &mut Session) -> String {
    s.document(&gusset_history::Actor::User).unwrap().to_text()
}

fn tier(r: &Value) -> &str {
    r["tier"].as_str().unwrap_or_default()
}

#[test]
fn a_perfect_history_comes_in_as_features_by_default_or_as_the_shape_when_asked() {
    let mut s = session(10.0, "Part::Box");
    let r = import(&mut s, None);
    assert!(tier(&r).contains("PartDesign history"), "{r}");
    assert_eq!(r["history"]["class"], "perfect");
    assert_eq!(r["history"]["imported"], "history");
    assert!(text(&mut s).contains("extrude"), "{}", text(&mut s));

    let mut s = session(10.0, "Part::Box");
    let r = import(&mut s, Some("final"));
    assert_eq!(r["history"]["imported"], "final", "{r}");
    let t = text(&mut s);
    assert!(t.contains("format: fcstd") && !t.contains("extrude"), "{t}");

    // `rebuild` of a perfect history is the default's outcome.
    let mut s = session(10.0, "Part::Box");
    let r = import(&mut s, Some("rebuild"));
    assert!(tier(&r).contains("PartDesign history"), "{r}");
}

#[test]
fn ask_reports_the_class_and_commits_nothing_while_there_is_a_choice() {
    for (height, class, first) in [(10.0, "perfect", "rebuild"), (12.0, "imperfect", "final")] {
        let mut s = session(height, "Part::Box");
        let before = text(&mut s);
        let r = import(&mut s, Some("ask"));
        assert_eq!(r["choice_required"], "history", "{r}");
        assert_eq!(r["history"]["class"], class, "{r}");
        assert_eq!(r["history"]["options"][0], first, "{r}");
        assert_eq!(text(&mut s), before, "nothing is committed");
    }
    // Without a choice to make, `ask` imports.
    let mut s = session(10.0, "Part::Feature");
    let r = import(&mut s, Some("ask"));
    assert_eq!(r["history"]["class"], "none", "{r}");
    assert!(text(&mut s).contains("format: fcstd"));
}

#[test]
fn an_imperfect_history_is_the_shape_by_default_and_the_features_on_request_in_one_undo_step() {
    let mut s = session(12.0, "Part::Box");
    let r = import(&mut s, None);
    assert_eq!(r["history"]["class"], "imperfect", "{r}");
    assert_eq!(r["history"]["imported"], "final");
    let why = r["history"]["why_user_text"].as_str().unwrap();
    assert!(why.contains("1000") && why.contains("1200"), "says how much: {why}");
    // Today's warning stays (the corpus tools read it).
    assert!(r["warnings_user_text"].to_string().contains("design history came in as exact bodies"), "{r}");
    assert!(text(&mut s).contains("format: fcstd"));

    let mut s = session(12.0, "Part::Box");
    let before = text(&mut s);
    let r = import(&mut s, Some("rebuild"));
    assert_eq!(tier(&r), crate::import::TIER_PARTIAL_HISTORY, "{r}");
    assert_eq!(r["history"]["imported"], "history");
    assert!(r["warnings_user_text"].to_string().contains("differs from the file"), "{r}");
    assert!(text(&mut s).contains("extrude"));
    // The corpus metric (the format's history tier) does not count it.
    assert!(!tier(&r).contains("PartDesign history"));
    execute(&mut s, &CmdCtx::user(1), "history.undo", json!({})).unwrap();
    assert_eq!(text(&mut s), before, "one undo takes the whole import back");
}

#[test]
fn without_a_readable_history_only_the_shape_comes_in_and_the_result_says_why() {
    let mut s = session(10.0, "Part::Feature");
    let r = import(&mut s, Some("rebuild"));
    assert_eq!(r["history"]["class"], "none", "{r}");
    assert_eq!(r["history"]["imported"], "final");
    assert!(r["history"]["why_user_text"].as_str().unwrap().contains("no feature history"), "{r}");
    assert!(r["warnings_user_text"].to_string().contains("only the final shape"), "{r}");
    assert!(text(&mut s).contains("format: fcstd"));
}

#[test]
fn formats_without_a_history_have_no_class() {
    let mut s = session(10.0, "Part::Box");
    let dir = s.project_dir.clone().unwrap();
    let stl = "solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid t\n";
    std::fs::write(dir.join("t.stl"), stl).unwrap();
    let r = execute(&mut s, &CmdCtx::user(1), "import.file", json!({"path": "t.stl", "history": "ask"})).unwrap();
    assert!(r.get("history").is_none() && r.get("choice_required").is_none(), "{r}");
}

#[test]
fn a_feature_that_fails_is_kept_suppressed_with_its_error_only_when_asked() {
    let xy = json!({"origin": [0, 0, 0], "x_axis": [1, 0, 0], "y_axis": [0, 1, 0]});
    let t = json!({"format": "gusset-feature-tree", "version": 2, "features": [
        {"type": "sketch", "name": "S1", "plane": xy,
         "points": [{"id": "p1", "x": 0, "y": 0}, {"id": "p2", "x": 10, "y": 0}, {"id": "p3", "x": 10, "y": 10}, {"id": "p4", "x": 0, "y": 10}],
         "curves": [{"id": "l1", "type": "line", "start": "p1", "end": "p2"}, {"id": "l2", "type": "line", "start": "p2", "end": "p3"},
                    {"id": "l3", "type": "line", "start": "p3", "end": "p4"}, {"id": "l4", "type": "line", "start": "p4", "end": "p1"}],
         "constraints": []},
        {"type": "extrude", "name": "E1", "sketch": "S1", "profiles": "all", "distance": 5, "operation": "new"},
        // The mock kernel has no shell: it fails to build.
        {"type": "shell", "name": "Sh", "body": "E1", "thickness": 1, "faces": []},
        // An unknown feature writes nothing: it can only be reported.
        {"type": "unsupported", "name": "Loft9", "original_type": "LoftFeature"}
    ]});
    for keep in [false, true] {
        let mut tree = crate::tree_import::parse(&serde_json::to_vec(&t).unwrap()).unwrap();
        tree.keep_failed = keep;
        let mut s = session(10.0, "Part::Box");
        let r = crate::tree_import::apply(&mut s, &CmdCtx::user(1), &tree, "t").unwrap();
        assert_eq!(r["skipped_user_text"].as_array().unwrap().len(), 2, "{r}");
        let doc = s.document(&gusset_history::Actor::User).unwrap();
        let shell = doc.features.iter().find(|f| f.name == "Sh");
        if keep {
            let shell = shell.expect("kept");
            assert!(shell.suppressed);
            assert!(shell.note.as_deref().unwrap().contains("failed to rebuild"), "{:?}", shell.note);
        } else {
            assert!(shell.is_none());
        }
        assert!(s.evaluate(&gusset_history::Actor::User).unwrap().errors().is_empty());
    }
}

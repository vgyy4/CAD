//! The import's history choice in the GUI (ADR 0042 amendment): perfect and imperfect histories
//! ask, a history that cannot be read is said, and a history imported although it differs offers
//! Undo.

use crate::app::{App, Dialog, Then};
use crate::tests::{app, harness};
use egui_kittest::kittest::Queryable;
use gusset_kernel::Kernel;
use gusset_kernel::mock::MockKernel;
use serde_json::json;
use std::sync::Arc;

/// An import worker whose every shape is a 10 × 10 × `height` mm box at the origin.
pub(crate) struct BoxWorker(pub(crate) f64);

impl gusset_commands::session::Importer for BoxWorker {
    fn import(&self, _: &str, _: &[u8], _: &std::sync::atomic::AtomicBool) -> Result<Vec<(String, Vec<u8>)>, String> {
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
        let shape =
            k.extrude(&p, &gusset_kernel::ExtrudeSpec { distance: self.0, symmetric: false }).map_err(|e| e.message)?;
        Ok(vec![("box".into(), k.write_native(&shape.shape).map_err(|e| e.message)?)])
    }
}

/// A FreeCAD file of one 10 mm object of type `kind` (a `Part::Box` has a history), written to a
/// new folder; its path.
pub(crate) fn fcstd(kind: &str) -> std::path::PathBuf {
    let doc = format!(
        r#"<Document SchemaVersion="4"><Objects Count="1"><Object type="{kind}" name="Box"/></Objects>
<ObjectData Count="1"><Object name="Box"><Properties>
<Property name="Length" type="App::PropertyLength"><Float value="10"/></Property>
<Property name="Width" type="App::PropertyLength"><Float value="10"/></Property>
<Property name="Height" type="App::PropertyLength"><Float value="10"/></Property>
<Property name="Shape" type="Part::PropertyPartShape"><Part file="PartShape.brp"/></Property>
</Properties></Object></ObjectData></Document>"#
    );
    let zip =
        gusset_meshio::archive::write(&[("Document.xml", doc.as_bytes()), ("PartShape.brp", b"CASCADE")]).unwrap();
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("gusset-app-hist-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bracket.FCStd");
    std::fs::write(&path, zip).unwrap();
    path
}

/// An app whose worker reads shapes as boxes `height` high, importing a FreeCAD `kind` object.
pub(crate) fn importing(height: f64, kind: &str) -> App {
    let mut a = app();
    a.set_importer(Arc::new(BoxWorker(height)));
    let path = fcstd(kind);
    a.submit_command("import.file", json!({"path": path.to_string_lossy()}), Then::Notice);
    // The parse runs on its own thread: wait for it and for the commit.
    let start = std::time::Instant::now();
    a.wait_idle();
    while a.busy() && start.elapsed() < std::time::Duration::from_secs(60) {
        std::thread::sleep(std::time::Duration::from_millis(20));
        a.wait_idle();
    }
    a
}

#[test]
fn an_imperfect_history_asks_and_try_anyway_imports_it_with_undo() {
    let a = importing(12.0, "Part::Box");
    assert!(matches!(a.dialog, Some(Dialog::ImportHistory { .. })), "asks before committing");
    assert!(!a.document_text().contains("Import"), "nothing committed yet");
    let mut h = harness(a);
    h.run();
    h.get_by_label("The design history does not rebuild exactly");
    h.get_by_label_contains("1200");
    h.get_by_label("Import final shape only");
    h.get_by_label("Try importing with history anyway (you can undo)").click();
    h.run();
    h.state_mut().wait_idle();
    h.run();
    let text = h.state().document_text();
    assert!(text.contains("extrude"), "{text}");
    // The notice says the model differs and offers Undo, which takes the whole import back.
    h.get_by_label_contains("the model differs from the file");
    h.get_by_label("Undo").click();
    h.run();
    h.state_mut().wait_idle();
    let text = h.state().document_text();
    assert!(!text.contains("extrude") && !text.contains("fcstd"), "{text}");
}

#[test]
fn a_perfect_history_defaults_to_editable_history_and_can_be_the_final_shape() {
    let mut h = harness(importing(10.0, "Part::Box"));
    h.run();
    h.get_by_label("Import the design history?");
    h.get_by_label("Import with editable history");
    h.get_by_label("Import final shape only").click();
    h.run();
    h.state_mut().wait_idle();
    let text = h.state().document_text();
    assert!(text.contains("format: fcstd") && !text.contains("extrude"), "{text}");

    let mut h = harness(importing(10.0, "Part::Box"));
    h.run();
    h.get_by_label("Import with editable history").click();
    h.run();
    h.state_mut().wait_idle();
    assert!(h.state().document_text().contains("extrude"));
    assert!(h.state().import_undo.is_none(), "a perfect history needs no Undo offer");
}

#[test]
fn cancel_imports_nothing() {
    let mut h = harness(importing(12.0, "Part::Box"));
    h.run();
    h.get_by_label("Cancel").click();
    h.run();
    h.state_mut().wait_idle();
    assert!(h.state().dialog.is_none());
    assert!(!h.state().document_text().contains("fcstd"));
}

#[test]
fn a_history_that_cannot_be_read_is_said_and_the_final_shape_comes_in() {
    let a = importing(10.0, "Part::Feature");
    assert!(matches!(&a.dialog, Some(Dialog::Message { title, body })
        if title == "Importing the final shape only" && body.contains("no feature history")));
    assert!(a.document_text().contains("format: fcstd"));
}

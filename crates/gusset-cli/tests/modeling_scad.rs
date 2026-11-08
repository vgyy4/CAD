//! OpenSCAD import with the real OCCT kernel (ADR 0033): each top-level object becomes
//! editable features, or an exact body where features cannot say it, whose volume matches
//! the analytic value (true spheres, cones and tori, not facets), with transforms, mirrors
//! and booleans.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp, clippy::disallowed_methods)]

use gusset_commands::{CmdCtx, Session, execute};
use gusset_history::Actor;
use serde_json::json;
use std::f64::consts::PI;
use std::sync::Arc;

const SCAD: &str = r#"
// A plate with a through hole.
difference() {
  cube([40, 20, 5]);
  translate([20, 10, -1]) cylinder(h = 7, r = 3, $fn = 64);
}
translate([60, 0, 0]) sphere(d = 4);
translate([0, 40, 0]) cylinder(h = 10, r1 = 5, r2 = 0);
translate([0, 80, 0]) rotate_extrude() translate([10, 0]) circle(r = 2);
module wedge() { linear_extrude(height = 2, center = true) polygon([[0, 0], [6, 0], [0, 3]]); }
translate([100, 0, 0]) mirror([1, 0, 0]) rotate([90, 0, 0]) scale(2) wedge();
intersection() { cube(10, center = true); sphere(r = 6); }
for (i = [0 : 2]) translate([i * 10, -30, 0]) cube([2, 2, i + 1]);
translate([0, -60, 0]) rotate([20, 30, 40]) cube([5, 8, 3]);
rotate([180, 0, 0]) cylinder(h = 4, r = 1);
"#;

#[test]
fn an_openscad_file_becomes_exact_bodies() {
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let dir = std::env::temp_dir().join(format!("gusset-scad-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("parts.scad"), SCAD).unwrap();
    let doc = gusset_doc::Document::from_text("gusset 1\nunits mm\n").unwrap();
    let mut s = Session::new(&doc, k.clone(), &CmdCtx::user(0));
    s.project_dir = Some(dir.clone());
    let r = execute(&mut s, &CmdCtx::user(1), "import.file", json!({"path": "parts.scad"})).unwrap();
    assert_eq!(r["features"].as_array().unwrap().len(), 9, "{r}");
    // Six objects become editable features (ADR 0033 amendment); the mirrored wedge, which
    // a move cannot say, stays an exact body.
    // The last two are turned (any axis, and half a turn): their boxes match the exact
    // build's, or they would have stayed exact bodies.
    assert_eq!(
        r["scad_features"],
        json!(["Cube1", "Sphere1", "Cone1", "Revolve1", "Cube2", "Cube3", "Cube6", "Cylinder2"]),
        "{r}"
    );
    assert_eq!(r["warnings_user_text"].as_array().unwrap().len(), 1, "{r}");
    assert!(r["warnings_user_text"][0].as_str().unwrap().contains("Object 5"), "{r}");
    let ev = s.evaluate(&Actor::User).unwrap();
    assert!(ev.errors().is_empty(), "{:?}", ev.errors());
    let text = s.document(&Actor::User).unwrap().to_text();
    // Every sketch written is fully defined (dogfooding F17-4): a dimension edit resizes.
    let sketches: Vec<String> = s
        .document(&Actor::User)
        .unwrap()
        .features
        .iter()
        .filter(|f| matches!(f.kind, gusset_doc::FeatureKind::Sketch(_)))
        .map(|f| f.name.clone())
        .collect();
    assert!(!sketches.is_empty());
    for sk in &sketches {
        let q = execute(&mut s, &CmdCtx::user(1), "query.sketch", json!({"sketch": sk})).unwrap();
        assert_eq!(q["dof"], 0, "{sk}: {q}");
    }
    assert!(
        text.contains("combine Combine") && text.contains("move Place") && text.contains("revolve Sphere1"),
        "{text}"
    );
    // In the file's order: plate, sphere, cone, torus, wedge, rounded cube, steps.
    let order = ["Cube1", "Sphere1", "Cone1", "Revolve1", "Import1", "Cube2", "Cube3"];
    let body = |i: usize| ev.bodies.iter().find(|b| b.feature == order[i]).unwrap();
    let vols: Vec<f64> = (0..order.len()).map(|i| k.mass_properties(&body(i).shape).unwrap().volume).collect();
    let near = |i: usize, want: f64, tol: f64| {
        assert!((vols[i] - want).abs() < tol * want, "body {i}: {} vs {want}", vols[i]);
    };
    near(0, 40.0 * 20.0 * 5.0 - PI * 9.0 * 5.0, 1e-9);
    near(1, 4.0 / 3.0 * PI * 8.0, 1e-9);
    near(2, PI * 25.0 * 10.0 / 3.0, 1e-9);
    near(3, 2.0 * PI * 10.0 * PI * 4.0, 1e-9);
    // The wedge doubled: 12 x 6 triangle, 4 thick.
    near(4, 12.0 * 6.0 / 2.0 * 4.0, 1e-9);
    // A 10 mm cube with its corners rounded off by a sphere of radius 6: the sphere's
    // caps beyond the faces cut away (6 caps of height 1).
    let cap = PI * 1.0 * 1.0 * (3.0 * 6.0 - 1.0) / 3.0;
    near(5, 4.0 / 3.0 * PI * 216.0 - 6.0 * cap, 1e-6);
    near(6, 4.0 * (1.0 + 2.0 + 3.0), 1e-9);
    // Placement: the mirrored wedge lies at x <= 100, the sphere is centred at x = 60.
    let b = k.bbox(&body(4).shape).unwrap();
    assert!(b.max[0] <= 100.0 + 1e-6 && b.min[0] >= 100.0 - 12.0 - 1e-6, "{b:?}");
    let b = k.bbox(&body(1).shape).unwrap();
    assert!((b.min[0] + b.max[0] - 120.0).abs() < 1e-6, "{b:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bad_openscad_is_reported_with_its_line() {
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let dir = std::env::temp_dir().join(format!("gusset-scad-bad-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("bad.scad"), "cube(1);\nscale([1, 2, 1]) sphere(3);\n").unwrap();
    let doc = gusset_doc::Document::from_text("gusset 1\nunits mm\n").unwrap();
    let mut s = Session::new(&doc, k, &CmdCtx::user(0));
    s.project_dir = Some(dir.clone());
    // The object that cannot be built is skipped with its line and reason; the cube comes in
    // (dogfooding F17-2).
    let r = execute(&mut s, &CmdCtx::user(1), "import.file", json!({"path": "bad.scad"})).unwrap();
    assert_eq!(r["features"].as_array().unwrap().len(), 1, "{r}");
    let w = r["warnings_user_text"].to_string();
    assert!(w.contains("line 2") && w.contains("evenly"), "{r}");
    // Nothing buildable at all is an error that says why.
    std::fs::write(dir.join("worse.scad"), "scale([1, 2, 1]) sphere(3);\n").unwrap();
    let e = execute(&mut s, &CmdCtx::user(1), "import.file", json!({"path": "worse.scad"})).unwrap_err();
    assert!(e.message.contains("could be built") && e.details.unwrap().to_string().contains("evenly"), "{}", e.message);
    let _ = std::fs::remove_dir_all(&dir);
}

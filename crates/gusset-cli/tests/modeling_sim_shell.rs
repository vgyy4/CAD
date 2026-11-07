//! Sheet-metal bodies analysed on shell elements (ADR 0041 amendment) with the real kernel.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use gusset_commands::{CmdCtx, Session, execute};
use serde_json::{Value, json};
use std::sync::Arc;

const STRIP: &str = "gusset 1
units mm

sketch T (plane: XY, next_id: 9) {
  p1 = point(0, 0)
  p2 = point(100, 0)
  p3 = point(100, 10)
  p4 = point(0, 10)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}

tab Plate (profile: T, thickness: 1 mm)
";

fn session(src: &str) -> Session {
    let doc = gusset_doc::Document::from_text(src).unwrap_or_else(|e| panic!("{e}"));
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    Session::new(&doc, k, &CmdCtx::user(0))
}

fn run(s: &mut Session, id: &str, args: Value) -> Value {
    execute(s, &CmdCtx::user(1), id, args).unwrap_or_else(|e| panic!("{id}: {e:?}"))
}

#[test]
fn a_sheet_strip_bends_as_a_beam_on_shell_elements() {
    let mut s = session(STRIP);
    let r = run(
        &mut s,
        "sim.static",
        json!({"fixed": ["Plate/side(T/l8)"], "loads": [{"face": "Plate/side(T/l6)", "force": [0, 0, -10]}]}),
    );
    assert_eq!(r["method"], "shell", "{r}");
    // P L^3 / (3 E I), I = 10 * 1^3 / 12.
    let exact = 10.0 * 1e6 / (3.0 * 210_000.0 * 10.0 / 12.0);
    let d = r["max_displacement_mm"].as_f64().unwrap();
    assert!((d - exact).abs() < 0.05 * exact, "{d} vs {exact}");
    // Root stress M c / I = 10 * 100 * 0.5 / (10/12) = 600 MPa (element centres read lower).
    let vm = r["max_von_mises_mpa"].as_f64().unwrap();
    assert!(vm > 450.0 && vm < 650.0, "{vm}");
}

#[test]
fn a_bent_strip_is_one_shell_through_its_bend() {
    // The strip with a 90 degree flange up at x = 100, held at x = 0, pushed along -x at the
    // flange's top: the flange bends as a cantilever and the strip is pushed lengthwise.
    let src = format!(
        "{STRIP}\nedge_flange F1 (body: Plate, line: l6, length: 30 mm, angle: 90 deg, bend_radius: 1 mm, k_factor: 0.44)\n"
    );
    let mut s = session(&src);
    let r = run(
        &mut s,
        "sim.static",
        json!({"fixed": ["Plate/side(T/l8)"], "loads": [{"face": "F1/end(l6)", "force": [-1, 0, 0]}]}),
    );
    assert_eq!(r["method"], "shell", "{r}");
    // Beam theory, lever h = 31.5 from the strip's mid-plane, L = 100, EI = 210000 * 10/12:
    // across, the flange bends (P h^3 / 3EI) and turns with the strip's end (P h L / EI * h);
    // the strip's end rises by P h L^2 / 2EI. The bend's own give is left out.
    let (h, l, ei) = (31.5f64, 100.0f64, 210_000.0 * 10.0 / 12.0);
    let across = h * h * (h / 3.0 + l) / ei;
    let up = h * l * l / (2.0 * ei);
    let exact = (across * across + up * up).sqrt();
    let d = r["max_displacement_mm"].as_f64().unwrap();
    assert!((d - exact).abs() < 0.1 * exact, "{d} vs {exact}: {r}");
    let at = &r["max_displacement_at_mm"];
    assert!(at[2].as_f64().unwrap() > 30.0, "at the flange's top: {at}");
}

#[test]
fn a_sheet_strip_rings_at_its_beam_frequency() {
    let mut s = session(STRIP);
    let r = run(&mut s, "sim.modal", json!({"fixed": ["Plate/side(T/l8)"], "modes": 2, "voxel": 5, "shape": 1}));
    assert_eq!(r["method"], "shell", "{r}");
    // Cantilever, first bending: 1.8751^2 / (2 pi L^2) sqrt(E I / (rho A)).
    let (e, i, rho, a, l) = (210_000.0f64, 10.0f64 / 12.0, 7.85e-9f64, 10.0f64, 100.0f64);
    let exact = 1.8751f64.powi(2) / (2.0 * std::f64::consts::PI * l * l) * (e * i / (rho * a)).sqrt();
    let f1 = r["frequencies_hz"][0].as_f64().unwrap();
    assert!((f1 - exact).abs() < 0.03 * exact, "{f1} vs {exact}: {r}");
    // The free end swings across the sheet.
    let sc = &r["shape_scale"];
    assert!(sc["largest_at_mm"][0].as_f64().unwrap() > 95.0, "{sc}");
    assert!(sc["direction"][2].as_f64().unwrap().abs() > 0.9, "{sc}");
}

#[test]
fn a_bent_tab_solves_at_several_element_sizes() {
    // Dogfooding F8-1: thin facets where the bend meets the flats made folded elements at
    // most sizes. A 100 x 20 x 2 tab with a 30 mm flange, held at its far end, pushed at the
    // flange's top: every size solves and they agree.
    let src = "gusset 1
units mm

sketch T (plane: XY, next_id: 9) {
  p1 = point(0, 0)
  p2 = point(100, 0)
  p3 = point(100, 20)
  p4 = point(0, 20)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}

tab Plate (profile: T, thickness: 2 mm)

edge_flange F1 (body: Plate, line: l6, length: 30 mm, angle: 90 deg, bend_radius: 2 mm, k_factor: 0.44)
";
    let mut s = session(src);
    let mut seen = Vec::new();
    for h in [2.5, 3.0, 5.0] {
        let r = run(
            &mut s,
            "sim.static",
            json!({"fixed": ["Plate/side(T/l8)"], "loads": [{"face": "F1/end(l6)", "force": [-1, 0, 0]}], "voxel": h}),
        );
        seen.push(r["max_displacement_mm"].as_f64().unwrap());
    }
    let (lo, hi) = seen.iter().fold((f64::MAX, 0.0f64), |(a, b), d| (a.min(*d), b.max(*d)));
    assert!(hi - lo < 0.05 * hi, "{seen:?}");
}

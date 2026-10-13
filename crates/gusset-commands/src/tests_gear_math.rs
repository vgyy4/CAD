//! Involute gear geometry and the generated parameter expressions (ADR 0018), without a kernel.

use crate::gear::*;
use gusset_units::expr::parse;
use gusset_units::params::{self, Param};
use std::f64::consts::{FRAC_PI_2, PI};

fn values(module: f64, teeth: u32) -> GearValues {
    GearValues {
        module,
        teeth,
        pressure_angle: 20f64.to_radians(),
        shift: 0.0,
        backlash: 0.0,
        fillet: 0.38,
        helix: 0.0,
        face_width: 10.0,
        bore: 6.0,
    }
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

#[test]
fn standard_gear_dimensions() {
    let g = Geometry::new(&values(2.0, 20)).unwrap();
    assert!(near(g.pitch_radius, 20.0, 1e-12) && near(g.tip_radius, 22.0, 1e-12) && near(g.root_radius, 17.5, 1e-12));
    assert!(near(g.base_radius, 20.0 * 20f64.to_radians().cos(), 1e-12));
    // Tooth thickness on the pitch circle is pi m / 2 (x = 0): the involute point at the
    // pitch radius sits half a thickness off the tooth axis.
    let (r, th) = g.involute_polar(g.transverse_angle);
    assert!(near(r, 20.0, 1e-12));
    assert!(near(-2.0 * th * r, PI, 1e-12), "{}", -2.0 * th * r);
    // Positive shift thickens it by 2 x m tan(a), backlash thins it.
    let thick = Geometry::new(&GearValues { shift: 0.4, ..values(2.0, 20) }).unwrap();
    let (r, th) = thick.involute_polar(thick.transverse_angle);
    assert!(near(-2.0 * th * r, PI + 2.0 * 0.4 * 2.0 * 20f64.to_radians().tan(), 1e-12));
    let loose = Geometry::new(&GearValues { backlash: 0.1, ..values(2.0, 20) }).unwrap();
    let (r, th) = loose.involute_polar(loose.transverse_angle);
    assert!(near(-2.0 * th * r, PI - 0.1, 1e-12));
}

#[test]
fn fillet_meets_the_involute_and_the_root_circle_tangentially() {
    for v in [
        values(2.0, 20),
        values(1.0, 18),
        GearValues { shift: 0.4, ..values(3.0, 14) },
        GearValues { shift: -0.2, backlash: 0.05, ..values(2.0, 60) },
        GearValues { helix: 18f64.to_radians(), ..values(2.0, 24) },
        GearValues { pressure_angle: 25f64.to_radians(), fillet: 0.2, ..values(1.5, 30) },
        GearValues { fillet: 0.2, ..values(2.0, 40) },
    ] {
        let g = Geometry::new(&v).unwrap_or_else(|e| panic!("{v:?}: {e:?}"));
        // Same point where the two descriptions meet.
        let a = g.fillet_xy(g.transverse_angle);
        let b = g.involute_xy(g.flank_roll);
        assert!(near(a[0], b[0], 1e-9) && near(a[1], b[1], 1e-9), "{v:?}: {a:?} {b:?}");
        // Same direction there (no kink), judged by tiny steps on either side.
        let h = 1e-5;
        let before = g.fillet_xy(g.transverse_angle + h);
        let after = g.involute_xy(g.flank_roll + h);
        let d1 = [a[0] - before[0], a[1] - before[1]];
        let d2 = [after[0] - b[0], after[1] - b[1]];
        let cos = (d1[0] * d2[0] + d1[1] * d2[1]) / (d1[0].hypot(d1[1]) * d2[0].hypot(d2[1]));
        assert!(cos > 1.0 - 1e-6, "{v:?}: kink at the junction, cos {cos}");
        // The root end touches the root circle and runs along it.
        let (r_root, _) = g.fillet_polar(FRAC_PI_2);
        assert!(near(r_root, g.root_radius, 1e-9));
        let (r_near, _) = g.fillet_polar(FRAC_PI_2 - 1e-3);
        assert!(r_near - g.root_radius < 1e-4 * v.module, "{v:?}: {}", r_near - g.root_radius);
    }
}

#[test]
fn bad_inputs_have_hints() {
    for (v, why) in [
        (values(2.0, 8), "undercut"),
        (values(2.0, 5), "out of range"),
        (values(2.0, 501), "out of range"),
        (values(0.0, 20), "module"),
        (GearValues { pressure_angle: 1.5, ..values(2.0, 20) }, "pressure angle"),
        (GearValues { shift: 1.9, ..values(2.0, 20) }, "point"),
        (GearValues { fillet: 0.9, ..values(2.0, 20) }, "fillet"),
        (GearValues { backlash: 5.0, ..values(2.0, 20) }, "backlash"),
        (GearValues { helix: 1.2, ..values(2.0, 20) }, "helix"),
        (GearValues { module: f64::NAN, ..values(2.0, 20) }, "finite"),
    ] {
        let e = Geometry::new(&v).unwrap_err();
        assert!(e.message.contains(why) || e.hint.contains(why), "{why}: {e:?}");
        assert!(!e.hint.is_empty());
    }
    // The undercut hint names ways out that work.
    let v = values(2.0, 9);
    let e = Geometry::new(&v).unwrap_err();
    assert!(e.hint.contains("teeth") && e.hint.contains("shift"), "{e:?}");
    assert!(Geometry::new(&GearValues { teeth: min_teeth(&v), ..v }).is_ok());
    let x = min_shift(&v);
    assert!(Geometry::new(&GearValues { shift: x + 0.01, ..v }).is_ok(), "x {x}");
    // Review v0.2 L9: the pointed-tooth hint is readable and names what widens the tip.
    let e = Geometry::new(&GearValues { shift: 1.9, ..values(2.0, 20) }).unwrap_err();
    assert_eq!(e.hint, "use a smaller profile shift, less backlash or more teeth");
    // The bore must leave material below the root circle (radius 17.5 mm here).
    let v = values(2.0, 20);
    let g = Geometry::new(&v).unwrap();
    assert!(check_bore(&GearValues { bore: 34.0, ..v }, &g).is_ok());
    let e = check_bore(&GearValues { bore: 35.0, ..v }, &g).unwrap_err();
    assert!(e.message.contains("bore") && e.hint.contains("34.00"), "{e:?}");
    assert!(check_bore(&GearValues { bore: -1.0, ..v }, &g).is_err());
}

#[test]
fn flanks_stay_within_a_micron_of_the_exact_curves() {
    for v in [
        values(2.0, 20),
        values(0.5, 18),
        values(10.0, 19),
        GearValues { shift: 0.5, ..values(3.0, 12) },
        values(1.0, 100),
        values(2.0, 500),
        GearValues { shift: -0.3, ..values(2.0, 30) },
        GearValues { helix: 30f64.to_radians(), ..values(2.0, 40) },
    ] {
        let g = Geometry::new(&v).unwrap_or_else(|e| panic!("{v:?}: {e:?}"));
        let f = choose_flank(&g).unwrap();
        assert!(f.error <= FLANK_TOLERANCE, "{v:?}: error {} with {} points", f.error, f.points.len());
        eprintln!("m {} z {}: {} fit points, error {:.2e} mm", v.module, v.teeth, f.points.len(), f.error);
    }
    const { assert!(FLANK_TOLERANCE <= 0.001) };
}

/// The expressions written into the document give the same numbers as the Rust formulas.
#[test]
fn expressions_reproduce_the_numbers() {
    for (v, helical) in [
        (values(2.0, 20), false),
        (GearValues { shift: 0.3, backlash: 0.04, ..values(1.5, 18) }, false),
        (GearValues { helix: 20f64.to_radians(), shift: -0.1, ..values(2.0, 24) }, true),
    ] {
        let nm = Names::from_feature("Gear1");
        let g = Geometry::new(&v).unwrap_or_else(|e| panic!("{v:?}: {e:?}"));
        let flank = choose_flank(&g).unwrap();
        let mut ps: Vec<Param> = Vec::new();
        let mut add = |name: &str, src: String| {
            ps.push(Param { name: nm.n(name), expr: parse(&src).unwrap(), comment: String::new() });
        };
        add("module", format!("{} mm", v.module));
        add("teeth", format!("{}", v.teeth));
        add("pressure_angle", format!("{} rad", v.pressure_angle));
        add("shift", format!("{}", v.shift));
        add("backlash", format!("{} mm", v.backlash));
        add("fillet", format!("{}", v.fillet));
        if helical {
            add("helix", format!("{} rad", v.helix));
        }
        for (name, src, _) in derived_params(&nm, helical) {
            ps.push(Param { name, expr: parse(&src).unwrap_or_else(|e| panic!("{src}: {e}")), comment: String::new() });
        }
        let vals = params::evaluate(&ps);
        let get = |n: &str| vals[&nm.n(n)].as_ref().unwrap_or_else(|e| panic!("{n}: {e}")).value;
        assert!(near(get("pitch_diameter"), 2.0 * g.pitch_radius, 1e-9));
        assert!(near(get("base_diameter"), 2.0 * g.base_radius, 1e-9));
        assert!(near(get("tip_diameter"), 2.0 * g.tip_radius, 1e-9));
        assert!(near(get("root_diameter"), 2.0 * g.root_radius, 1e-9));
        assert!(near(get("tooth_angle"), g.tooth_angle, 1e-12));
        assert!(near(get("flank_roll"), g.flank_roll, 1e-9), "{} {}", get("flank_roll"), g.flank_roll);
        assert!(near(get("tip_roll"), g.tip_roll, 1e-12));
        let scope = |n: &str| vals.get(n).and_then(|r| r.as_ref().ok().copied());
        for p in &flank.points {
            let (re, ae) = point_exprs(&nm, p);
            let r = gusset_units::expr::eval(&parse(&re).unwrap(), &scope).unwrap_or_else(|e| panic!("{re}: {e}"));
            let a = gusset_units::expr::eval(&parse(&ae).unwrap(), &scope).unwrap_or_else(|e| panic!("{ae}: {e}"));
            assert!(near(r.value, p.radius, 1e-9), "{p:?}: {} vs {}", r.value, p.radius);
            assert!(near(a.value, p.angle, 1e-9), "{p:?}: {} vs {}", a.value, p.angle);
        }
    }
}

#[test]
fn names_follow_the_feature() {
    assert_eq!(Names::from_feature("Gear1").n("module"), "gear1_module");
    assert_eq!(Names::from_feature("My-Gear").n("teeth"), "my_gear_teeth");
}

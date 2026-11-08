#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use super::*;
use crate::tests::box_mesh;

fn steel() -> Material {
    Material { young: 210_000.0, poisson: 0.3 }
}

fn cantilever(keep: f64, iterations: usize) -> Result<TopoResult, FeaError> {
    // 60 long (x), 10 deep (y), 20 high (z): held at x = 0, pushed down at x = 60.
    let m = box_mesh([60.0, 10.0, 20.0]);
    optimise(&TopoProblem {
        mesh: &m,
        voxel: 2.5,
        fixed: vec![0],
        loads: vec![Load { face: 1, force: [0.0, 0.0, -100.0] }],
        material: steel(),
        keep,
        iterations,
    })
}

#[test]
fn a_cantilever_keeps_its_share_and_gets_much_stiffer_than_a_uniform_one() {
    let r = cantilever(0.4, 40).unwrap();
    let (first, last) = (r.compliance[0], *r.compliance.last().unwrap());
    // The uniform start is soft (density 0.4 to the third); the layout carries the load.
    assert!(last < 0.3 * first, "{:?}", r.compliance);
    assert!((0.3..=0.55).contains(&r.kept_share), "{}", r.kept_share);
    assert!((r.kept_volume - r.kept_share * 12_000.0).abs() < 1e-6);
    assert!(!r.faces.is_empty());
    // Material goes to the top and bottom fibres near the support, where bending peaks: the
    // kept faces there outnumber those along the beam's middle height.
    let near = |z: f64| r.faces.iter().filter(|f| f.iter().all(|p| p[0] < 15.0 && (p[2] - z).abs() < 2.6)).count();
    let (fibres, middle) = (near(0.0) + near(20.0), near(10.0));
    assert!(fibres > middle, "fibres {fibres}, middle {middle}");
}

#[test]
fn bad_input_is_refused() {
    for (keep, its) in [(0.0, 10), (0.99, 10), (f64::NAN, 10), (0.4, 0), (0.4, 201)] {
        assert!(matches!(cantilever(keep, its), Err(FeaError::Invalid(_))), "{keep} {its}");
    }
}

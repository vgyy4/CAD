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
        symmetry: [false; 3],
        min_member: None,
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

#[test]
fn a_symmetric_layout_is_its_own_mirror_image() {
    // A beam held at both ends (x = 0 and 60), pushed down in the middle of its top: with
    // symmetry about the middle across X, every kept face has its mirror.
    let m = box_mesh([60.0, 10.0, 20.0]);
    let r = optimise(&TopoProblem {
        mesh: &m,
        voxel: 2.5,
        fixed: vec![0, 1],
        loads: vec![Load { face: 5, force: [0.0, 0.0, -100.0] }],
        material: steel(),
        keep: 0.4,
        iterations: 20,
        symmetry: [true, false, false],
        min_member: None,
    })
    .unwrap();
    let key = |f: &[[f64; 3]; 4], mirror: bool| {
        let mut c: Vec<[i64; 3]> = f
            .iter()
            .map(|p| [if mirror { 60.0 - p[0] } else { p[0] }, p[1], p[2]].map(|v| (v * 100.0).round() as i64))
            .collect();
        c.sort_unstable();
        c
    };
    let all: std::collections::HashSet<Vec<[i64; 3]>> = r.faces.iter().map(|f| key(f, false)).collect();
    assert!(r.faces.iter().all(|f| all.contains(&key(f, true))), "not symmetric");
}

#[test]
fn a_larger_minimum_member_gives_fewer_holes() {
    // Thicker members: fewer separate voids, so fewer outer faces for the same share.
    let m = box_mesh([60.0, 10.0, 20.0]);
    let run = |min_member| {
        optimise(&TopoProblem {
            mesh: &m,
            voxel: 2.5,
            fixed: vec![0],
            loads: vec![Load { face: 1, force: [0.0, 0.0, -100.0] }],
            material: steel(),
            keep: 0.4,
            iterations: 25,
            symmetry: [false; 3],
            min_member,
        })
        .unwrap()
    };
    let (fine, coarse) = (run(None), run(Some(12.5)));
    assert!(coarse.faces.len() < fine.faces.len(), "{} vs {}", coarse.faces.len(), fine.faces.len());
    assert!(matches!(
        optimise(&TopoProblem {
            min_member: Some(-1.0),
            ..TopoProblem {
                mesh: &m,
                voxel: 2.5,
                fixed: vec![0],
                loads: vec![Load { face: 1, force: [0.0, 0.0, -1.0] }],
                material: steel(),
                keep: 0.4,
                iterations: 1,
                symmetry: [false; 3],
                min_member: None
            }
        }),
        Err(FeaError::Invalid(_))
    ));
}

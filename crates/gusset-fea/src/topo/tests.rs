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
    assert!((r.kept_share - 0.4).abs() < 0.02, "{}", r.kept_share);
    assert!((r.kept_volume - r.kept_share * 12_000.0).abs() < 1e-6);
    assert!(!r.faces.is_empty());
    // Material goes to the top and bottom fibres near the support, where bending peaks: the
    // kept faces there outnumber those along the beam's middle height.
    let near = |z: f64| r.faces.iter().filter(|f| f.iter().all(|p| p[0] < 15.0 && (p[2] - z).abs() < 2.6)).count();
    let (fibres, middle) = (near(0.0) + near(20.0), near(10.0));
    assert!(fibres > middle, "fibres {fibres}, middle {middle}");
}

#[test]
fn the_kept_share_is_the_asked_one_even_before_convergence() {
    for (keep, its) in [(0.2, 30), (0.95, 5), (0.4, 3)] {
        let r = cantilever(keep, its).unwrap();
        assert!((r.kept_share - keep).abs() < 0.03, "keep {keep}: {}", r.kept_share);
    }
}

#[test]
fn a_mirror_the_load_case_does_not_follow_is_flagged() {
    let m = box_mesh([60.0, 10.0, 20.0]);
    let r = optimise(&TopoProblem {
        mesh: &m,
        voxel: 2.5,
        fixed: vec![0],
        loads: vec![Load { face: 1, force: [0.0, 0.0, -100.0] }],
        material: steel(),
        keep: 0.4,
        iterations: 2,
        symmetry: [true, true, false],
        min_member: None,
    })
    .unwrap();
    // Held at x = 0, pushed at x = 60: not mirrored in x; both ends span the whole y.
    assert_eq!(r.asymmetric, [true, false, false]);
}

#[test]
fn loads_that_push_nothing_are_refused() {
    let m = box_mesh([60.0, 10.0, 20.0]);
    let p = |fixed: Vec<usize>, force: [f64; 3]| TopoProblem {
        mesh: &m,
        voxel: 2.5,
        fixed,
        loads: vec![Load { face: 1, force }],
        material: steel(),
        keep: 0.4,
        iterations: 5,
        symmetry: [false; 3],
        min_member: None,
    };
    assert!(matches!(optimise(&p(vec![0], [0.0; 3])), Err(FeaError::Invalid(_))));
    assert!(matches!(optimise(&p(vec![0, 1], [0.0, 0.0, -100.0])), Err(FeaError::Invalid(_))));
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

#[test]
fn smoothing_rounds_a_voxel_staircase_without_shrinking_it() {
    let r = cantilever(0.4, 15).unwrap();
    let mut faces = r.faces.clone();
    smooth_faces(&mut faces, 10);
    assert_eq!(faces.len(), r.faces.len());
    // Bounding boxes stay close (Taubin does not shrink), but corners moved.
    let bbox = |f: &[[[f64; 3]; 4]]| {
        f.iter().flatten().fold(([f64::MAX; 3], [f64::MIN; 3]), |(lo, hi), p| {
            ([lo[0].min(p[0]), lo[1].min(p[1]), lo[2].min(p[2])], [hi[0].max(p[0]), hi[1].max(p[1]), hi[2].max(p[2])])
        })
    };
    let (a, b) = (bbox(&r.faces), bbox(&faces));
    for i in 0..3 {
        assert!((a.0[i] - b.0[i]).abs() < 2.5 && (a.1[i] - b.1[i]).abs() < 2.5, "{a:?} {b:?}");
    }
    assert!(faces.iter().zip(&r.faces).any(|(x, y)| x != y));
    assert!(faces.iter().flatten().flatten().all(|v| v.is_finite()));
}

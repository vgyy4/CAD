// Test-only code (see the note at the top of profiles.rs).
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

//! The arrangement's work with many splines (security review V2): close splines fail
//! cleanly and quickly instead of taking hours, and crossing splines give exactly the
//! arrangement they gave before the spatial index.

use gusset_sketch::{ProfileError, Sketch, arrange};
use std::time::{Duration, Instant};

const TOL: f64 = 1e-6;

/// A sine through `n` points over 0..`len` mm, amplitude `amp`, shifted by `phase` and `dy`.
fn sine(s: &mut Sketch, n: usize, len: f64, amp: f64, phase: f64, dy: f64) {
    let ids: Vec<_> = (0..n)
        .map(|i| {
            let x = len * i as f64 / (n - 1) as f64;
            s.add_point([x, dy + amp * libm::sin(x / 7.0 + phase)]).unwrap()
        })
        .collect();
    s.add_spline(&ids, false).unwrap();
}

#[test]
fn close_dense_splines_fail_cleanly_and_quickly() {
    // The review's case: a 1,000-point spline and copies 0.0001 mm apart (about 5 s for 5
    // copies and 106 s for 20 before the work limit; hours for a 500-copy pattern).
    let mut s = Sketch::new();
    for k in 0..20 {
        sine(&mut s, 1000, 500.0, 10.0, 0.0, 1e-4 * f64::from(k));
    }
    let t = Instant::now();
    let r = arrange(&s, TOL);
    let took = t.elapsed();
    eprintln!("20 close splines: {took:?}");
    match r {
        Err(e @ ProfileError::TooMuchWork { .. }) => assert!(e.to_string().contains("too close"), "{e}"),
        other => panic!("{:?}", other.map(|a| a.profiles.len())),
    }
    let budget = if cfg!(debug_assertions) { Duration::from_secs(20) } else { Duration::from_secs(3) };
    assert!(took < budget, "took {took:?}");
}

#[test]
fn parallel_dense_splines_are_fine() {
    // Six 1,000-point splines 1 mm apart (a channel or a set of grooves) are ordinary work.
    let mut s = Sketch::new();
    for k in 0..6 {
        sine(&mut s, 1000, 500.0, 10.0, 0.0, f64::from(k));
    }
    let t = Instant::now();
    let a = arrange(&s, TOL).unwrap();
    eprintln!("6 parallel splines: {:?}", t.elapsed());
    assert!(a.profiles.is_empty() && !a.crossings);
}

/// FNV-1a of the arrangement's exact output: every profile's area, label, pieces and
/// segment count (bit for bit), and the crossing flag.
fn fingerprint(s: &Sketch) -> (usize, u64) {
    let a = arrange(s, TOL).unwrap();
    let mut text = format!("{}", a.crossings);
    for p in &a.profiles {
        let segs = p.outer.segments.len() + p.holes.iter().map(|h| h.segments.len()).sum::<usize>();
        text.push_str(&format!(
            "|{:x} {:x} {:x} {:?} {segs}",
            p.area.to_bits(),
            p.label[0].to_bits(),
            p.label[1].to_bits(),
            p.pieces
        ));
    }
    let hash = text.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    (a.profiles.len(), hash)
}

#[test]
fn crossing_splines_arrange_as_before() {
    // Six 60-point sines with different phases across a 300 x 40 mm frame.
    let mut s = Sketch::new();
    let c = [[-5.0, -20.0], [305.0, -20.0], [305.0, 20.0], [-5.0, 20.0]];
    let ids: Vec<_> = c.iter().map(|&p| s.add_point(p).unwrap()).collect();
    for i in 0..4 {
        s.add_line_between(ids[i], ids[(i + 1) % 4]).unwrap();
    }
    for k in 0..6 {
        sine(&mut s, 60, 300.0, 12.0, f64::from(k) * 0.9, 0.0);
    }
    // Recorded with the arrangement before the spatial index and the work limit.
    assert_eq!(fingerprint(&s), (201, 0xc373_d816_8907_a2e8));
}

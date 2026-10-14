//! The BSplCLib port against the C++ original: `bsplclib.txt` was printed by
//! `oracle/bsplclib.cpp` (OCCT 8.0.1). B-spline evaluation uses only `+ - * /`, so
//! every value must be identical, bit for bit.

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use occt_port::bsplclib::{self, Curve};
use occt_port::gp::{Pnt, Vec};

fn f(word: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(word, 16).unwrap_or_else(|e| panic!("{word}: {e}")))
}

fn push_p(out: &mut std::vec::Vec<f64>, p: &Pnt) {
    let x = p.xyz();
    out.extend([x.x(), x.y(), x.z()]);
}

fn push_v(out: &mut std::vec::Vec<f64>, v: &Vec) {
    let x = v.xyz();
    out.extend([x.x(), x.y(), x.z()]);
}

#[test]
fn bsplclib_matches_occt_bit_for_bit() {
    let mut cases = 0;
    for line in include_str!("bsplclib.txt").lines() {
        let (input, want) = line.split_once(" | ").unwrap();
        let mut w = input.split_whitespace();
        assert_eq!(w.next(), Some("bspl"));
        let mut int = || w.next().unwrap().parse::<i32>().unwrap();
        let (degree, periodic, rational, nk, np) = (int(), int() == 1, int() == 1, int(), int());
        let (nk, np) = (nk as usize, np as usize);
        let rest: std::vec::Vec<&str> = w.collect();
        let knots: std::vec::Vec<f64> = rest[..nk].iter().map(|s| f(s)).collect();
        let mults: std::vec::Vec<i32> = rest[nk..2 * nk].iter().map(|s| s.parse().unwrap()).collect();
        let mut at = 2 * nk;
        let poles: std::vec::Vec<Pnt> = (0..np)
            .map(|i| {
                let c = &rest[at + 3 * i..at + 3 * i + 3];
                Pnt::new(f(c[0]), f(c[1]), f(c[2]))
            })
            .collect();
        at += 3 * np;
        let weights: Option<std::vec::Vec<f64>> = rational.then(|| {
            let ws = rest[at..at + np].iter().map(|s| f(s)).collect();
            at += np;
            ws
        });
        let u = f(rest[at]);
        assert_eq!(at + 1, rest.len(), "{line}");

        let curve =
            Curve { degree, periodic, poles: &poles, weights: weights.as_deref(), knots: &knots, mults: &mults };
        let mut want = want.split_whitespace();
        let index: i32 = want.next().unwrap().parse().unwrap();
        let want: std::vec::Vec<f64> = want.map(f).collect();

        let (got_index, new_u) = bsplclib::locate_parameter(degree, &knots, &mults, u, periodic);
        assert_eq!(got_index, index, "LocateParameter index\n{line}");
        let mut got = vec![new_u];
        push_p(&mut got, &bsplclib::d0(u, &curve));
        let (p, v1) = bsplclib::d1(u, &curve);
        push_p(&mut got, &p);
        push_v(&mut got, &v1);
        let (p, v1, v2) = bsplclib::d2(u, &curve);
        push_p(&mut got, &p);
        [v1, v2].iter().for_each(|v| push_v(&mut got, v));
        let (p, v1, v2, v3) = bsplclib::d3(u, &curve);
        push_p(&mut got, &p);
        [v1, v2, v3].iter().for_each(|v| push_v(&mut got, v));
        for n in 1..=4 {
            push_v(&mut got, &bsplclib::dn(u, n, &curve));
        }
        assert_eq!(want.len(), got.len(), "{line}");
        for (i, (a, b)) in want.iter().zip(&got).enumerate() {
            assert!(a.to_bits() == b.to_bits(), "value {i}: want {a:e}, got {b:e}\n{line}");
        }
        cases += 1;
    }
    assert!(cases >= 400, "{cases}");
}

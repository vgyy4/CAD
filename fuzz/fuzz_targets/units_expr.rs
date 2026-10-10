//! Unit expression parser (`gusset_units::expr`). Must never panic on any input, and
//! everything it accepts must print canonically: `parse(print(e))` succeeds and yields
//! the same tree (modulo spans, `Expr::same_tree`), as documented on `Display for Expr`. Evaluating
//! anything accepted must not panic either.
#![no_main]

use gusset_units::Quantity;
use gusset_units::expr::{self, ExprErrorKind, MAX_EXPR_LEN};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(src) = std::str::from_utf8(data) else { return };
    let Ok(e) = expr::parse(src) else { return };
    // Every variable resolves, alternating dimensions, so evaluation exercises every
    // operator and the dimension checks.
    let scope =
        |name: &str| Some(if name.len().is_multiple_of(2) { Quantity::mm(1.5) } else { Quantity::number(-2.0) });
    let _ = expr::eval(&e, &scope);
    let printed = e.to_string();
    match expr::parse(&printed) {
        Ok(e2) => {
            assert!(e2.same_tree(&e), "{src:?} printed as {printed:?}, which parses to a different tree");
            assert_eq!(e2.to_string(), printed, "printing is not canonical for {src:?}");
        }
        // Canonical printing adds spaces, so a near-limit input can print past the limit.
        Err(err) if err.kind == ExprErrorKind::TooComplex && printed.len() > MAX_EXPR_LEN => {}
        Err(err) => panic!("{src:?} parsed, but its printed form {printed:?} did not: {err}"),
    }
});

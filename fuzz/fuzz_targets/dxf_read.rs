//! `dxf_read::read` must never panic, hang or allocate without bound on any input, and
//! everything it returns must be finite and within its caps.
#![no_main]

use gusset_drawing::dxf_read::{self, Curve, MAX_CURVES};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(r) = dxf_read::read(data) {
        assert!(r.curves.len() <= MAX_CURVES + 1);
        for c in &r.curves {
            let ok = match c {
                Curve::Line(a, b) => a.iter().chain(b).all(|v| v.is_finite()),
                Curve::Circle { center, radius } => center.iter().all(|v| v.is_finite()) && radius.is_finite(),
                Curve::Arc { center, radius, start, end } => {
                    center.iter().all(|v| v.is_finite()) && radius.is_finite() && start.is_finite() && end.is_finite()
                }
            };
            assert!(ok, "{c:?}");
        }
    }
});

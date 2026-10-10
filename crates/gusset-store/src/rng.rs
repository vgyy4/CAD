//! Small deterministic PRNG (SplitMix64) for the fault simulator, plus OS randomness.

use std::io;

/// SplitMix64: tiny, fast, good enough for test scheduling. Not cryptographic.
#[derive(Debug, Clone)]
pub(crate) struct SplitMix64(u64);

impl SplitMix64 {
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform-ish value in `0..n` (`n > 0`; returns 0 for `n == 0`).
    pub(crate) fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next_u64() % n }
    }

    /// True with probability `num / den`.
    pub(crate) fn chance(&mut self, num: u64, den: u64) -> bool {
        self.below(den) < num
    }

    /// Fisher-Yates shuffle.
    pub(crate) fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = usize::try_from(self.below(i as u64 + 1)).unwrap_or(0);
            v.swap(i, j);
        }
    }
}

/// 64 bits from the OS CSPRNG.
pub(crate) fn os_random_u64() -> io::Result<u64> {
    getrandom::u64().map_err(|e| io::Error::other(format!("OS random number generator failed: {e}")))
}

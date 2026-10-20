//! `math_BullardGenerator`: fast random number generator (the algorithm proposed by Ian C. Bullard).

/// `math_BullardGenerator`.
#[derive(Clone, Debug)]
pub struct BullardGenerator {
    state_hi: u32,
    state_lo: u32,
}

impl Default for BullardGenerator {
    fn default() -> Self {
        Self::new(1)
    }
}

impl BullardGenerator {
    /// `math_BullardGenerator(theSeed = 1)`.
    pub fn new(seed: u32) -> Self {
        let mut g = Self { state_hi: seed, state_lo: 0 };
        g.set_seed(seed);
        g
    }

    /// `SetSeed`.
    pub fn set_seed(&mut self, seed: u32) {
        self.state_hi = seed;
        self.state_lo = seed ^ 0x4961_6E42;
    }

    /// `NextInt`.
    pub fn next_int(&mut self) -> u32 {
        self.state_hi = (self.state_hi >> 2).wrapping_add(self.state_hi << 2);
        self.state_hi = self.state_hi.wrapping_add(self.state_lo);
        self.state_lo = self.state_lo.wrapping_add(self.state_hi);
        self.state_hi
    }

    /// `NextReal`.
    pub fn next_real(&mut self) -> f64 {
        f64::from(self.next_int()) / f64::from(0xFFFF_FFFF_u32)
    }
}

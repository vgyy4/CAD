//! `math_PSOParticlesPool` and `PSO_Particle`.
use crate::mathlin::MathError;

/// `PSO_Particle`.
#[derive(Clone, Debug, PartialEq)]
pub struct PsoParticle {
    /// `Position`.
    pub position: Vec<f64>,
    /// `Velocity`.
    pub velocity: Vec<f64>,
    /// `BestPosition`.
    pub best_position: Vec<f64>,
    /// `Distance`.
    pub distance: f64,
    /// `BestDistance`.
    pub best_distance: f64,
}

/// `math_PSOParticlesPool`.
#[derive(Clone, Debug)]
pub struct PsoParticlesPool {
    particles: Vec<PsoParticle>,
    dimension_count: i32,
}

impl PsoParticlesPool {
    /// `math_PSOParticlesPool(ParticlesCount, DimensionCount)`.
    pub fn new(particles_count: i32, dimension_count: i32) -> Result<Self, MathError> {
        if particles_count < 1 || dimension_count < 1 {
            return Err(MathError::Range);
        }
        let d = usize::try_from(dimension_count).map_err(|_| MathError::Range)?;
        let one = PsoParticle {
            position: vec![0.0; d],
            velocity: vec![0.0; d],
            best_position: vec![0.0; d],
            distance: f64::MAX,
            best_distance: f64::MAX,
        };
        let n = usize::try_from(particles_count).map_err(|_| MathError::Range)?;
        Ok(Self { particles: vec![one; n], dimension_count })
    }

    /// Number of particles.
    pub fn nb_particles(&self) -> i32 {
        i32::try_from(self.particles.len()).unwrap_or(i32::MAX)
    }

    /// Dimension of the particles.
    pub fn nb_dimensions(&self) -> i32 {
        self.dimension_count
    }

    fn slot(&self, index: i32) -> Result<usize, MathError> {
        if index < 1 || index > self.nb_particles() {
            return Err(MathError::Range);
        }
        usize::try_from(index - 1).map_err(|_| MathError::Range)
    }

    /// `GetParticle(index)`.
    pub fn get_particle(&self, index: i32) -> Result<&PsoParticle, MathError> {
        let k = self.slot(index)?;
        self.particles.get(k).ok_or(MathError::Range)
    }

    /// `GetParticle(index)`, mutable.
    pub fn get_particle_mut(&mut self, index: i32) -> Result<&mut PsoParticle, MathError> {
        let k = self.slot(index)?;
        self.particles.get_mut(k).ok_or(MathError::Range)
    }

    /// `GetBestParticle`: 1-based index of the first particle with the smallest distance.
    pub fn get_best_particle(&self) -> i32 {
        let mut best = 0;
        for (k, p) in self.particles.iter().enumerate() {
            if let Some(b) = self.particles.get(best)
                && p.distance < b.distance
            {
                best = k;
            }
        }
        i32::try_from(best + 1).unwrap_or(i32::MAX)
    }

    /// `GetWorstParticle`: 1-based index of the first particle with the largest distance.
    pub fn get_worst_particle(&self) -> i32 {
        let mut worst = 0;
        for (k, p) in self.particles.iter().enumerate() {
            if let Some(w) = self.particles.get(worst)
                && w.distance < p.distance
            {
                worst = k;
            }
        }
        i32::try_from(worst + 1).unwrap_or(i32::MAX)
    }
}

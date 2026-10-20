//! `math_PSO`: Particle Swarm Optimization.
#![allow(clippy::float_cmp)] // exact comparisons with the clamping borders, as in the C++
use super::bullard::BullardGenerator;
use super::pso_pool::PsoParticlesPool;
use crate::mathlin::{MathError, Vector};
use crate::mathnd::MultipleVarFunction;

const BORDER_DIVISOR: f64 = 1.0e+4;

/// `Vector(i)` read; the indices used here are always inside the vectors.
fn at(v: &Vector, i: i32) -> f64 {
    v.value(i).unwrap_or(0.0)
}

fn put(v: &mut Vector, i: i32, x: f64) {
    let _ = v.set_value(i, x);
}

/// `std::min(a, b)`.
fn min(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

/// `std::max(a, b)`.
fn max(a: f64, b: f64) -> f64 {
    if a < b { b } else { a }
}

fn slot(v: &mut [f64], i: usize) -> Option<&mut f64> {
    v.get_mut(i)
}

/// `math_PSO`.
#[derive(Clone, Debug)]
pub struct Pso {
    low_border: Vector,
    upp_border: Vector,
    steps: Vector,
    n: i32,
    nb_particles: i32,
    nb_iter: i32,
}

impl Pso {
    /// `math_PSO(F, Low, Upp, Steps, NbParticles, NbIter)`.
    pub fn new(
        f: &dyn MultipleVarFunction,
        low: &Vector,
        upp: &Vector,
        steps: &Vector,
        nb_particles: i32,
        nb_iter: i32,
    ) -> Result<Self, MathError> {
        let n = f.nb_variables();
        let mut s = Self {
            low_border: Vector::new(1, n)?,
            upp_border: Vector::new(1, n)?,
            steps: Vector::new(1, n)?,
            n,
            nb_particles,
            nb_iter,
        };
        s.low_border.initialized(low)?;
        s.upp_border.initialized(upp)?;
        s.steps.initialized(steps)?;
        Ok(s)
    }

    fn borders(&self) -> Result<(Vector, Vector), MathError> {
        let range = self.upp_border.subtracted(&self.low_border)?;
        let min_uv = self.low_border.added(&range.divided(BORDER_DIVISOR)?)?;
        let range = self.upp_border.subtracted(&self.low_border)?;
        let max_uv = self.upp_border.subtracted(&range.divided(BORDER_DIVISOR)?)?;
        Ok((min_uv, max_uv))
    }

    /// `Perform(Steps, Value, OutPnt, NbIter)`: the particles are constructed inside.
    pub fn perform(
        &mut self,
        f: &mut dyn MultipleVarFunction,
        steps: &Vector,
        out: &mut Vector,
        nb_iter: i32,
    ) -> Result<f64, MathError> {
        let n = self.n;
        if steps.length() != n || out.length() != n || f.nb_variables() != n {
            return Err(MathError::Dimension);
        }
        if self.nb_particles < 1 {
            return Err(MathError::Range);
        }
        let (min_uv, max_uv) = self.borders()?;
        self.nb_iter = nb_iter;
        self.steps.initialized(steps)?;

        let mut pool = PsoParticlesPool::new(self.nb_particles, n)?;

        let mut is_regular_grid_finished = false;
        let mut curr_value = 0.0;
        let mut curr_point = Vector::new(1, n)?;
        let mut particle = pool.get_worst_particle();
        curr_point.initialized(&min_uv)?;
        loop {
            if let Some(v) = f.value(&curr_point) {
                curr_value = v;
            }

            let worst_distance = pool.get_particle(particle)?.distance;
            if curr_value < worst_distance {
                let p = pool.get_particle_mut(particle)?;
                for d in 0..n {
                    let x = at(&curr_point, d + 1);
                    if let Some(s) = slot(&mut p.position, d as usize) {
                        *s = x;
                    }
                    if let Some(s) = slot(&mut p.best_position, d as usize) {
                        *s = x;
                    }
                }
                p.distance = curr_value;
                p.best_distance = curr_value;

                particle = pool.get_worst_particle();
            }

            // Step.
            let step0 = max(at(&self.steps, 1), 1.0e-15);
            let x1 = at(&curr_point, 1) + step0;
            put(&mut curr_point, 1, x1);
            for d in 1..n {
                if at(&curr_point, d) > at(&max_uv, d) {
                    put(&mut curr_point, d, at(&min_uv, d));
                    let x = at(&curr_point, d + 1) + at(&self.steps, d + 1);
                    put(&mut curr_point, d + 1, x);
                } else {
                    break;
                }
            }

            // Stop criteria.
            if at(&curr_point, n) > at(&max_uv, n) {
                is_regular_grid_finished = true;
            }
            if is_regular_grid_finished {
                break;
            }
        }

        let nb = self.nb_particles;
        self.perform_pso_with_given_particles(f, &mut pool, nb, out, nb_iter)
    }

    /// `Perform(Particles, NbParticles, Value, OutPnt, NbIter)`.
    pub fn perform_with_pool(
        &mut self,
        f: &mut dyn MultipleVarFunction,
        pool: &mut PsoParticlesPool,
        nb_particles: i32,
        out: &mut Vector,
        nb_iter: i32,
    ) -> Result<f64, MathError> {
        if out.length() != self.n || f.nb_variables() != self.n || pool.nb_dimensions() != self.n {
            return Err(MathError::Dimension);
        }
        if nb_particles > pool.nb_particles() {
            return Err(MathError::Range);
        }
        self.perform_pso_with_given_particles(f, pool, nb_particles, out, nb_iter)
    }

    fn perform_pso_with_given_particles(
        &mut self,
        f: &mut dyn MultipleVarFunction,
        particles: &mut PsoParticlesPool,
        nb_particles: i32,
        out: &mut Vector,
        nb_iter: i32,
    ) -> Result<f64, MathError> {
        let n = self.n;
        let (min_uv, max_uv) = self.borders()?;
        self.nb_iter = nb_iter;
        self.nb_particles = nb_particles;

        let mut curr_point = Vector::new(1, n)?;
        let mut best_global_position = Vector::new(1, n)?;

        // Generate initial particle velocities.
        let mut random = BullardGenerator::default();
        for part in 1..=self.nb_particles {
            let p = particles.get_particle_mut(part)?;
            for d in 1..=n {
                let ksi = random.next_real();
                if let Some(s) = slot(&mut p.velocity, (d - 1) as usize) {
                    *s = at(&self.steps, d) * (ksi - 0.5) * 2.0;
                }
            }
        }

        let best = particles.get_best_particle();
        let p = particles.get_particle(best)?;
        for d in 0..n {
            put(&mut best_global_position, d + 1, p.position.get(d as usize).copied().unwrap_or(0.0));
        }
        let mut best_global_distance = p.distance;

        // This velocity is used for detecting stagnation state.
        let termination_velocity = self.steps.divided(2048.0)?;
        // This velocity shows minimal velocity on current step.
        let mut minimal_velocity = Vector::new(1, n)?;

        // Run PSO iterations.
        let mut step = 1;
        while step < self.nb_iter {
            minimal_velocity.init(f64::MAX);

            for part in 1..=self.nb_particles {
                let ksi1 = random.next_real();
                let ksi2 = random.next_real();

                let p = particles.get_particle_mut(part)?;

                let retent_weight = 0.72900;
                let person_weight = 1.49445;
                let social_weight = 1.49445;

                for d in 0..n {
                    let k = d as usize;
                    let (Some(vel), Some(pos), Some(bpos)) =
                        (p.velocity.get(k).copied(), p.position.get(k).copied(), p.best_position.get(k).copied())
                    else {
                        return Err(MathError::Dimension);
                    };
                    let vel = vel * retent_weight
                        + (bpos - pos) * (person_weight * ksi1)
                        + (at(&best_global_position, d + 1) - pos) * (social_weight * ksi2);
                    let mut pos = pos + vel;
                    pos = min(max(pos, at(&min_uv, d + 1)), at(&max_uv, d + 1));
                    if let Some(s) = slot(&mut p.velocity, k) {
                        *s = vel;
                    }
                    if let Some(s) = slot(&mut p.position, k) {
                        *s = pos;
                    }
                    put(&mut curr_point, d + 1, pos);

                    let mv = min(vel.abs(), at(&minimal_velocity, d + 1));
                    put(&mut minimal_velocity, d + 1, mv);
                }

                if let Some(v) = f.value(&curr_point) {
                    p.distance = v;
                }
                if p.distance < p.best_distance {
                    p.best_distance = p.distance;
                    for d in 0..n {
                        let k = d as usize;
                        let x = p.position.get(k).copied().unwrap_or(0.0);
                        if let Some(s) = slot(&mut p.best_position, k) {
                            *s = x;
                        }
                    }

                    if p.distance < best_global_distance {
                        best_global_distance = p.distance;
                        for d in 0..n {
                            put(&mut best_global_position, d + 1, p.position.get(d as usize).copied().unwrap_or(0.0));
                        }
                    }
                }
            }

            let mut is_terminal_velocity_reached = true;
            for d in 1..=n {
                if at(&minimal_velocity, d) > at(&termination_velocity, d) {
                    is_terminal_velocity_reached = false;
                    break;
                }
            }

            if is_terminal_velocity_reached {
                // Minimum number of steps.
                let min_steps = 16;

                if step > min_steps {
                    break;
                }

                for part in 1..=self.nb_particles {
                    let ksi = random.next_real();

                    let p = particles.get_particle_mut(part)?;

                    for d in 0..n {
                        let k = d as usize;
                        let pos = p.position.get(k).copied().unwrap_or(0.0);
                        let s = at(&self.steps, d + 1);
                        let v = if pos == at(&min_uv, d + 1) || pos == at(&max_uv, d + 1) {
                            if pos == at(&min_uv, d + 1) { s * ksi } else { -s * ksi }
                        } else {
                            s * (ksi - 0.5) * 2.0
                        };
                        if let Some(sl) = slot(&mut p.velocity, k) {
                            *sl = v;
                        }
                    }
                }
            }
            step += 1;
        }
        out.initialized(&best_global_position)?;
        Ok(best_global_distance)
    }
}

//! Noise sampled on a cylinder so terrain detail repeats seamlessly across the
//! east-west map wrap.

use bevy::math::Vec2;
use noise::NoiseFn;
use std::f64::consts::TAU;

#[derive(Clone, Copy)]
pub struct Cylinder {
    /// World-space width of one wrap period.
    pub period: f64,
}

impl Cylinder {
    pub fn new(period: f32) -> Self {
        Self { period: period as f64 }
    }

    /// Samples `n` at world point `p` with the given frequency (features per unit).
    /// `offset` decorrelates several lookups of the same noise.
    pub fn get(&self, n: &impl NoiseFn<f64, 3>, p: Vec2, freq: f64, offset: f64) -> f32 {
        let a = p.x as f64 / self.period * TAU;
        let r = self.period / TAU * freq;
        n.get([a.cos() * r + offset, a.sin() * r - offset, p.y as f64 * freq + offset * 0.7]) as f32
    }

    /// A 2D vector of two decorrelated samples, for domain warping.
    pub fn vec2(&self, n: &impl NoiseFn<f64, 3>, p: Vec2, freq: f64) -> Vec2 {
        Vec2::new(self.get(n, p, freq, 0.0), self.get(n, p, freq, 57.3))
    }
}

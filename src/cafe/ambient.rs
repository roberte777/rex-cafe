//! Decorative life that runs on the clock alone and never reflects Rex.

use super::geometry::{LAMP_Y, LAMPS_X};
use crate::rng::Rng;

const CAT_MIN_X: f32 = 45.0;
const CAT_MAX_X: f32 = 86.0;
const CAT_SPEED: f32 = 3.0;

#[derive(Debug)]
pub struct Cat {
    /// Sprite left edge.
    pub x: f32,
    pub facing_left: bool,
    pub sitting: bool,
    pub stride: f32,
    timer: f32,
}

#[derive(Debug)]
pub struct Mote {
    pub x: f32,
    pub y: f32,
    lamp: usize,
    drift: f32,
    pub phase: f32,
}

#[derive(Debug)]
pub struct Ambient {
    pub cat: Cat,
    pub motes: Vec<Mote>,
    rng: Rng,
}

impl Ambient {
    pub fn new() -> Ambient {
        let mut rng = Rng::new(42);
        let motes = (0..27)
            .map(|i| {
                let lamp = i % LAMPS_X.len();
                let mut m = Mote {
                    x: 0.0,
                    y: 0.0,
                    lamp,
                    drift: 0.0,
                    phase: rng.next_f32() * std::f32::consts::TAU,
                };
                respawn(&mut m, &mut rng);
                m.y = LAMP_Y as f32 + 6.0 + rng.next_f32() * 48.0;
                m
            })
            .collect();
        Ambient {
            cat: Cat {
                x: 66.0,
                facing_left: true,
                sitting: true,
                stride: 0.0,
                timer: 5.0,
            },
            motes,
            rng,
        }
    }

    pub fn update(&mut self, dt: f32, time: f64) {
        let cat = &mut self.cat;
        cat.timer -= dt;
        if cat.timer <= 0.0 {
            cat.sitting = !cat.sitting && self.rng.next_f32() < 0.6;
            if !cat.sitting && self.rng.next_f32() < 0.5 {
                cat.facing_left = !cat.facing_left;
            }
            cat.timer = if cat.sitting {
                5.0 + self.rng.next_f32() * 8.0
            } else {
                3.0 + self.rng.next_f32() * 6.0
            };
        }
        if !cat.sitting {
            let dir = if cat.facing_left { -1.0 } else { 1.0 };
            cat.x += dir * CAT_SPEED * dt;
            cat.stride += dt;
            if cat.x <= CAT_MIN_X || cat.x >= CAT_MAX_X {
                cat.x = cat.x.clamp(CAT_MIN_X, CAT_MAX_X);
                cat.facing_left = !cat.facing_left;
            }
        }

        for m in &mut self.motes {
            m.y += (0.8 + m.drift) * dt;
            m.x += ((time as f32) * 0.7 + m.phase).sin() * 0.6 * dt;
            if m.y > 66.0 {
                respawn(m, &mut self.rng);
            }
        }
    }
}

impl Default for Ambient {
    fn default() -> Self {
        Ambient::new()
    }
}

fn respawn(m: &mut Mote, rng: &mut Rng) {
    m.x = LAMPS_X[m.lamp] as f32 + (rng.next_f32() - 0.5) * 12.0;
    m.y = LAMP_Y as f32 + 6.0 + rng.next_f32() * 4.0;
    m.drift = rng.next_f32() * 0.8;
}

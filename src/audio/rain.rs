//! Rain on the window: a hiss that swells in gusts, a low rumble, and drops on the glass.

use std::f32::consts::TAU;

use super::dsp::{Biquad, Noise, OnePole, Pink, decay, pan};

const MAX_DROPS: usize = 48;
/// Light ticks and heavier drips per second.
const TICKS_PER_SEC: f32 = 32.0;
const DRIPS_PER_SEC: f32 = 1.4;

struct Drop {
    phase: f32,
    step: f32,
    level: f32,
    fade: f32,
    noisy: f32,
    gains: (f32, f32),
}

pub struct Rain {
    sr: f32,
    noise: Noise,
    pink: [Pink; 2],
    highpass: [OnePole; 2],
    lowpass: [Biquad; 2],
    brown: f32,
    rumble: Biquad,
    gust: f32,
    gust_target: f32,
    gust_left: u32,
    drops: Vec<Drop>,
}

impl Rain {
    pub fn new(sr: f32, seed: u32) -> Rain {
        Rain {
            sr,
            noise: Noise::new(seed),
            pink: [Pink::default(), Pink::default()],
            highpass: [OnePole::new(sr, 350.0), OnePole::new(sr, 350.0)],
            lowpass: [
                Biquad::lowpass(sr, 7500.0, 0.6),
                Biquad::lowpass(sr, 7500.0, 0.6),
            ],
            brown: 0.0,
            rumble: Biquad::lowpass(sr, 160.0, 0.7),
            gust: 0.8,
            gust_target: 0.8,
            gust_left: 0,
            drops: Vec::with_capacity(MAX_DROPS),
        }
    }

    fn spawn(&mut self, heavy: bool) {
        if self.drops.len() >= MAX_DROPS {
            return;
        }
        let n = &mut self.noise;
        let (freq, secs, level) = if heavy {
            (
                n.range(650.0, 1400.0),
                n.range(0.010, 0.022),
                n.range(0.05, 0.11),
            )
        } else {
            (
                n.range(2400.0, 6500.0),
                n.range(0.0015, 0.005),
                n.range(0.015, 0.06),
            )
        };
        let gains = pan(n.range(-0.8, 0.8));
        self.drops.push(Drop {
            phase: 0.0,
            step: TAU * freq / self.sr,
            level: level * self.gust,
            fade: decay(self.sr, secs),
            noisy: if heavy { 0.25 } else { 0.5 },
            gains,
        });
    }

    /// A burst of heavier rain, as after a thunderclap.
    pub fn surge(&mut self) {
        self.gust_target = 1.0;
        self.gust_left = (self.noise.range(4.0, 8.0) * self.sr) as u32;
    }

    pub fn sample(&mut self) -> (f32, f32) {
        // Gusts: every few seconds the rain picks a new strength and drifts toward it.
        if self.gust_left == 0 {
            self.gust_target = self.noise.range(0.55, 1.0);
            self.gust_left = (self.noise.range(2.5, 7.0) * self.sr) as u32;
        }
        self.gust_left -= 1;
        self.gust += (self.gust_target - self.gust) * 0.00004;

        let a = self.noise.white();
        let b = self.noise.white() * 0.65 + a * 0.35;
        let mut hiss = [0.0; 2];
        for (ch, w) in [a, b].into_iter().enumerate() {
            let p = self.pink[ch].next(w);
            hiss[ch] = self.lowpass[ch].process(self.highpass[ch].highpass(p)) * 1.1 * self.gust;
        }

        self.brown = (self.brown + self.noise.white() * 0.02) * 0.998;
        let rumble = self.rumble.process(self.brown) * 0.35;

        if self.noise.chance(TICKS_PER_SEC * self.gust / self.sr) {
            self.spawn(false);
        }
        if self.noise.chance(DRIPS_PER_SEC / self.sr) {
            self.spawn(true);
        }
        let (mut l, mut r) = (hiss[0] + rumble, hiss[1] + rumble);
        let mut i = 0;
        while i < self.drops.len() {
            let d = &mut self.drops[i];
            let s = (d.phase.sin() * (1.0 - d.noisy) + self.noise.white() * d.noisy) * d.level;
            d.phase += d.step;
            d.level *= d.fade;
            l += s * d.gains.0;
            r += s * d.gains.1;
            if d.level < 1e-4 {
                self.drops.swap_remove(i);
            } else {
                i += 1;
            }
        }
        (l, r)
    }
}

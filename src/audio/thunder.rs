//! Thunder after the lightning. Real thunder is many overlapping booms from
//! different stretches of the bolt, arriving one after another; so this is a
//! dense stream of low, pitch-sliding "boom grains" whose rate and loudness
//! follow a rolling envelope, warmed so small speakers can carry it, and
//! echoed off the city by a long, dark reverb.

use std::f32::consts::TAU;

use super::dsp::{Biquad, Noise, OnePole, Reverb, decay, pan};

const MAX_ROLLS: usize = 3;
const MAX_GRAINS: usize = 72;
const SWELLS: usize = 6;
/// How much of the echo to mix in.
const WET: f32 = 0.55;
const GAIN: f32 = 1.15;

/// One low boom: a sine that sinks in pitch, roughed up with a little low noise.
#[derive(Clone, Copy)]
struct Grain {
    phase: f32,
    freq: f32,
    sink: f32,
    level: f32,
    fade: f32,
    rough: f32,
    gains: (f32, f32),
}

/// One thunderclap rolling across the sky.
struct Roll {
    /// Samples until it's heard; sound is slower than light.
    wait: u32,
    t: f32,
    strength: f32,
    length: f32,
    /// Seconds to build to full strength: none for a strike overhead, longer far away.
    attack: f32,
    /// Each swell of the roll: when it peaks, how broad it is, and how loud.
    swells: [(f32, f32, f32); SWELLS],
    /// Booms per second at full strength.
    rate: f32,
    /// Pitch range of its booms; distant thunder is lower.
    pitch: (f32, f32),
    waver: f32,
    waver_target: f32,
    waver_left: u32,
    opened: bool,
}

impl Roll {
    fn envelope(&self) -> f32 {
        let t = self.t;
        let build = if self.attack <= 0.0 {
            1.0
        } else {
            (t / self.attack).min(1.0).powf(1.5)
        };
        let swells: f32 = self
            .swells
            .iter()
            .map(|&(at, width, peak)| peak * (-((t - at) / width).powi(2)).exp())
            .sum();
        let tail = (-t / (self.length * 0.35)).exp() * 0.35;
        let fade_out = ((self.length - t) / 1.5).clamp(0.0, 1.0);
        build * (swells + tail).min(1.4) * fade_out
    }
}

pub struct Thunder {
    sr: f32,
    noise: Noise,
    rolls: Vec<Roll>,
    grains: Vec<Grain>,
    rumble_noise: [OnePole; 2],
    tone: [Biquad; 2],
    smooth: [Biquad; 2],
    reverb: Reverb,
}

impl Thunder {
    pub fn new(sr: f32, seed: u32) -> Thunder {
        Thunder {
            sr,
            noise: Noise::new(seed),
            rolls: Vec::with_capacity(MAX_ROLLS),
            grains: Vec::with_capacity(MAX_GRAINS),
            rumble_noise: [OnePole::new(sr, 90.0), OnePole::new(sr, 90.0)],
            tone: [
                Biquad::lowpass(sr, 240.0, 0.7),
                Biquad::lowpass(sr, 240.0, 0.7),
            ],
            smooth: [
                Biquad::lowpass(sr, 520.0, 0.6),
                Biquad::lowpass(sr, 520.0, 0.6),
            ],
            reverb: Reverb::new(sr, 0.85, 0.55),
        }
    }

    /// Thunder for a strike of `strength` (0.3 far to 1 overhead), heard after `delay` seconds.
    pub fn strike(&mut self, strength: f32, delay: f32) {
        if self.rolls.len() >= MAX_ROLLS {
            return;
        }
        let close = strength.clamp(0.0, 1.0);
        let n = &mut self.noise;
        let length = n.range(10.0, 15.0) * (0.8 + 0.3 * (1.0 - close));
        // Swells come thick at first, then spread out and fade as the roll moves off.
        let mut swells = [(0.0, 1.0, 0.0); SWELLS];
        let mut at = if close > 0.75 {
            0.15
        } else {
            n.range(0.8, 1.6)
        };
        let mut peak = 1.0;
        for s in &mut swells {
            *s = (at, n.range(0.5, 1.3) + at * 0.08, peak * n.range(0.75, 1.0));
            at += n.range(0.7, 2.2) + at * 0.15;
            peak *= n.range(0.62, 0.85);
        }
        self.rolls.push(Roll {
            wait: (delay.max(0.0) * self.sr) as u32,
            t: 0.0,
            strength: close,
            length,
            attack: if close > 0.75 {
                0.0
            } else {
                0.6 + (1.0 - close) * 1.4
            },
            swells,
            rate: 22.0 + close * 20.0,
            pitch: if close > 0.6 {
                (40.0, 105.0)
            } else {
                (30.0, 75.0)
            },
            waver: 1.0,
            waver_target: 1.0,
            waver_left: 0,
            opened: false,
        });
    }

    fn boom(&mut self, pitch: (f32, f32), level: f32, secs: f32) {
        if self.grains.len() >= MAX_GRAINS {
            return;
        }
        let n = &mut self.noise;
        let freq = n.range(pitch.0, pitch.1);
        self.grains.push(Grain {
            phase: n.unit(),
            freq,
            // Sinks by about a fifth over the boom.
            sink: 0.8f32.powf(1.0 / (secs * self.sr)),
            level,
            fade: decay(self.sr, secs),
            rough: n.range(0.25, 0.55),
            gains: pan(n.range(-0.6, 0.6)),
        });
    }

    pub fn sample(&mut self) -> (f32, f32) {
        let dt = 1.0 / self.sr;
        let mut i = 0;
        while i < self.rolls.len() {
            if self.rolls[i].wait > 0 {
                self.rolls[i].wait -= 1;
                i += 1;
                continue;
            }
            // A strike overhead lands with a few huge, deep booms at once.
            if !self.rolls[i].opened {
                self.rolls[i].opened = true;
                let strength = self.rolls[i].strength;
                if strength > 0.75 {
                    for _ in 0..4 {
                        let level = strength * self.noise.range(0.6, 0.9);
                        let secs = self.noise.range(0.5, 0.9);
                        self.boom((38.0, 62.0), level, secs);
                    }
                }
            }
            let env = self.rolls[i].envelope();
            let roll = &mut self.rolls[i];
            if roll.waver_left == 0 {
                roll.waver_target = self.noise.range(0.45, 1.0);
                roll.waver_left = (self.noise.range(0.12, 0.4) * self.sr) as u32;
            }
            roll.waver_left -= 1;
            roll.waver += (roll.waver_target - roll.waver) * 0.0015;
            let (rate, pitch, strength, waver) = (roll.rate, roll.pitch, roll.strength, roll.waver);
            roll.t += dt;
            let done = roll.t > roll.length;

            if self.noise.chance(rate * (0.25 + env) * dt) {
                // Mostly small booms, now and then a big one.
                let u = self.noise.unit();
                let level = env * waver * strength * (0.2 + 0.8 * u * u * u) * 0.55;
                let secs = self.noise.range(0.12, 0.45);
                self.boom(pitch, level, secs);
            }
            if done {
                self.rolls.swap_remove(i);
            } else {
                i += 1;
            }
        }

        let low = [
            self.rumble_noise[0].lowpass(self.noise.white()) * 6.0,
            self.rumble_noise[1].lowpass(self.noise.white()) * 6.0,
        ];
        let (mut l, mut r) = (0.0, 0.0);
        let mut i = 0;
        while i < self.grains.len() {
            let g = &mut self.grains[i];
            let tone = (TAU * g.phase).sin();
            let s = (tone * (1.0 - g.rough) + (low[0] + low[1]) * 0.5 * g.rough) * g.level;
            g.phase = (g.phase + g.freq * dt).fract();
            g.freq *= g.sink;
            g.level *= g.fade;
            l += s * g.gains.0;
            r += s * g.gains.1;
            if g.level < 1e-4 {
                self.grains.swap_remove(i);
            } else {
                i += 1;
            }
        }

        // Warm it so small speakers hear the low end as depth, then smooth away any fizz.
        let mut dry = [l, r];
        for (ch, d) in dry.iter_mut().enumerate() {
            let x = self.tone[ch].process(*d);
            *d = self.smooth[ch].process((x * 2.2).tanh() * 0.6);
        }
        let (wl, wr) = self.reverb.process(dry[0], dry[1]);
        ((dry[0] + wl * WET) * GAIN, (dry[1] + wr * WET) * GAIN)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(strength: f32, delay: f32, secs: f32) -> (Thunder, Vec<f32>) {
        let sr = 8_000.0;
        let mut th = Thunder::new(sr, 5);
        th.strike(strength, delay);
        let samples = (0..(sr * secs) as usize).map(|_| th.sample().0).collect();
        (th, samples)
    }

    fn energy(samples: &[f32], from: f32, to: f32) -> f32 {
        let sr = 8_000.0;
        samples[(from * sr) as usize..(to * sr) as usize]
            .iter()
            .map(|x| x * x)
            .sum()
    }

    #[test]
    fn thunder_waits_for_its_delay_then_rolls_and_ends() {
        let (th, samples) = render(0.9, 1.0, 24.0);
        assert_eq!(energy(&samples, 0.0, 0.99), 0.0, "thunder before its delay");
        assert!(
            energy(&samples, 1.0, 3.0) > 1.0,
            "no rumble after the delay"
        );
        assert!(energy(&samples, 5.0, 9.0) > 0.05, "it didn't keep rolling");
        assert!(samples.iter().all(|x| x.is_finite() && x.abs() < 2.0));
        assert!(
            th.rolls.is_empty() && th.grains.is_empty(),
            "thunder never ended"
        );
    }

    #[test]
    fn distant_thunder_builds_slowly() {
        let (_, samples) = render(0.4, 0.0, 6.0);
        assert!(
            energy(&samples, 0.0, 0.3) < energy(&samples, 1.5, 1.8),
            "it should swell in, not hit"
        );
    }
}

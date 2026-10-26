//! Small signal-processing pieces: noise, filters, envelopes, panning.

use std::f32::consts::{PI, TAU};

/// A fast generator for noise and musical choices on the audio thread.
#[derive(Clone, Debug)]
pub struct Noise(u32);

impl Noise {
    pub fn new(seed: u32) -> Noise {
        Noise(seed.max(1))
    }

    fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// Uniform in `0.0..1.0`.
    pub fn unit(&mut self) -> f32 {
        (self.next() >> 8) as f32 / (1u32 << 24) as f32
    }

    /// White noise in `-1.0..1.0`.
    pub fn white(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }

    pub fn chance(&mut self, p: f32) -> bool {
        self.unit() < p
    }

    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[(self.unit() * items.len() as f32) as usize % items.len()]
    }
}

/// Pink noise (equal energy per octave), after Paul Kellet's filter.
#[derive(Clone, Debug, Default)]
pub struct Pink {
    b: [f32; 7],
}

impl Pink {
    pub fn next(&mut self, white: f32) -> f32 {
        let b = &mut self.b;
        b[0] = 0.99886 * b[0] + white * 0.0555179;
        b[1] = 0.99332 * b[1] + white * 0.0750759;
        b[2] = 0.96900 * b[2] + white * 0.153852;
        b[3] = 0.86650 * b[3] + white * 0.3104856;
        b[4] = 0.55000 * b[4] + white * 0.5329522;
        b[5] = -0.7616 * b[5] - white * 0.0168980;
        let out = b.iter().sum::<f32>() + white * 0.5362;
        b[6] = white * 0.115926;
        out * 0.11
    }
}

/// A one-pole lowpass; `highpass` gives its complement.
#[derive(Clone, Debug)]
pub struct OnePole {
    a: f32,
    z: f32,
}

impl OnePole {
    pub fn new(sample_rate: f32, cutoff: f32) -> OnePole {
        OnePole {
            a: 1.0 - (-TAU * cutoff / sample_rate).exp(),
            z: 0.0,
        }
    }

    pub fn lowpass(&mut self, x: f32) -> f32 {
        self.z += self.a * (x - self.z);
        self.z
    }

    pub fn highpass(&mut self, x: f32) -> f32 {
        x - self.lowpass(x)
    }
}

/// A two-pole filter from Robert Bristow-Johnson's cookbook.
#[derive(Clone, Debug)]
pub struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    x: [f32; 2],
    y: [f32; 2],
}

impl Biquad {
    fn from(b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) -> Biquad {
        Biquad {
            b: [b0 / a0, b1 / a0, b2 / a0],
            a: [a1 / a0, a2 / a0],
            x: [0.0; 2],
            y: [0.0; 2],
        }
    }

    pub fn lowpass(sample_rate: f32, cutoff: f32, q: f32) -> Biquad {
        let w = TAU * below_nyquist(sample_rate, cutoff) / sample_rate;
        let (sin, cos) = w.sin_cos();
        let alpha = sin / (2.0 * q);
        Biquad::from(
            (1.0 - cos) / 2.0,
            1.0 - cos,
            (1.0 - cos) / 2.0,
            1.0 + alpha,
            -2.0 * cos,
            1.0 - alpha,
        )
    }

    pub fn bandpass(sample_rate: f32, center: f32, q: f32) -> Biquad {
        let w = TAU * below_nyquist(sample_rate, center) / sample_rate;
        let (sin, cos) = w.sin_cos();
        let alpha = sin / (2.0 * q);
        Biquad::from(alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cos, 1.0 - alpha)
    }

    /// Moves a bandpass to a new center without resetting it, for resonances that sweep.
    pub fn retune_bandpass(&mut self, sample_rate: f32, center: f32, q: f32) {
        let fresh = Biquad::bandpass(sample_rate, center, q);
        self.b = fresh.b;
        self.a = fresh.a;
    }

    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b[0] * x + self.b[1] * self.x[0] + self.b[2] * self.x[1]
            - self.a[0] * self.y[0]
            - self.a[1] * self.y[1];
        self.x = [x, self.x[0]];
        self.y = [y, self.y[0]];
        y
    }
}

/// A filter tuned at or above half the sample rate blows up; keep it under.
fn below_nyquist(sample_rate: f32, freq: f32) -> f32 {
    freq.min(sample_rate * 0.45)
}

/// The per-sample multiplier that decays a level to 1/e in `secs`.
pub fn decay(sample_rate: f32, secs: f32) -> f32 {
    (-1.0 / (secs * sample_rate)).exp()
}

pub fn midi_hz(note: f32) -> f32 {
    440.0 * 2f32.powf((note - 69.0) / 12.0)
}

/// Equal-power gains for a pan from -1 (left) to 1 (right).
pub fn pan(p: f32) -> (f32, f32) {
    let angle = (p.clamp(-1.0, 1.0) + 1.0) * PI / 4.0;
    (angle.cos(), angle.sin())
}

/// A value that glides toward its target instead of jumping, so level changes don't click.
#[derive(Clone, Debug)]
pub struct Smoothed {
    pub value: f32,
    pub target: f32,
    rate: f32,
}

impl Smoothed {
    pub fn new(value: f32, sample_rate: f32, secs: f32) -> Smoothed {
        Smoothed {
            value,
            target: value,
            rate: 1.0 - decay(sample_rate, secs),
        }
    }

    pub fn sample(&mut self) -> f32 {
        self.value += (self.target - self.value) * self.rate;
        self.value
    }
}

/// A feedback comb filter with a damped loop, the body of a reverb.
#[derive(Clone, Debug)]
struct Comb {
    buf: Vec<f32>,
    idx: usize,
    feedback: f32,
    damp: f32,
    store: f32,
}

impl Comb {
    fn process(&mut self, x: f32) -> f32 {
        let out = self.buf[self.idx];
        self.store = out * (1.0 - self.damp) + self.store * self.damp;
        self.buf[self.idx] = x + self.store * self.feedback;
        self.idx = (self.idx + 1) % self.buf.len();
        out
    }
}

#[derive(Clone, Debug)]
struct Allpass {
    buf: Vec<f32>,
    idx: usize,
}

impl Allpass {
    fn process(&mut self, x: f32) -> f32 {
        let delayed = self.buf[self.idx];
        self.buf[self.idx] = x + delayed * 0.5;
        self.idx = (self.idx + 1) % self.buf.len();
        delayed - x
    }
}

/// A small Schroeder reverb in the style of Freeverb: parallel combs into series allpasses.
#[derive(Clone, Debug)]
pub struct Reverb {
    combs: [Vec<Comb>; 2],
    allpasses: [Vec<Allpass>; 2],
}

impl Reverb {
    /// `size` from 0 to 1 sets how long the tail rings; `damp` from 0 to 1 how dark it gets.
    pub fn new(sample_rate: f32, size: f32, damp: f32) -> Reverb {
        let scale = sample_rate / 44_100.0;
        let len = |n: usize, ch: usize| (((n + ch * 23) as f32 * scale) as usize).max(1);
        let combs = |ch: usize| {
            [1116, 1188, 1277, 1356, 1422, 1491]
                .iter()
                .map(|&n| Comb {
                    buf: vec![0.0; len(n, ch)],
                    idx: 0,
                    feedback: 0.7 + 0.28 * size,
                    damp,
                    store: 0.0,
                })
                .collect()
        };
        let allpasses = |ch: usize| {
            [556, 441, 341]
                .iter()
                .map(|&n| Allpass {
                    buf: vec![0.0; len(n, ch)],
                    idx: 0,
                })
                .collect()
        };
        Reverb {
            combs: [combs(0), combs(1)],
            allpasses: [allpasses(0), allpasses(1)],
        }
    }

    /// The wet signal only.
    pub fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        let input = (left + right) * 0.5 * 0.03;
        let mut out = [0.0; 2];
        for (ch, o) in out.iter_mut().enumerate() {
            let mut s: f32 = self.combs[ch].iter_mut().map(|c| c.process(input)).sum();
            for a in &mut self.allpasses[ch] {
                s = a.process(s);
            }
            *o = s;
        }
        (out[0], out[1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_stays_in_range() {
        let mut n = Noise::new(9);
        let mut pink = Pink::default();
        for _ in 0..100_000 {
            let w = n.white();
            assert!((-1.0..1.0).contains(&w));
            assert!(pink.next(w).abs() < 1.0);
        }
    }

    #[test]
    fn lowpass_passes_dc_and_cuts_highs() {
        let sr = 48_000.0;
        let mut lp = Biquad::lowpass(sr, 500.0, 0.707);
        let dc = (0..4000).map(|_| lp.process(1.0)).last().unwrap();
        assert!((dc - 1.0).abs() < 1e-3);
        let mut lp = Biquad::lowpass(sr, 500.0, 0.707);
        let peak = (0..4000)
            .map(|i| lp.process((i as f32 * TAU * 8000.0 / sr).sin()).abs())
            .skip(2000)
            .fold(0.0, f32::max);
        assert!(peak < 0.01, "8 kHz got through at {peak}");
    }

    #[test]
    fn reverb_rings_on_after_an_impulse_and_dies_away() {
        let sr = 8_000.0;
        let mut rv = Reverb::new(sr, 0.8, 0.4);
        rv.process(1.0, 1.0);
        let tail: Vec<f32> = (0..(sr * 6.0) as usize)
            .map(|_| rv.process(0.0, 0.0).0)
            .collect();
        let energy = |a: usize, b: usize| tail[a..b].iter().map(|x| x * x).sum::<f32>();
        assert!(energy(0, 4000) > 0.0);
        assert!(energy(40_000, 48_000) < energy(0, 4000) * 1e-3);
        assert!(tail.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn midi_and_pan() {
        assert!((midi_hz(69.0) - 440.0).abs() < 1e-3);
        assert!((midi_hz(57.0) - 220.0).abs() < 1e-3);
        let (l, r) = pan(0.0);
        assert!((l * l + r * r - 1.0).abs() < 1e-5);
        assert!(pan(-1.0).1.abs() < 1e-6);
    }
}

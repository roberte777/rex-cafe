//! The sounds of a coffee shop at work, kept soft: each drink made at the
//! machine, a cup set down, a spoon stirring at a table, and, while the café
//! is open, a drip machine gurgling, a kettle pouring, and the odd clink.

use std::f32::consts::TAU;

use super::dsp::{Biquad, Noise, OnePole, decay, pan};

/// Where things are in the room, as stereo positions.
const MACHINE_PAN: f32 = -0.55;
const COUNTER_PAN: f32 = -0.35;
const PERCOLATOR_PAN: f32 = -0.4;
const KETTLE_PAN: f32 = -0.2;

const MAX_VOICES: usize = 40;
const MAX_QUEUED: usize = 32;
/// How often the background sounds come round, in seconds.
const PERCOLATE_EVERY: (f32, f32) = (70.0, 200.0);
const KETTLE_EVERY: (f32, f32) = (120.0, 300.0);
const CLINK_EVERY: (f32, f32) = (12.0, 40.0);

/// A ramp up over `attack` seconds from `start`, held until `end`, then released over `release`.
fn window(t: f32, start: f32, attack: f32, end: f32, release: f32) -> f32 {
    if t < start || t > end + release {
        0.0
    } else if t < start + attack {
        (t - start) / attack
    } else if t <= end {
        1.0
    } else {
        1.0 - (t - end) / release
    }
}

/// A bubble in liquid: a quick "bloop" that chirps upward as it rises.
#[derive(Clone, Copy, Default)]
struct Bloop {
    phase: f32,
    freq: f32,
    rise: f32,
    level: f32,
    fade: f32,
}

#[derive(Clone, Default)]
struct Bubbles {
    slots: [Bloop; 8],
}

impl Bubbles {
    /// A bubble at `freq` Hz that rises by `ratio` over `secs`.
    fn spawn(&mut self, sr: f32, freq: f32, ratio: f32, secs: f32, level: f32) {
        if let Some(slot) = self.slots.iter_mut().find(|b| b.level < 1e-4) {
            *slot = Bloop {
                phase: 0.0,
                freq,
                rise: ratio.powf(1.0 / (secs * sr)),
                level,
                fade: decay(sr, secs),
            };
        }
    }

    fn sample(&mut self, sr: f32) -> f32 {
        let mut out = 0.0;
        for b in self.slots.iter_mut().filter(|b| b.level >= 1e-4) {
            out += (TAU * b.phase).sin() * b.level;
            b.phase = (b.phase + b.freq / sr).fract();
            b.freq *= b.rise;
            b.level *= b.fade;
        }
        out
    }

    fn quiet(&self) -> bool {
        self.slots.iter().all(|b| b.level < 1e-4)
    }
}

/// Something struck that rings: a cup, a saucer, a spoon. Each partial is
/// (Hz, loudness, seconds to fade).
#[derive(Clone, Copy)]
struct Ring {
    partials: [(f32, f32, f32); 4],
    level: f32,
}

const CUP: [(f32, f32, f32); 4] = [
    (2320.0, 1.0, 0.12),
    (3570.0, 0.55, 0.09),
    (5110.0, 0.3, 0.06),
    (6890.0, 0.15, 0.04),
];
const SAUCER: [(f32, f32, f32); 4] = [
    (2900.0, 0.6, 0.08),
    (4300.0, 0.4, 0.06),
    (5600.0, 0.2, 0.04),
    (0.0, 0.0, 0.01),
];
const SPOON: [(f32, f32, f32); 4] = [
    (3300.0, 1.0, 0.06),
    (4900.0, 0.5, 0.05),
    (6800.0, 0.25, 0.03),
    (0.0, 0.0, 0.01),
];
const LATCH: [(f32, f32, f32); 4] = [
    (1850.0, 0.6, 0.03),
    (3120.0, 0.5, 0.025),
    (4400.0, 0.3, 0.015),
    (0.0, 0.0, 0.01),
];

fn tuned(partials: [(f32, f32, f32); 4], pitch: f32) -> [(f32, f32, f32); 4] {
    partials.map(|(f, a, s)| (f * pitch, a, s))
}

enum Sound {
    Ring(Ring),
    /// A dull knock: the puck into the knock box, the tamp, a cup on wood.
    Thud {
        from_hz: f32,
        to_hz: f32,
        secs: f32,
        level: f32,
        phase: f32,
        wood: Biquad,
    },
    Grind {
        secs: f32,
        phase: f32,
        motor: OnePole,
        crunch: Biquad,
        click: f32,
    },
    /// The machine's vibratory pump, humming while a shot pulls.
    Pump {
        secs: f32,
        phase: f32,
        low: Biquad,
    },
    /// Liquid streaming into a vessel, its resonance rising as it fills.
    Pour {
        secs: f32,
        from_hz: f32,
        to_hz: f32,
        level: f32,
        bands: [Biquad; 2],
        bubbles: Bubbles,
        waver: f32,
        waver_target: f32,
        waver_left: u32,
    },
    /// The steam wand: a papery "ch-ch-ch" as it stretches the milk, then a smooth roll.
    Steam {
        secs: f32,
        tear: Biquad,
        roll: Biquad,
        air: OnePole,
        hiss: OnePole,
        pulse: f32,
        pulse_target: f32,
        pulse_left: u32,
    },
    Hiss {
        secs: f32,
        level: f32,
        air: OnePole,
    },
    /// A drip machine finishing a pot: bursts of bubbles over a low boil.
    Percolate {
        secs: f32,
        bubbles: Bubbles,
        burst_left: u32,
        until_bubble: u32,
        boil: Biquad,
        air: OnePole,
    },
}

struct Voice {
    sound: Sound,
    t: f32,
    ticks: u64,
    gains: (f32, f32),
}

pub struct Coffee {
    sr: f32,
    noise: Noise,
    now: u64,
    voices: Vec<Voice>,
    queue: Vec<(u64, Voice)>,
    open: bool,
    next_percolate: u64,
    next_kettle: u64,
    next_clink: u64,
}

impl Coffee {
    pub fn new(sr: f32, seed: u32) -> Coffee {
        let mut noise = Noise::new(seed);
        let secs = |s: f32| (s * sr) as u64;
        let next_percolate = secs(noise.range(20.0, 60.0));
        let next_kettle = secs(noise.range(50.0, 120.0));
        let next_clink = secs(noise.range(5.0, 15.0));
        Coffee {
            sr,
            noise,
            now: 0,
            voices: Vec::with_capacity(MAX_VOICES),
            queue: Vec::with_capacity(MAX_QUEUED),
            open: false,
            next_percolate,
            next_kettle,
            next_clink,
        }
    }

    /// The background sounds only play while the café is open.
    pub fn set_open(&mut self, open: bool) {
        self.open = open;
    }

    fn at(&mut self, secs: f32, sound: Sound, pan_to: f32) {
        if self.queue.len() < MAX_QUEUED {
            let when = self.now + (secs.max(0.0) * self.sr) as u64;
            self.queue.push((
                when,
                Voice {
                    sound,
                    t: 0.0,
                    ticks: 0,
                    gains: pan(pan_to),
                },
            ));
        }
    }

    fn ring(&mut self, secs: f32, partials: [(f32, f32, f32); 4], level: f32, pan_to: f32) {
        self.at(secs, Sound::Ring(Ring { partials, level }), pan_to);
    }

    fn thud(&mut self, secs: f32, from_hz: f32, to_hz: f32, length: f32, level: f32, pan_to: f32) {
        let wood = Biquad::bandpass(self.sr, 380.0, 1.5);
        self.at(
            secs,
            Sound::Thud {
                from_hz,
                to_hz,
                secs: length,
                level,
                phase: 0.0,
                wood,
            },
            pan_to,
        );
    }

    fn pour(&mut self, secs: f32, length: f32, from_hz: f32, to_hz: f32, level: f32, pan_to: f32) {
        let bands = [
            Biquad::bandpass(self.sr, from_hz, 7.0),
            Biquad::bandpass(self.sr, from_hz * 2.3, 5.0),
        ];
        let sound = Sound::Pour {
            secs: length,
            from_hz,
            to_hz,
            level,
            bands,
            bubbles: Bubbles::default(),
            waver: 1.0,
            waver_target: 1.0,
            waver_left: 0,
        };
        self.at(secs, sound, pan_to);
    }

    /// One drink at the machine, timed to the Barista's brew: grind, knock out
    /// the old puck, tamp, lock in, then the pump and the shot. Milk drinks
    /// get `steam` seconds of steaming alongside.
    pub fn brew(&mut self, steam: f32) {
        let sr = self.sr;
        let grind = Sound::Grind {
            secs: 0.7,
            phase: 0.0,
            motor: OnePole::new(sr, 900.0),
            crunch: Biquad::bandpass(sr, 2200.0, 0.8),
            click: 0.0,
        };
        self.at(0.0, grind, MACHINE_PAN);
        self.thud(0.85, 95.0, 60.0, 0.09, 0.11, MACHINE_PAN);
        self.thud(1.05, 150.0, 100.0, 0.05, 0.06, MACHINE_PAN);
        self.ring(1.3, LATCH, 0.05, MACHINE_PAN);
        self.ring(1.42, tuned(LATCH, 0.92), 0.04, MACHINE_PAN);
        self.at(
            1.6,
            Sound::Pump {
                secs: 2.6,
                phase: 0.0,
                low: Biquad::lowpass(sr, 320.0, 0.7),
            },
            MACHINE_PAN,
        );
        self.pour(1.7, 2.4, 650.0, 1500.0, 1.0, MACHINE_PAN);
        if steam > 0.0 {
            self.at(
                1.75,
                Sound::Hiss {
                    secs: 0.25,
                    level: 0.5,
                    air: OnePole::new(sr, 2500.0),
                },
                MACHINE_PAN - 0.1,
            );
            let wand = Sound::Steam {
                secs: steam,
                tear: Biquad::bandpass(sr, 3500.0, 1.0),
                roll: Biquad::bandpass(sr, 500.0, 1.2),
                air: OnePole::new(sr, 1500.0),
                hiss: OnePole::new(sr, 4000.0),
                pulse: 0.0,
                pulse_target: 0.0,
                pulse_left: 0,
            };
            self.at(2.0, wand, MACHINE_PAN - 0.1);
        }
    }

    /// A cup set down on its saucer at the end of the counter.
    pub fn serve(&mut self) {
        let pitch = self.noise.range(0.96, 1.05);
        self.ring(0.0, tuned(CUP, pitch), 0.07, COUNTER_PAN);
        self.ring(0.07, tuned(SAUCER, pitch), 0.035, COUNTER_PAN);
        self.thud(0.0, 180.0, 150.0, 0.03, 0.03, COUNTER_PAN);
    }

    /// A spoon stirring round a cup, then two taps on the rim.
    pub fn stir(&mut self, pan_to: f32) {
        let turns = 6 + (self.noise.unit() * 4.0) as usize;
        let mut t = 0.0;
        for _ in 0..turns {
            let pitch = self.noise.range(0.96, 1.04);
            let level = self.noise.range(0.03, 0.05);
            self.ring(t, tuned(SPOON, pitch), level, pan_to);
            t += self.noise.range(0.14, 0.2);
        }
        t += 0.28;
        for _ in 0..2 {
            self.ring(t, tuned(SPOON, 1.15), 0.055, pan_to);
            t += 0.12;
        }
    }

    /// The drip machine in the back finishes a pot.
    pub(super) fn percolate(&mut self) {
        let sr = self.sr;
        let secs = self.noise.range(10.0, 14.0);
        let sound = Sound::Percolate {
            secs,
            bubbles: Bubbles::default(),
            burst_left: 0,
            until_bubble: (0.5 * sr) as u32,
            boil: Biquad::bandpass(sr, 260.0, 0.9),
            air: OnePole::new(sr, 3000.0),
        };
        self.at(0.0, sound, PERCOLATOR_PAN);
    }

    /// Hot water from a kettle, poured slowly over grounds into a carafe.
    pub(super) fn kettle(&mut self) {
        let secs = self.noise.range(6.0, 8.0);
        self.pour(0.0, secs, 320.0, 850.0, 1.8, KETTLE_PAN);
    }

    /// Someone at a table sets their cup down.
    fn clink(&mut self) {
        let pitch = self.noise.range(0.88, 1.1);
        let level = self.noise.range(0.015, 0.03);
        let where_ = self.noise.range(-0.7, 0.8);
        self.ring(0.0, tuned(CUP, pitch), level, where_);
        if self.noise.chance(0.5) {
            self.ring(0.06, tuned(SAUCER, pitch), level * 0.5, where_);
        }
    }

    fn background(&mut self) {
        let secs =
            |noise: &mut Noise, (lo, hi): (f32, f32), sr: f32| (noise.range(lo, hi) * sr) as u64;
        if self.now >= self.next_percolate {
            self.next_percolate = self.now + secs(&mut self.noise, PERCOLATE_EVERY, self.sr);
            if self.open {
                self.percolate();
            }
        }
        if self.now >= self.next_kettle {
            self.next_kettle = self.now + secs(&mut self.noise, KETTLE_EVERY, self.sr);
            if self.open {
                self.kettle();
            }
        }
        if self.now >= self.next_clink {
            self.next_clink = self.now + secs(&mut self.noise, CLINK_EVERY, self.sr);
            if self.open {
                self.clink();
            }
        }
    }

    pub fn sample(&mut self) -> (f32, f32) {
        self.background();
        let mut i = 0;
        while i < self.queue.len() {
            if self.queue[i].0 <= self.now {
                let (_, voice) = self.queue.swap_remove(i);
                if self.voices.len() < MAX_VOICES {
                    self.voices.push(voice);
                }
            } else {
                i += 1;
            }
        }
        self.now += 1;

        let (sr, dt) = (self.sr, 1.0 / self.sr);
        let (mut l, mut r) = (0.0, 0.0);
        let mut i = 0;
        while i < self.voices.len() {
            let v = &mut self.voices[i];
            let t = v.t;
            // Sweeping filters retune every 64 samples, which is smooth enough to hear as a glide.
            let retune = v.ticks.is_multiple_of(64);
            let n = &mut self.noise;
            let (s, done) = match &mut v.sound {
                Sound::Ring(ring) => {
                    let mut s = 0.0;
                    let mut longest: f32 = 0.0;
                    for &(f, a, secs) in &ring.partials {
                        if a > 0.0 {
                            s += a * (TAU * f * t).sin() * (-t / secs).exp();
                            longest = longest.max(secs);
                        }
                    }
                    (s * ring.level, t > longest * 7.0)
                }
                Sound::Thud {
                    from_hz,
                    to_hz,
                    secs,
                    level,
                    phase,
                    wood,
                } => {
                    let f = *to_hz + (*from_hz - *to_hz) * (-t / 0.02).exp();
                    *phase = (*phase + f * dt).fract();
                    let body = (TAU * *phase).sin() * (-t / *secs).exp();
                    let knock = wood.process(n.white()) * (-t / 0.012).exp() * 0.5;
                    ((body + knock) * *level, t > *secs * 8.0)
                }
                Sound::Grind {
                    secs,
                    phase,
                    motor,
                    crunch,
                    click,
                } => {
                    let env = window(t, 0.0, 0.05, *secs - 0.12, 0.12);
                    *phase = (*phase + 140.0 * (1.0 + 0.02 * (TAU * 7.0 * t).sin()) * dt).fract();
                    let whirr: f32 = (1..=5)
                        .map(|k| (TAU * *phase * k as f32).sin() / k as f32)
                        .sum();
                    if n.chance(140.0 * dt) {
                        *click = n.range(0.3, 1.0);
                    }
                    *click *= 0.9;
                    let beans = crunch.process(*click * n.white());
                    (
                        (motor.lowpass(whirr) * 0.025 + beans * 0.06) * env,
                        t > *secs,
                    )
                }
                Sound::Pump { secs, phase, low } => {
                    let env = window(t, 0.0, 0.08, *secs - 0.2, 0.2);
                    *phase = (*phase + (50.0 - 3.0 * t / *secs) * dt).fract();
                    let buzz: f32 = [1.0, 3.0, 5.0, 7.0]
                        .iter()
                        .map(|k| (TAU * *phase * k).sin() / k)
                        .sum();
                    (low.process(buzz) * 0.026 * env, t > *secs)
                }
                Sound::Pour {
                    secs,
                    from_hz,
                    to_hz,
                    level,
                    bands,
                    bubbles,
                    waver,
                    waver_target,
                    waver_left,
                } => {
                    let p = (t / *secs).min(1.0);
                    let stream_env =
                        window(t, *secs * 0.12, *secs * 0.08, *secs * 0.86, *secs * 0.1);
                    if retune {
                        let f = *from_hz + (*to_hz - *from_hz) * p.powf(0.8);
                        bands[0].retune_bandpass(sr, f, 7.0);
                        bands[1].retune_bandpass(sr, f * 2.3, 5.0);
                    }
                    if *waver_left == 0 {
                        *waver_target = n.range(0.45, 1.0);
                        *waver_left = (n.range(0.015, 0.04) * sr) as u32;
                    }
                    *waver_left -= 1;
                    *waver += (*waver_target - *waver) * 0.01;
                    let w = n.white();
                    let stream =
                        (bands[0].process(w) + 0.5 * bands[1].process(w)) * *waver * stream_env;
                    let dripping = p < 0.16 || p > 0.86;
                    if t < *secs && n.chance(if dripping { 9.0 } else { 4.0 } * dt) {
                        let freq = n.range(800.0, 1500.0) * (1.0 + p * 0.6);
                        let secs_b = n.range(0.02, 0.04);
                        let lvl = if dripping {
                            n.range(0.5, 1.0)
                        } else {
                            n.range(0.2, 0.4)
                        };
                        bubbles.spawn(sr, freq, 1.7, secs_b, lvl);
                    }
                    let s = (stream * 0.55 + bubbles.sample(sr) * 0.02) * *level;
                    (s, t > *secs + 0.2 && bubbles.quiet())
                }
                Sound::Steam {
                    secs,
                    tear,
                    roll,
                    air,
                    hiss,
                    pulse,
                    pulse_target,
                    pulse_left,
                } => {
                    let p = (t / *secs).min(1.0);
                    let env = window(t, 0.0, 0.1, *secs - 0.25, 0.25);
                    if *pulse_left == 0 {
                        *pulse_target = if n.chance(0.65) {
                            n.range(0.6, 1.0)
                        } else {
                            0.1
                        };
                        *pulse_left = (n.range(0.008, 0.03) * sr) as u32;
                    }
                    *pulse_left -= 1;
                    *pulse += (*pulse_target - *pulse) * 0.08;
                    if retune {
                        roll.retune_bandpass(sr, 500.0 + 450.0 * p, 1.2);
                    }
                    let w = n.white();
                    let tearing = tear.process(air.highpass(w)) * *pulse;
                    let rolling = roll.process(w) * (0.8 + 0.2 * (TAU * 4.5 * t).sin());
                    let tear_w = (1.0 - p / 0.35).clamp(0.0, 1.0);
                    let roll_w = ((p - 0.2) / 0.3).clamp(0.0, 1.0);
                    let air_hiss = hiss.highpass(w) * 0.15;
                    let s =
                        (tearing * tear_w * 0.9 + rolling * roll_w + air_hiss * 0.2) * env * 0.06;
                    (s, t > *secs)
                }
                Sound::Hiss { secs, level, air } => {
                    let env = window(t, 0.0, 0.03, *secs - 0.15, 0.15);
                    (air.highpass(n.white()) * env * *level * 0.03, t > *secs)
                }
                Sound::Percolate {
                    secs,
                    bubbles,
                    burst_left,
                    until_bubble,
                    boil,
                    air,
                } => {
                    let env = window(t, 0.0, 1.0, *secs - 1.5, 1.5);
                    if *until_bubble == 0 {
                        if *burst_left > 0 {
                            let freq = n.range(180.0, 420.0);
                            let ratio = n.range(1.5, 2.2);
                            let secs_b = n.range(0.04, 0.09);
                            let lvl = n.range(0.5, 1.0);
                            bubbles.spawn(sr, freq, ratio, secs_b, lvl);
                            *burst_left -= 1;
                            *until_bubble = (n.range(0.04, 0.12) * sr) as u32;
                        } else {
                            *burst_left = 2 + (n.unit() * 5.0) as u32;
                            *until_bubble = (n.range(0.6, 2.5) * sr) as u32;
                        }
                    }
                    *until_bubble = until_bubble.saturating_sub(1);
                    let w = n.white();
                    let s = (bubbles.sample(sr) * 0.9
                        + boil.process(w) * 0.15
                        + air.highpass(w) * 0.05)
                        * env
                        * 0.14;
                    (s, t > *secs)
                }
            };
            l += s * v.gains.0;
            r += s * v.gains.1;
            v.t += dt;
            v.ticks += 1;
            if done {
                self.voices.swap_remove(i);
            } else {
                i += 1;
            }
        }
        (l, r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 16_000.0;

    fn run(coffee: &mut Coffee, secs: f32) -> Vec<f32> {
        (0..(secs * SR) as usize)
            .map(|_| {
                let (l, r) = coffee.sample();
                assert!(l.is_finite() && r.is_finite() && l.abs() < 1.0 && r.abs() < 1.0);
                l + r
            })
            .collect()
    }

    fn rms(samples: &[f32], from: f32, to: f32) -> f32 {
        let s = &samples[(from * SR) as usize..(to * SR) as usize];
        (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt()
    }

    #[test]
    fn a_brew_plays_its_steps_in_order_and_finishes() {
        let mut c = Coffee::new(SR, 3);
        c.brew(2.0);
        let out = run(&mut c, 6.0);
        assert!(rms(&out, 0.1, 0.6) > 1e-4, "no grinder");
        assert!(
            rms(&out, 2.4, 3.6) > rms(&out, 0.75, 0.84),
            "the shot should be louder than the pause before the knock"
        );
        assert!(
            rms(&out, 5.2, 6.0) < 1e-4,
            "still making noise after the drink"
        );
        assert!(c.voices.is_empty() && c.queue.is_empty());
    }

    #[test]
    fn a_stir_and_a_serve_are_brief() {
        let mut c = Coffee::new(SR, 4);
        c.serve();
        c.stir(0.3);
        let out = run(&mut c, 4.0);
        assert!(rms(&out, 0.0, 0.5) > 1e-4);
        assert!(rms(&out, 3.0, 4.0) < 1e-5);
    }

    #[test]
    fn the_background_only_plays_while_open() {
        let mut closed = Coffee::new(SR, 5);
        let quiet = run(&mut closed, 130.0);
        assert!(
            rms(&quiet, 0.0, 130.0) == 0.0,
            "the closed café made coffee"
        );

        let mut open = Coffee::new(SR, 5);
        open.set_open(true);
        let busy = run(&mut open, 130.0);
        assert!(rms(&busy, 0.0, 130.0) > 1e-4, "the open café was silent");
    }
}

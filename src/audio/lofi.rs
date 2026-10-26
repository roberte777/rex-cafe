//! An endless lo-fi track, composed as it plays: jazzy electric-piano chords,
//! a soft bass, swung boom-bap drums, a sparse melody, and vinyl crackle,
//! all through a warm low-pass and a little tape wobble.

use std::f32::consts::TAU;

use super::dsp::{Biquad, Noise, OnePole, Pink, decay, midi_hz, pan};

const BPM: f32 = 74.0;
/// How late the off-beat sixteenths land, as a fraction of a sixteenth.
const SWING: f32 = 0.17;
const STEPS: u32 = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Chord {
    Maj9,
    Min9,
    Dom9,
    Min11,
    Dom13,
}

impl Chord {
    /// Rootless voicings: the bass plays the root.
    fn intervals(self) -> [i32; 4] {
        match self {
            Chord::Maj9 => [4, 7, 11, 14],
            Chord::Min9 => [3, 7, 10, 14],
            Chord::Dom9 => [4, 10, 14, 19],
            Chord::Min11 => [3, 10, 14, 17],
            Chord::Dom13 => [4, 10, 14, 21],
        }
    }
}

use Chord::*;

/// Four-bar progressions as (semitones above the key, chord).
const PROGRESSIONS: [[(i32, Chord); 4]; 6] = [
    [(2, Min9), (7, Dom9), (0, Maj9), (9, Min9)],
    [(0, Maj9), (9, Min9), (2, Min11), (7, Dom13)],
    [(5, Maj9), (4, Min9), (2, Min9), (0, Maj9)],
    [(9, Min9), (5, Maj9), (0, Maj9), (7, Dom9)],
    [(0, Maj9), (5, Maj9), (0, Maj9), (5, Maj9)],
    [(4, Min11), (9, Min9), (2, Min9), (7, Dom13)],
];

/// When in a bar the piano plays its chord, in sixteenths, with how hard.
const COMPING: [&[(u32, f32)]; 4] = [
    &[(0, 0.9)],
    &[(0, 0.85), (6, 0.55)],
    &[(0, 0.85), (10, 0.6)],
    &[(0, 0.8), (3, 0.45), (10, 0.55)],
];

const PENTATONIC: [i32; 5] = [0, 2, 4, 7, 9];

#[derive(Clone, Copy, Debug)]
enum Event {
    Key { note: f32, vel: f32 },
    Bell { note: f32, vel: f32 },
    Bass { note: f32, vel: f32 },
    Kick(f32),
    Snare(f32),
    Hat { vel: f32, open: bool },
}

/// A frequency-modulated electric piano or bell note.
struct Tone {
    freq: f32,
    ratio: f32,
    carrier: f32,
    modulator: f32,
    index: f32,
    index_floor: f32,
    index_fade: f32,
    level: f32,
    fade: f32,
    attack: f32,
    gains: (f32, f32),
}

struct BassNote {
    freq: f32,
    phase: f32,
    level: f32,
    fade: f32,
    attack: f32,
}

enum DrumKind {
    Kick { phase: f32 },
    Snare { band: Biquad },
    Hat { high: OnePole, open: bool },
}

struct Drum {
    kind: DrumKind,
    t: f32,
    vel: f32,
}

pub struct Lofi {
    sr: f32,
    rng: Noise,
    sixteenth: f64,
    sample: u64,
    next_step: f64,
    step: u32,
    bar: u32,
    key: i32,
    progression: usize,
    chord_root: i32,
    voicing: [f32; 4],
    center: f32,
    melody_note: i32,
    pending: Vec<(u64, Event)>,
    tones: Vec<Tone>,
    bass: Vec<BassNote>,
    drums: Vec<Drum>,
    time: f32,
    pink: Pink,
    crackle: OnePole,
    tone_filter: [Biquad; 2],
}

impl Lofi {
    pub fn new(sr: f32, seed: u32) -> Lofi {
        let mut rng = Noise::new(seed);
        let progression = (rng.unit() * PROGRESSIONS.len() as f32) as usize % PROGRESSIONS.len();
        Lofi {
            sr,
            rng,
            sixteenth: 60.0 / BPM as f64 / 4.0 * sr as f64,
            sample: 0,
            next_step: 0.0,
            step: 0,
            bar: 0,
            key: 5,
            progression,
            chord_root: 0,
            voicing: [0.0; 4],
            center: 62.0,
            melody_note: 76,
            pending: Vec::with_capacity(64),
            tones: Vec::with_capacity(32),
            bass: Vec::with_capacity(4),
            drums: Vec::with_capacity(16),
            time: 0.0,
            pink: Pink::default(),
            crackle: OnePole::new(sr, 1500.0),
            tone_filter: [
                Biquad::lowpass(sr, 4600.0, 0.7),
                Biquad::lowpass(sr, 4600.0, 0.7),
            ],
        }
    }

    fn schedule(&mut self, at: f64, event: Event) {
        if self.pending.len() < self.pending.capacity() {
            self.pending.push((at.max(0.0) as u64, event));
        }
    }

    /// A few milliseconds of human looseness.
    fn loose(&mut self) -> f64 {
        self.rng.range(-0.004, 0.004) as f64 * self.sr as f64
    }

    fn new_bar(&mut self) {
        if self.bar > 0 && self.bar.is_multiple_of(8) {
            let mut next = self.progression;
            while next == self.progression {
                next = (self.rng.unit() * PROGRESSIONS.len() as f32) as usize % PROGRESSIONS.len();
            }
            self.progression = next;
        }
        if self.bar > 0 && self.bar.is_multiple_of(32) {
            // Move to a nearby key, staying within a tritone of C.
            let k = (self.key + *self.rng.pick(&[5, 7, -5, 2, -2])).rem_euclid(12);
            self.key = if k > 6 { k - 12 } else { k };
        }
        let (degree, chord) = PROGRESSIONS[self.progression][(self.bar % 4) as usize];
        self.chord_root = 48 + self.key + degree;
        // Keep the voicing near the last one so chords move smoothly.
        let base = chord.intervals().map(|i| (self.chord_root + i) as f32);
        let mean = base.iter().sum::<f32>() / 4.0;
        let shift = [-12.0, 0.0, 12.0]
            .into_iter()
            .filter(|s| (55.0..70.0).contains(&(mean + s)))
            .min_by(|a, b| {
                ((mean + a) - self.center)
                    .abs()
                    .total_cmp(&((mean + b) - self.center).abs())
            })
            .unwrap_or(0.0);
        for (v, n) in self.voicing.iter_mut().zip(base) {
            *v = n + shift;
        }
        self.center = self.center * 0.5 + (mean + shift) * 0.5;
    }

    /// A pentatonic note within a fourth of the last melody note, if any.
    fn melody_step(&mut self) -> Option<i32> {
        let fits = |n: i32, last: i32, key: i32| {
            PENTATONIC.contains(&(n - key).rem_euclid(12))
                && (n - last).abs() <= 5
                && (70..86).contains(&n)
        };
        let (last, key) = (self.melody_note, self.key);
        let count = (60..90).filter(|&n| fits(n, last, key)).count();
        if count == 0 {
            return None;
        }
        let pick = (self.rng.unit() * count as f32) as usize % count;
        (60..90).filter(|&n| fits(n, last, key)).nth(pick)
    }

    fn on_step(&mut self) {
        let t0 = self.next_step
            + if self.step % 2 == 1 {
                SWING as f64 * self.sixteenth
            } else {
                0.0
            };
        if self.step == 0 {
            self.new_bar();
        }
        let phrase_bar = self.bar % 16;
        let intro = self.bar < 2;
        let drums = !intro && phrase_bar != 15;
        let melody = (4..12).contains(&phrase_bar) && self.bar >= 4;

        // Piano.
        if self.step == 0 {
            let pattern = *self.rng.pick(&COMPING);
            for &(at, vel) in pattern {
                let start = self.next_step
                    + at as f64 * self.sixteenth
                    + if at % 2 == 1 {
                        SWING as f64 * self.sixteenth
                    } else {
                        0.0
                    };
                for (i, note) in self.voicing.into_iter().enumerate() {
                    let strum = i as f64 * self.rng.range(0.008, 0.018) as f64 * self.sr as f64;
                    let vel = vel * self.rng.range(0.85, 1.0);
                    let jitter = self.loose();
                    self.schedule(start + strum + jitter, Event::Key { note, vel });
                }
            }
        }

        // Bass.
        let root = (self.chord_root - 12) as f32;
        match self.step {
            0 => {
                let j = self.loose();
                self.schedule(
                    t0 + j,
                    Event::Bass {
                        note: root,
                        vel: 0.9,
                    },
                );
            }
            10 if self.rng.chance(0.65) => {
                let note = root + *self.rng.pick(&[0.0, 7.0, 12.0]);
                let j = self.loose();
                self.schedule(t0 + j, Event::Bass { note, vel: 0.6 });
            }
            14 if self.rng.chance(0.25) => {
                let j = self.loose();
                self.schedule(
                    t0 + j,
                    Event::Bass {
                        note: root + 10.0,
                        vel: 0.45,
                    },
                );
            }
            _ => {}
        }

        // Drums: kick on one and the and-of-three, snare on two and four, swung hats.
        if drums {
            let kick = match self.step {
                0 => Some(1.0),
                10 => Some(0.85),
                7 if self.rng.chance(0.3) => Some(0.6),
                13 if self.rng.chance(0.2) => Some(0.5),
                _ => None,
            };
            if let Some(v) = kick {
                let j = self.loose();
                self.schedule(t0 + j, Event::Kick(v));
            }
            let snare = match self.step {
                4 | 12 => Some(self.rng.range(0.85, 1.0)),
                15 if self.rng.chance(0.2) => Some(0.25),
                _ => None,
            };
            if let Some(v) = snare {
                // Lo-fi snares sit a touch behind the beat.
                let late = 0.012 * self.sr as f64;
                self.schedule(t0 + late, Event::Snare(v));
            }
            let hat = if self.step.is_multiple_of(2) {
                Some(if self.step.is_multiple_of(4) {
                    0.6
                } else {
                    0.4
                })
            } else if self.rng.chance(0.12) {
                Some(0.22)
            } else {
                None
            };
            if let Some(v) = hat {
                let open = self.step == 14 && self.rng.chance(0.25);
                let vel = v * self.rng.range(0.8, 1.0);
                let j = self.loose();
                self.schedule(t0 + j, Event::Hat { vel, open });
            }
        }

        // A sparse melody, stepping around the pentatonic scale.
        if melody
            && self.step.is_multiple_of(2)
            && self.step != 0
            && self.rng.chance(0.22)
            && let Some(note) = self.melody_step()
        {
            self.melody_note = note;
            let vel = self.rng.range(0.45, 0.7);
            let j = self.loose();
            self.schedule(
                t0 + j,
                Event::Bell {
                    note: note as f32,
                    vel,
                },
            );
        }

        self.next_step += self.sixteenth;
        self.step = (self.step + 1) % STEPS;
        if self.step == 0 {
            self.bar += 1;
        }
    }

    fn start(&mut self, event: Event) {
        let sr = self.sr;
        match event {
            Event::Key { note, vel } | Event::Bell { note, vel } => {
                if self.tones.len() >= self.tones.capacity() {
                    // Steal the quietest note.
                    if let Some(i) = (0..self.tones.len())
                        .min_by(|&a, &b| self.tones[a].level.total_cmp(&self.tones[b].level))
                    {
                        self.tones.swap_remove(i);
                    }
                }
                let bell = matches!(event, Event::Bell { .. });
                let p =
                    ((note - 60.0) / 24.0).clamp(-0.6, 0.6) * 0.4 + if bell { 0.2 } else { 0.0 };
                let length = if bell {
                    1.1
                } else {
                    2.6 - (note - 48.0).max(0.0) * 0.03
                };
                self.tones.push(Tone {
                    freq: midi_hz(note),
                    ratio: if bell { 3.5 } else { 1.0 },
                    carrier: 0.0,
                    modulator: 0.0,
                    index: if bell { 1.6 } else { 1.9 } * vel,
                    index_floor: if bell { 0.08 } else { 0.3 },
                    index_fade: decay(sr, if bell { 0.08 } else { 0.18 }),
                    level: vel * if bell { 0.1 } else { 0.13 },
                    fade: decay(sr, length),
                    attack: 0.0,
                    gains: pan(p),
                });
            }
            Event::Bass { note, vel } => {
                if self.bass.len() < self.bass.capacity() {
                    // A new note cuts the last one short, as a bassist would.
                    for b in &mut self.bass {
                        b.fade = decay(sr, 0.03);
                    }
                    self.bass.push(BassNote {
                        freq: midi_hz(note),
                        phase: 0.0,
                        level: vel * 0.2,
                        fade: decay(sr, 0.7),
                        attack: 0.0,
                    });
                }
            }
            Event::Kick(vel) => self.drum(DrumKind::Kick { phase: 0.0 }, vel),
            Event::Snare(vel) => self.drum(
                DrumKind::Snare {
                    band: Biquad::bandpass(sr, 1900.0, 0.9),
                },
                vel,
            ),
            Event::Hat { vel, open } => self.drum(
                DrumKind::Hat {
                    high: OnePole::new(sr, 7000.0),
                    open,
                },
                vel,
            ),
        }
    }

    fn drum(&mut self, kind: DrumKind, vel: f32) {
        if self.drums.len() < self.drums.capacity() {
            self.drums.push(Drum { kind, t: 0.0, vel });
        }
    }

    pub fn sample(&mut self) -> (f32, f32) {
        while self.sample as f64 >= self.next_step {
            self.on_step();
        }
        let mut i = 0;
        while i < self.pending.len() {
            if self.pending[i].0 <= self.sample {
                let (_, ev) = self.pending.swap_remove(i);
                self.start(ev);
            } else {
                i += 1;
            }
        }

        let dt = 1.0 / self.sr;
        // Tape: a slow wow and a faint flutter on everything pitched.
        let wobble =
            1.0 + 0.0022 * (TAU * 0.33 * self.time).sin() + 0.0006 * (TAU * 6.1 * self.time).sin();
        let tremolo = 1.0 + 0.06 * (TAU * 4.2 * self.time).sin();

        let (mut l, mut r) = (0.0, 0.0);
        let mut i = 0;
        while i < self.tones.len() {
            let t = &mut self.tones[i];
            let m = (TAU * t.modulator).sin() * t.index;
            let s = (TAU * t.carrier + m).sin() * t.level * t.attack.min(1.0) * tremolo;
            t.carrier = (t.carrier + t.freq * wobble * dt).fract();
            t.modulator = (t.modulator + t.freq * t.ratio * wobble * dt).fract();
            t.index = t.index_floor + (t.index - t.index_floor) * t.index_fade;
            t.level *= t.fade;
            t.attack += dt / 0.004;
            l += s * t.gains.0;
            r += s * t.gains.1;
            if t.level < 1e-4 {
                self.tones.swap_remove(i);
            } else {
                i += 1;
            }
        }

        let mut low = 0.0;
        let mut i = 0;
        while i < self.bass.len() {
            let b = &mut self.bass[i];
            let s = ((TAU * b.phase).sin() + 0.22 * (2.0 * TAU * b.phase).sin())
                * b.level
                * b.attack.min(1.0);
            b.phase = (b.phase + b.freq * wobble * dt).fract();
            b.level *= b.fade;
            b.attack += dt / 0.006;
            low += s;
            if b.level < 1e-4 {
                self.bass.swap_remove(i);
            } else {
                i += 1;
            }
        }

        let mut i = 0;
        while i < self.drums.len() {
            let d = &mut self.drums[i];
            let t = d.t;
            let (s, p, done) = match &mut d.kind {
                DrumKind::Kick { phase } => {
                    let f = 46.0 + 72.0 * (-t / 0.03).exp();
                    *phase = (*phase + f * dt).fract();
                    let body = (TAU * *phase).sin() * (-t / 0.24).exp();
                    let click = self.rng.white() * (-t / 0.002).exp() * 0.12;
                    ((body + click) * 0.36, 0.0, t > 0.7)
                }
                DrumKind::Snare { band } => {
                    let rattle = band.process(self.rng.white()) * (-t / 0.13).exp();
                    let body = (TAU * 185.0 * t).sin() * (-t / 0.04).exp() * 0.5;
                    ((rattle * 1.1 + body) * 0.26, 0.05, t > 0.6)
                }
                DrumKind::Hat { high, open } => {
                    let length = if *open { 0.12 } else { 0.022 };
                    (
                        high.highpass(self.rng.white()) * (-t / length).exp() * 0.09,
                        0.3,
                        t > length * 7.0,
                    )
                }
            };
            let (gl, gr) = pan(p);
            l += s * d.vel * gl;
            r += s * d.vel * gr;
            d.t += dt;
            if done {
                self.drums.swap_remove(i);
            } else {
                i += 1;
            }
        }

        // Vinyl: a faint hiss with crackles and the odd pop.
        let mut vinyl = self.pink.next(self.rng.white()) * 0.012;
        if self.rng.chance(9.0 * dt) {
            vinyl += self.rng.white() * 0.12;
        }
        let vinyl = self.crackle.highpass(vinyl);

        l = self.tone_filter[0].process(l + low) + vinyl;
        r = self.tone_filter[1].process(r + low) + vinyl;
        self.sample += 1;
        self.time += dt;
        (l, r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voicings_stay_in_the_middle_of_the_keyboard() {
        let mut lofi = Lofi::new(48_000.0, 3);
        for _ in 0..64 {
            lofi.new_bar();
            let mean = lofi.voicing.iter().sum::<f32>() / 4.0;
            assert!(
                (55.0..70.0).contains(&mean),
                "voicing {:?} wandered off",
                lofi.voicing
            );
            lofi.bar += 1;
        }
    }

    #[test]
    fn keeps_time_and_stays_in_bounds() {
        let sr = 8_000.0;
        let mut lofi = Lofi::new(sr, 11);
        let secs = 60.0;
        let mut peak: f32 = 0.0;
        for _ in 0..(sr * secs) as usize {
            let (l, r) = lofi.sample();
            assert!(l.is_finite() && r.is_finite());
            peak = peak.max(l.abs()).max(r.abs());
        }
        let bars = secs * BPM / 60.0 / 4.0;
        assert!(
            (lofi.bar as f32 - bars).abs() <= 1.0,
            "{} bars in {secs}s",
            lofi.bar
        );
        assert!(peak > 0.05 && peak < 1.5, "peak {peak}");
    }
}

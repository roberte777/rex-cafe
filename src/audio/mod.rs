//! The café's sound: rain on the window, lo-fi on the speakers, and the
//! espresso machine whenever the Barista makes a drink. Everything is
//! synthesized, so there are no recordings to license or ship.

pub mod coffee;
pub mod dsp;
pub mod lofi;
pub mod rain;
pub mod thunder;

use std::path::Path;
use std::sync::mpsc;

use anyhow::{Context, Result, bail};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde::{Deserialize, Serialize};

use coffee::Coffee;
use dsp::Smoothed;
use lofi::Lofi;
use rain::Rain;
use thunder::Thunder;

const RAIN_LEVEL: f32 = 0.75;
const MUSIC_LEVEL: f32 = 0.85;
const SFX_LEVEL: f32 = 0.9;
const MASTER_LEVEL: f32 = 0.8;

/// A part of the sound the listener can turn up or down.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Channel {
    Master,
    Music,
    Rain,
    Coffee,
}

impl Channel {
    pub const ALL: [Channel; 4] = [
        Channel::Master,
        Channel::Music,
        Channel::Rain,
        Channel::Coffee,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Channel::Master => "master",
            Channel::Music => "music",
            Channel::Rain => "rain",
            Channel::Coffee => "coffee",
        }
    }
}

/// How loud each channel is, from 0 (off) to 1 (as mixed).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Levels {
    pub master: f32,
    pub music: f32,
    pub rain: f32,
    pub coffee: f32,
}

impl Default for Levels {
    fn default() -> Levels {
        Levels {
            master: 1.0,
            music: 1.0,
            rain: 1.0,
            coffee: 1.0,
        }
    }
}

/// Sliders move in tenths.
pub const LEVEL_STEP: f32 = 0.1;

impl Levels {
    pub fn get(&self, ch: Channel) -> f32 {
        match ch {
            Channel::Master => self.master,
            Channel::Music => self.music,
            Channel::Rain => self.rain,
            Channel::Coffee => self.coffee,
        }
    }

    /// Sets a channel, clamped to 0..=1 and snapped to the nearest tenth.
    pub fn set(&mut self, ch: Channel, level: f32) {
        let level = ((level.clamp(0.0, 1.0) / LEVEL_STEP).round() * LEVEL_STEP).clamp(0.0, 1.0);
        match ch {
            Channel::Master => self.master = level,
            Channel::Music => self.music = level,
            Channel::Rain => self.rain = level,
            Channel::Coffee => self.coffee = level,
        }
    }
}

/// Sliders feel even to the ear: half way is about a quarter of the power.
fn gain(level: f32) -> f32 {
    level * level
}

/// Something the soundscape should react to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cue {
    /// The Barista started a drink, steaming milk for `steam` seconds (0 for none).
    Brew {
        steam: f32,
    },
    /// A drink was handed over.
    Serve,
    /// A Clawd stirred its drink, at this stereo position.
    Stir {
        pan: f32,
    },
    Muted(bool),
    Level(Channel, f32),
    /// The café opened or Closed; the music only plays while it's open.
    Open(bool),
    /// Lightning struck: thunder of this strength, heard after `delay` seconds.
    Thunder {
        strength: f32,
        delay: f32,
    },
}

/// Every sound in the café, mixed down to stereo one sample at a time.
pub struct Soundscape {
    rain: Rain,
    thunder: Thunder,
    music: Lofi,
    coffee: Coffee,
    levels: Levels,
    muted: bool,
    fade_in: Smoothed,
    master: Smoothed,
    rain_gain: Smoothed,
    music_gain: Smoothed,
    coffee_gain: Smoothed,
    music_open: Smoothed,
}

impl Soundscape {
    pub fn new(sample_rate: f32, levels: Levels, muted: bool) -> Soundscape {
        let glide = |level: f32| Smoothed::new(level, sample_rate, 0.12);
        let mut fade_in = Smoothed::new(0.0, sample_rate, 1.5);
        fade_in.target = 1.0;
        let mut scape = Soundscape {
            rain: Rain::new(sample_rate, 17),
            thunder: Thunder::new(sample_rate, 41),
            music: Lofi::new(sample_rate, 23),
            coffee: Coffee::new(sample_rate, 31),
            levels,
            muted,
            fade_in,
            master: glide(0.0),
            rain_gain: glide(gain(levels.rain)),
            music_gain: glide(gain(levels.music)),
            coffee_gain: glide(gain(levels.coffee)),
            music_open: Smoothed::new(0.0, sample_rate, 2.0),
        };
        scape.update_master();
        scape.master.value = scape.master.target;
        scape
    }

    fn update_master(&mut self) {
        self.master.target = if self.muted {
            0.0
        } else {
            gain(self.levels.master)
        };
    }

    pub fn cue(&mut self, cue: Cue) {
        match cue {
            Cue::Brew { steam } => self.coffee.brew(steam),
            Cue::Serve => self.coffee.serve(),
            Cue::Stir { pan } => self.coffee.stir(pan),
            Cue::Muted(m) => {
                self.muted = m;
                self.update_master();
            }
            Cue::Level(ch, level) => {
                self.levels.set(ch, level);
                let g = gain(self.levels.get(ch));
                match ch {
                    Channel::Master => self.update_master(),
                    Channel::Music => self.music_gain.target = g,
                    Channel::Rain => self.rain_gain.target = g,
                    Channel::Coffee => self.coffee_gain.target = g,
                }
            }
            Cue::Open(open) => {
                self.music_open.target = if open { 1.0 } else { 0.0 };
                self.coffee.set_open(open);
            }
            Cue::Thunder { strength, delay } => {
                self.thunder.strike(strength, delay);
                self.rain.surge();
            }
        }
    }

    pub fn sample(&mut self) -> (f32, f32) {
        let (rl, rr) = self.rain.sample();
        // Thunder is weather, so it follows the rain's level.
        let (tl, tr) = self.thunder.sample();
        let (rl, rr) = (rl + tl, rr + tr);
        let (ml, mr) = self.music.sample();
        let (sl, sr) = self.coffee.sample();
        let rain = self.rain_gain.sample() * RAIN_LEVEL;
        let music = self.music_gain.sample() * self.music_open.sample() * MUSIC_LEVEL;
        let coffee = self.coffee_gain.sample() * SFX_LEVEL;
        let gain = self.master.sample() * self.fade_in.sample() * MASTER_LEVEL;
        let mix = |r: f32, m: f32, s: f32| ((r * rain + m * music + s * coffee) * gain).tanh();
        (mix(rl, ml, sl), mix(rr, mr, sr))
    }

    /// Fills an interleaved buffer with `channels` channels.
    pub fn render(&mut self, out: &mut [f32], channels: usize) {
        for frame in out.chunks_mut(channels) {
            let (l, r) = self.sample();
            match frame {
                [mono] => *mono = (l + r) * 0.5,
                [left, right, rest @ ..] => {
                    *left = l;
                    *right = r;
                    rest.fill(0.0);
                }
                [] => {}
            }
        }
    }
}

/// Plays the soundscape on the default output device until dropped.
pub struct Audio {
    cues: mpsc::Sender<Cue>,
    _stream: cpal::Stream,
}

impl Audio {
    pub fn start(levels: Levels, muted: bool) -> Result<Audio> {
        let device = cpal::default_host()
            .default_output_device()
            .context("no audio output device")?;
        let config = device
            .default_output_config()
            .context("reading the output format")?;
        if config.sample_format() != cpal::SampleFormat::F32 {
            bail!(
                "the output device wants {:?} samples",
                config.sample_format()
            );
        }
        let channels = config.channels() as usize;
        let mut scape = Soundscape::new(config.sample_rate() as f32, levels, muted);
        let (tx, rx) = mpsc::channel();
        let stream = device.build_output_stream(
            config.into(),
            move |data: &mut [f32], _| {
                while let Ok(cue) = rx.try_recv() {
                    scape.cue(cue);
                }
                scape.render(data, channels);
            },
            |_| {},
            None,
        )?;
        stream.play()?;
        Ok(Audio {
            cues: tx,
            _stream: stream,
        })
    }

    pub fn cue(&self, cue: Cue) {
        let _ = self.cues.send(cue);
    }
}

/// Renders `secs` of the café to a 16-bit stereo WAV for listening without
/// running it. The first minute plays one of everything: an espresso and a
/// flat white with their serves and stirs, a kettle pour, the drip machine,
/// and a close and a distant thunderclap.
pub fn render_wav(path: &Path, secs: f32, levels: Levels) -> Result<()> {
    const SR: u32 = 48_000;
    let mut scape = Soundscape::new(SR as f32, levels, false);
    scape.cue(Cue::Open(true));
    let timeline: [(f32, Option<Cue>); 10] = [
        (3.0, Some(Cue::Brew { steam: 0.0 })),
        (7.5, Some(Cue::Serve)),
        (10.0, Some(Cue::Stir { pan: 0.2 })),
        (14.0, None),
        (24.0, Some(Cue::Brew { steam: 2.3 })),
        (28.5, Some(Cue::Serve)),
        (31.0, Some(Cue::Stir { pan: -0.3 })),
        (
            35.0,
            Some(Cue::Thunder {
                strength: 0.95,
                delay: 0.5,
            }),
        ),
        (44.0, None),
        (
            52.0,
            Some(Cue::Thunder {
                strength: 0.45,
                delay: 0.5,
            }),
        ),
    ];
    let frames = (secs * SR as f32) as u32;
    let mut data = Vec::with_capacity(frames as usize * 4);
    for i in 0..frames {
        for (at, cue) in &timeline {
            if i == (at * SR as f32) as u32 {
                match cue {
                    Some(cue) => scape.cue(*cue),
                    None if *at < 20.0 => scape.coffee.kettle(),
                    None => scape.coffee.percolate(),
                }
            }
        }
        let (l, r) = scape.sample();
        for s in [l, r] {
            data.extend(((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes());
        }
    }
    let mut wav = Vec::with_capacity(44 + data.len());
    wav.extend(b"RIFF");
    wav.extend((36 + data.len() as u32).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(2u16.to_le_bytes());
    wav.extend(SR.to_le_bytes());
    wav.extend((SR * 4).to_le_bytes());
    wav.extend(4u16.to_le_bytes());
    wav.extend(16u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend((data.len() as u32).to_le_bytes());
    wav.extend(data);
    std::fs::write(path, wav)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix_stays_finite_and_under_full_scale() {
        let sr = 8_000.0;
        let mut s = Soundscape::new(sr, Levels::default(), false);
        s.cue(Cue::Open(true));
        let mut buf = vec![0.0; 2 * 8_000];
        for second in 0..20 {
            if second % 5 == 1 {
                s.cue(Cue::Brew { steam: 2.0 });
                s.cue(Cue::Stir { pan: 0.0 });
                s.cue(Cue::Serve);
            }
            s.render(&mut buf, 2);
            assert!(buf.iter().all(|x| x.is_finite() && x.abs() <= 1.0));
        }
    }

    #[test]
    fn levels_snap_to_tenths_and_clamp() {
        let mut l = Levels::default();
        l.set(Channel::Rain, 0.44);
        assert!((l.rain - 0.4).abs() < 1e-6);
        l.set(Channel::Music, 1.7);
        assert_eq!(l.music, 1.0);
        l.set(Channel::Coffee, -1.0);
        assert_eq!(l.coffee, 0.0);
    }

    #[test]
    fn turning_a_channel_down_quiets_it() {
        let sr = 8_000.0;
        let rms = |levels: Levels| {
            let mut s = Soundscape::new(sr, levels, false);
            let mut buf = vec![0.0; 2 * 8_000];
            for _ in 0..3 {
                s.render(&mut buf, 2);
            }
            (buf.iter().map(|x| x * x).sum::<f32>() / buf.len() as f32).sqrt()
        };
        let full = rms(Levels::default());
        let quiet = rms(Levels {
            rain: 0.0,
            ..Levels::default()
        });
        assert!(quiet < full * 0.2, "rain off: {quiet} vs {full}");
    }

    #[test]
    fn muting_fades_to_silence() {
        let sr = 8_000.0;
        let mut s = Soundscape::new(sr, Levels::default(), false);
        s.cue(Cue::Open(true));
        let mut buf = vec![0.0; 2 * 8_000];
        s.render(&mut buf, 2);
        s.cue(Cue::Muted(true));
        for _ in 0..12 {
            s.render(&mut buf, 2);
        }
        assert!(buf.iter().all(|x| x.abs() < 1e-3));
    }
}

//! Decorative life that runs on the clock alone and never reflects Rex.

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

/// Lightning over the city: a strike every so often, flickering as it goes.
#[derive(Debug)]
pub struct Storm {
    rng: Rng,
    gap: (f32, f32),
    until_next: f32,
    /// Seconds since the last strike.
    since: f32,
    strength: f32,
    /// Each flicker of the flash: when it lands after the strike, and how bright it is.
    pulses: [(f32, f32); 3],
    /// Close strikes show a bolt: which window pane, and a seed for its shape.
    bolt: Option<(usize, u64)>,
}

/// Strikes this strong or more are close: a visible bolt and a startled cat.
const CLOSE: f32 = 0.65;
/// The shortest and longest wait between strikes, in seconds, unless configured otherwise.
pub const STRIKE_GAP: (f32, f32) = (25.0, 300.0);

impl Storm {
    fn new() -> Storm {
        let mut storm = Storm {
            rng: Rng::new(99),
            gap: STRIKE_GAP,
            until_next: 0.0,
            since: f32::INFINITY,
            strength: 0.0,
            pulses: [(0.0, 0.0); 3],
            bolt: None,
        };
        storm.until_next = storm.next_gap();
        storm
    }

    fn next_gap(&mut self) -> f32 {
        let (min, max) = self.gap;
        min + self.rng.next_f32() * (max - min)
    }

    /// Changes how long to wait between strikes, and re-times the next one to match.
    pub fn set_gap(&mut self, min_secs: f32, max_secs: f32) {
        self.gap = (min_secs.min(max_secs), min_secs.max(max_secs));
        self.until_next = self.next_gap();
    }

    /// Advances the storm; returns the strength of a strike that just happened.
    fn update(&mut self, dt: f32) -> Option<f32> {
        self.since += dt;
        self.until_next -= dt;
        if self.until_next > 0.0 {
            return None;
        }
        self.until_next = self.next_gap();
        let r = &mut self.rng;
        self.since = 0.0;
        self.strength = 0.3 + r.next_f32() * 0.7;
        let flickers = 1 + (r.next_f32() * 3.0) as usize;
        self.pulses = [
            (0.0, 1.0),
            (0.09 + r.next_f32() * 0.06, 0.5 + r.next_f32() * 0.4),
            (0.2 + r.next_f32() * 0.12, 0.3 + r.next_f32() * 0.4),
        ];
        for p in self.pulses.iter_mut().skip(flickers) {
            p.1 = 0.0;
        }
        self.bolt =
            (self.strength >= CLOSE).then(|| ((r.next_f32() * 3.0) as usize % 3, r.next_u64()));
        Some(self.strength)
    }

    /// How bright the lightning is right now, from 0 to 1.
    pub fn flash(&self) -> f32 {
        let f = self
            .pulses
            .iter()
            .map(|&(at, peak)| {
                let x = self.since - at;
                if x >= 0.0 {
                    peak * (-x / 0.07).exp()
                } else {
                    0.0
                }
            })
            .fold(0.0, f32::max);
        (f * self.strength).clamp(0.0, 1.0)
    }

    /// The bolt to draw, while the flash is bright enough to show it.
    pub fn bolt(&self) -> Option<(usize, u64)> {
        self.bolt.filter(|_| self.flash() > 0.25)
    }
}

#[derive(Debug)]
pub struct Ambient {
    pub cat: Cat,
    pub storm: Storm,
    rng: Rng,
}

impl Ambient {
    pub fn new() -> Ambient {
        Ambient {
            cat: Cat {
                x: 66.0,
                facing_left: true,
                sitting: true,
                stride: 0.0,
                timer: 5.0,
            },
            storm: Storm::new(),
            rng: Rng::new(42),
        }
    }

    /// Advances the cat and the storm; returns the strength of a lightning strike, if one just happened.
    pub fn update(&mut self, dt: f32) -> Option<f32> {
        let strike = self.storm.update(dt);
        let cat = &mut self.cat;
        if strike.is_some_and(|s| s >= CLOSE) {
            // Thunder overhead: the cat bolts the other way.
            cat.sitting = false;
            cat.facing_left = !cat.facing_left;
            cat.timer = 2.5;
        }
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
        strike
    }
}

impl Default for Ambient {
    fn default() -> Self {
        Ambient::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wait_between_strikes_can_be_changed() {
        let mut a = Ambient::new();
        a.storm.set_gap(5.0, 8.0);
        let dt = 1.0 / 30.0;
        let mut strikes = Vec::new();
        let mut t = 0.0f64;
        while t < 120.0 {
            if a.update(dt).is_some() {
                strikes.push(t);
            }
            t += dt as f64;
        }
        assert!(
            strikes[0] >= 4.9 && strikes[0] <= 8.1,
            "first strike at {}",
            strikes[0]
        );
        for pair in strikes.windows(2) {
            let gap = pair[1] - pair[0];
            assert!((4.9..=8.1).contains(&gap), "a {gap}s gap");
        }
    }

    #[test]
    fn lightning_strikes_every_25_to_300_seconds_and_flashes_briefly() {
        let mut a = Ambient::new();
        let dt = 1.0 / 30.0;
        let mut strikes: Vec<f64> = Vec::new();
        // Keep the test's clock in f64; an f32 one drifts over 50 minutes.
        let mut t = 0.0f64;
        while t < 3000.0 {
            if let Some(s) = a.update(dt) {
                assert!((0.3..=1.0).contains(&s));
                strikes.push(t);
            }
            let f = a.storm.flash();
            assert!((0.0..=1.0).contains(&f));
            match strikes.last() {
                Some(last) if t - last > 1.0 => {
                    assert!(f < 0.01, "still flashing {}s after a strike", t - last)
                }
                None => assert_eq!(f, 0.0),
                _ => {}
            }
            t += dt as f64;
        }
        assert!(
            strikes.len() >= 10,
            "only {} strikes in 50 minutes",
            strikes.len()
        );
        let mut last = 0.0;
        for s in &strikes {
            let gap = s - last;
            assert!(
                (24.9..=300.1).contains(&gap),
                "a {gap}s gap between strikes"
            );
            last = *s;
        }
    }
}

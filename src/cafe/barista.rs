use std::collections::VecDeque;

use super::geometry::{BARISTA_HANDOFF_X, BARISTA_MACHINE_X};
use crate::rng::Rng;

const SPEED: f32 = 10.0;
/// Long enough to grind, tamp, pull a shot, and steam the milk.
const BREW_SECS: f32 = 4.2;

/// What the Barista makes for each order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drink {
    Espresso,
    Cortado,
    FlatWhite,
}

impl Drink {
    /// How long the milk steams, in seconds; none for an espresso.
    pub fn steam_secs(self) -> f32 {
        match self {
            Drink::Espresso => 0.0,
            Drink::Cortado => 1.4,
            Drink::FlatWhite => 2.3,
        }
    }
}
const HAND_SECS: f32 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Idle,
    ToMachine,
    Brewing,
    ToHandoff,
    Handing,
}

/// Makes one drink at a time for Clawds waiting at the counter.
#[derive(Debug)]
pub struct Barista {
    /// Sprite left edge, in canvas pixels.
    pub x: f32,
    target: f32,
    orders: VecDeque<String>,
    stage: Stage,
    drink: Drink,
    timer: f32,
    wander: f32,
    rng: Rng,
    present: bool,
    facing_left: bool,
}

impl Barista {
    pub fn new() -> Barista {
        Barista {
            x: BARISTA_MACHINE_X,
            target: BARISTA_MACHINE_X,
            orders: VecDeque::new(),
            stage: Stage::Idle,
            drink: Drink::Espresso,
            timer: 0.0,
            wander: 4.0,
            rng: Rng::new(7),
            present: true,
            facing_left: false,
        }
    }

    pub fn present(&self) -> bool {
        self.present
    }

    pub fn brewing(&self) -> bool {
        self.stage == Stage::Brewing
    }

    /// The drink being made, while brewing.
    pub fn drink(&self) -> Option<Drink> {
        self.brewing().then_some(self.drink)
    }

    /// How far through the current drink the machine is, from 0 to 1.
    pub fn brew_progress(&self) -> Option<f32> {
        self.brewing()
            .then(|| (1.0 - self.timer / BREW_SECS).clamp(0.0, 1.0))
    }

    /// Faces the machine while making a drink, otherwise the way it's going,
    /// and the customers when standing still.
    pub fn facing_left(&self) -> bool {
        self.facing_left
    }

    /// Carrying a finished drink to the counter.
    pub fn holding_cup(&self) -> bool {
        matches!(self.stage, Stage::ToHandoff | Stage::Handing)
    }

    pub fn serving(&self) -> bool {
        self.stage == Stage::Handing
    }

    pub fn order(&mut self, block: String) {
        if !self.orders.contains(&block) {
            self.orders.push_back(block);
        }
    }

    pub fn cancel(&mut self, block: &str) {
        let was_front = self.orders.front().is_some_and(|b| b == block);
        self.orders.retain(|b| b != block);
        if was_front {
            self.stage = Stage::Idle;
        }
    }

    pub fn clear_orders(&mut self) {
        self.orders.clear();
        self.stage = Stage::Idle;
    }

    /// The Barista goes home; the café is Closed.
    pub fn close(&mut self) {
        self.clear_orders();
        self.present = false;
    }

    fn arrived(&self) -> bool {
        (self.x - self.target).abs() < 0.01
    }

    /// Advances the Barista; returns the block whose drink was just handed over.
    pub fn update(&mut self, dt: f32) -> Option<String> {
        if !self.present {
            return None;
        }
        let step = SPEED * dt;
        let d = self.target - self.x;
        self.x = if d.abs() <= step {
            self.target
        } else {
            self.x + step * d.signum()
        };
        self.facing_left = match self.stage {
            Stage::ToMachine | Stage::Brewing => true,
            _ if d.abs() > step => d < 0.0,
            _ => false,
        };

        match self.stage {
            Stage::Idle => {
                if !self.orders.is_empty() {
                    self.stage = Stage::ToMachine;
                    self.target = BARISTA_MACHINE_X;
                } else {
                    self.wander -= dt;
                    if self.wander <= 0.0 {
                        self.wander = 3.0 + self.rng.next_f32() * 5.0;
                        self.target = BARISTA_MACHINE_X
                            + self.rng.next_f32() * (BARISTA_HANDOFF_X - BARISTA_MACHINE_X - 2.0);
                    }
                }
            }
            Stage::ToMachine if self.arrived() => {
                self.stage = Stage::Brewing;
                self.timer = BREW_SECS;
                self.drink = match self.rng.next_f32() {
                    x if x < 0.35 => Drink::Espresso,
                    x if x < 0.65 => Drink::Cortado,
                    _ => Drink::FlatWhite,
                };
            }
            Stage::Brewing => {
                self.timer -= dt;
                if self.timer <= 0.0 {
                    self.stage = Stage::ToHandoff;
                    self.target = BARISTA_HANDOFF_X;
                }
            }
            Stage::ToHandoff if self.arrived() => {
                self.stage = Stage::Handing;
                self.timer = HAND_SECS;
            }
            Stage::Handing => {
                self.timer -= dt;
                if self.timer <= 0.0 {
                    self.stage = Stage::Idle;
                    self.wander = 2.0;
                    return self.orders.pop_front();
                }
            }
            Stage::ToMachine | Stage::ToHandoff => {}
        }
        None
    }

    /// Back at work after the café reopens.
    pub fn reopen(&mut self) {
        self.present = true;
    }
}

impl Default for Barista {
    fn default() -> Self {
        Barista::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serves_orders_in_turn() {
        let mut b = Barista::new();
        b.order("a".into());
        b.order("b".into());
        let mut served = Vec::new();
        for _ in 0..600 {
            if let Some(s) = b.update(1.0 / 30.0) {
                served.push(s);
            }
        }
        assert_eq!(served, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn a_closed_barista_serves_nobody() {
        let mut b = Barista::new();
        b.order("a".into());
        b.close();
        assert!((0..600).all(|_| b.update(1.0 / 30.0).is_none()));
    }
}

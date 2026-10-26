//! The Café's world: Clawds, the Barista, and how Status changes become Activities.
//!
//! Time comes in through `update(dt)`, so everything here is deterministic.

pub mod ambient;
pub mod barista;
pub mod geometry;

use std::collections::VecDeque;

use crate::agents::{Agent, BlockedOn, Status};
use crate::engine::Rgb;
use ambient::Ambient;
use barista::Barista;
use geometry::{DOOR, PICKUP, Place, SEATS, STANDING_SPOTS, route, seat_order};

/// How fast Clawds walk, in pixels per second.
const WALK_SPEED: f32 = 20.0;
/// How long the door stays open after someone passes through.
const DOOR_OPEN_SECS: f64 = 1.0;

#[derive(Clone, Copy, Debug)]
pub struct Hat {
    pub sprite: &'static str,
    pub color: Rgb,
    pub shade: Rgb,
}

pub const HATS: [Hat; 8] = [
    Hat {
        sprite: "hat_beanie",
        color: Rgb::hex(0x4a80d0),
        shade: Rgb::hex(0x2f5ea8),
    },
    Hat {
        sprite: "hat_cap",
        color: Rgb::hex(0x4cb37a),
        shade: Rgb::hex(0x2e8456),
    },
    Hat {
        sprite: "hat_party",
        color: Rgb::hex(0xc04a9c),
        shade: Rgb::hex(0x8e2f72),
    },
    Hat {
        sprite: "hat_propeller",
        color: Rgb::hex(0xe2b93b),
        shade: Rgb::hex(0xb08a1e),
    },
    Hat {
        sprite: "hat_whipped",
        color: Rgb::hex(0xf2eee6),
        shade: Rgb::hex(0xcfc6b8),
    },
    Hat {
        sprite: "hat_tophat",
        color: Rgb::hex(0xd24a3c),
        shade: Rgb::hex(0x9a2f25),
    },
    Hat {
        sprite: "hat_bow",
        color: Rgb::hex(0xec8fbf),
        shade: Rgb::hex(0xc26897),
    },
    Hat {
        sprite: "hat_beret",
        color: Rgb::hex(0x45b8b0),
        shade: Rgb::hex(0x2b8a84),
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    WalkingIn,
    CoffeeBreak,
    GettingCoffee,
    Working,
    NeedsYou(BlockedOn),
    Done,
    Spilled,
    Leaving,
}

impl Activity {
    fn of(status: Status) -> Activity {
        match status {
            Status::Idle => Activity::CoffeeBreak,
            Status::Working => Activity::Working,
            Status::Blocked(on) => Activity::NeedsYou(on),
            Status::Done => Activity::Done,
            Status::Error => Activity::Spilled,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Errand {
    None,
    WalkIn,
    ToCounter,
    Waiting,
    Back,
    Relocate,
    Leave,
}

#[derive(Clone, Debug)]
pub struct Clawd {
    pub agent: Agent,
    pub hat: usize,
    /// Arrival order, which is the Legend order.
    pub order: u64,
    /// `None` while Outside or leaving.
    pub place: Option<Place>,
    /// Feet position in canvas pixels.
    pub pos: (f32, f32),
    path: VecDeque<(f32, f32)>,
    errand: Errand,
    wants_coffee: bool,
    pub has_cup: bool,
    pub facing_left: bool,
    /// Advances while walking; drives the leg animation.
    pub stride: f32,
    gone: bool,
}

impl Clawd {
    pub fn activity(&self) -> Activity {
        let status = self.agent.status;
        match self.errand {
            Errand::Leave => Activity::Leaving,
            _ if status.is_urgent() => Activity::of(status),
            Errand::WalkIn => Activity::WalkingIn,
            Errand::ToCounter | Errand::Waiting | Errand::Back => Activity::GettingCoffee,
            _ => Activity::of(status),
        }
    }

    pub fn is_walking(&self) -> bool {
        !self.path.is_empty()
    }

    /// Settled in a Seat, so drawn sitting behind its table.
    pub fn is_seated(&self) -> bool {
        !self.is_walking()
            && self.errand != Errand::Waiting
            && self.place.is_some_and(Place::is_seat)
    }

    /// Outside Clawds have no body in the room.
    pub fn is_outside(&self) -> bool {
        self.place.is_none() && self.errand != Errand::Leave
    }

    pub fn is_leaving(&self) -> bool {
        self.errand == Errand::Leave
    }

    fn walk(&mut self, dt: f32) {
        let mut budget = WALK_SPEED * dt;
        while budget > 0.0 {
            let Some(&(tx, ty)) = self.path.front() else {
                break;
            };
            let (dx, dy) = (tx - self.pos.0, ty - self.pos.1);
            let dist = (dx * dx + dy * dy).sqrt();
            if dx.abs() > 0.01 {
                self.facing_left = dx < 0.0;
            }
            if dist <= budget {
                self.pos = (tx, ty);
                self.path.pop_front();
                budget -= dist;
            } else {
                self.pos = (
                    self.pos.0 + dx / dist * budget,
                    self.pos.1 + dy / dist * budget,
                );
                budget = 0.0;
            }
        }
        if self.is_walking() {
            self.stride += dt;
        }
    }

    fn go(&mut self, path: Vec<(f32, f32)>, errand: Errand) {
        self.path = path.into();
        self.errand = errand;
    }
}

#[derive(Debug)]
pub struct Cafe {
    clawds: Vec<Clawd>,
    pub barista: Barista,
    pub ambient: Ambient,
    /// Connected to Rex. A Closed café has no Barista.
    pub open: bool,
    pub time: f64,
    door_open_until: f64,
    next_order: u64,
}

impl Default for Cafe {
    fn default() -> Self {
        Cafe::new()
    }
}

impl Cafe {
    pub fn new() -> Cafe {
        Cafe {
            clawds: Vec::new(),
            barista: Barista::new(),
            ambient: Ambient::new(),
            open: false,
            time: 0.0,
            door_open_until: 0.0,
            next_order: 0,
        }
    }

    /// Every Clawd with a body in the room, including ones on their way out.
    pub fn clawds_in_room(&self) -> impl Iterator<Item = &Clawd> {
        self.clawds.iter().filter(|c| !c.is_outside())
    }

    /// The Legend's Agents, in arrival order. Leaving Clawds have no Agent.
    pub fn legend(&self) -> Vec<&Clawd> {
        let mut v: Vec<&Clawd> = self.clawds.iter().filter(|c| !c.is_leaving()).collect();
        v.sort_by_key(|c| c.order);
        v
    }

    pub fn clawd(&self, block: &str) -> Option<&Clawd> {
        self.clawds
            .iter()
            .find(|c| c.agent.block == block && !c.is_leaving())
    }

    pub fn door_open(&self) -> bool {
        self.time < self.door_open_until
    }

    pub fn set_open(&mut self, open: bool) {
        self.open = open;
        if open {
            self.barista.reopen();
        } else {
            self.barista.close();
            for c in &mut self.clawds {
                if matches!(c.errand, Errand::ToCounter | Errand::Waiting) {
                    let path = route(c.pos, None, c.place, c.pos);
                    c.go(path, Errand::Relocate);
                }
            }
        }
    }

    /// Replaces everyone with `agents`, already in their places: the scene a
    /// visitor sees on walking in, with no parade of arrivals.
    pub fn reset(&mut self, agents: &[Agent]) {
        self.clawds.clear();
        self.barista.clear_orders();
        for agent in agents {
            let mut c = self.new_clawd(agent.clone());
            if let Some(p) = c.place {
                c.pos = p.spot();
            }
            self.clawds.push(c);
        }
    }

    /// Brings the room in line with the latest Agents: newcomers walk in,
    /// missing ones leave, and Status changes are choreographed.
    pub fn sync(&mut self, agents: &[Agent]) {
        for agent in agents {
            if let Some(i) = self
                .clawds
                .iter()
                .position(|c| c.agent.block == agent.block && !c.is_leaving())
            {
                self.update_agent(i, agent.clone());
            } else {
                let mut c = self.new_clawd(agent.clone());
                c.wants_coffee = agent.status == Status::Working;
                if let Some(p) = c.place {
                    c.go(route(DOOR, None, Some(p), DOOR), Errand::WalkIn);
                    self.door_open_until = self.time + DOOR_OPEN_SECS;
                }
                self.clawds.push(c);
            }
        }
        let mut freed = false;
        for i in 0..self.clawds.len() {
            let c = &self.clawds[i];
            if c.is_leaving() || agents.iter().any(|a| a.block == c.agent.block) {
                continue;
            }
            freed = true;
            self.leave(i);
        }
        self.clawds.retain(|c| !c.gone);
        if freed {
            self.fill_places();
        }
    }

    fn new_clawd(&mut self, agent: Agent) -> Clawd {
        let in_use: Vec<usize> = self
            .clawds
            .iter()
            .filter(|c| !c.is_leaving())
            .map(|c| c.hat)
            .collect();
        let hat = (0..HATS.len())
            .find(|h| !in_use.contains(h))
            .unwrap_or(self.next_order as usize % HATS.len());
        let place = self.free_place();
        self.next_order += 1;
        Clawd {
            agent,
            hat,
            order: self.next_order,
            place,
            pos: DOOR,
            path: VecDeque::new(),
            errand: Errand::None,
            wants_coffee: false,
            has_cup: false,
            facing_left: true,
            stride: 0.0,
            gone: false,
        }
    }

    fn taken(&self, p: Place) -> bool {
        self.clawds.iter().any(|c| c.place == Some(p))
    }

    fn free_seat(&self) -> Option<Place> {
        seat_order().map(Place::Seat).find(|p| !self.taken(*p))
    }

    fn free_place(&self) -> Option<Place> {
        self.free_seat().or_else(|| {
            (0..STANDING_SPOTS)
                .map(Place::Standing)
                .find(|p| !self.taken(*p))
        })
    }

    fn update_agent(&mut self, i: usize, agent: Agent) {
        let c = &mut self.clawds[i];
        let prev = c.agent.status;
        c.agent = agent;
        let now = c.agent.status;
        if prev == now {
            return;
        }
        match now {
            // A new turn starts with coffee; resuming after you unblock it doesn't.
            Status::Working => {
                c.wants_coffee |= matches!(prev, Status::Idle | Status::Done | Status::Error)
            }
            Status::Idle | Status::Done | Status::Error => c.wants_coffee = false,
            Status::Blocked(_) => {}
        }
        if now.is_urgent() && matches!(c.errand, Errand::ToCounter | Errand::Waiting) {
            let block = c.agent.block.clone();
            let path = route(c.pos, None, c.place, c.pos);
            c.go(path, Errand::Relocate);
            self.barista.cancel(&block);
        }
    }

    fn leave(&mut self, i: usize) {
        let c = &mut self.clawds[i];
        if c.is_outside() {
            c.gone = true;
            return;
        }
        let from = if c.is_walking() { None } else { c.place };
        let path = route(c.pos, from, None, DOOR);
        c.place = None;
        c.has_cup = false;
        let block = c.agent.block.clone();
        c.go(path, Errand::Leave);
        self.barista.cancel(&block);
    }

    /// Moves standing Clawds into freed Seats, then lets Outside ones in.
    fn fill_places(&mut self) {
        let mut order: Vec<usize> = (0..self.clawds.len()).collect();
        order.sort_by_key(|&i| self.clawds[i].order);
        for &i in &order {
            let c = &self.clawds[i];
            if c.errand != Errand::None || !matches!(c.place, Some(Place::Standing(_))) {
                continue;
            }
            let Some(seat) = self.free_seat() else { break };
            let c = &mut self.clawds[i];
            let path = route(c.pos, c.place, Some(seat), c.pos);
            c.place = Some(seat);
            c.go(path, Errand::Relocate);
        }
        for &i in &order {
            if !self.clawds[i].is_outside() {
                continue;
            }
            let Some(p) = self.free_place() else { break };
            let c = &mut self.clawds[i];
            c.place = Some(p);
            c.pos = DOOR;
            c.go(route(DOOR, None, Some(p), DOOR), Errand::WalkIn);
            self.door_open_until = self.time + DOOR_OPEN_SECS;
        }
    }

    pub fn update(&mut self, dt: f32) {
        self.time += dt as f64;
        self.ambient.update(dt, self.time);
        let served = self.barista.update(dt);
        for c in &mut self.clawds {
            if let Some(block) = &served
                && *block == c.agent.block
                && c.errand == Errand::Waiting
            {
                c.has_cup = true;
                let path = route(c.pos, None, c.place, c.pos);
                c.go(path, Errand::Back);
            }
            let was_walking = c.is_walking();
            c.walk(dt);
            if was_walking && !c.is_walking() {
                match c.errand {
                    Errand::WalkIn | Errand::Relocate => c.errand = Errand::None,
                    Errand::ToCounter => {
                        c.errand = Errand::Waiting;
                        c.facing_left = true;
                        self.barista.order(c.agent.block.clone());
                    }
                    Errand::Back => {
                        c.errand = Errand::None;
                        c.has_cup = false;
                        c.wants_coffee = false;
                    }
                    Errand::Leave => c.gone = true,
                    Errand::None | Errand::Waiting => {}
                }
            }
            let near_door = (c.pos.0 - DOOR.0).abs() < 4.0 && (c.pos.1 - DOOR.1).abs() < 2.0;
            if near_door && (c.errand == Errand::Leave || c.errand == Errand::WalkIn) {
                self.door_open_until = self.door_open_until.max(self.time + 0.4);
            }
            let settled = c.errand == Errand::None && !c.is_walking() && c.place.is_some();
            if settled && c.wants_coffee && c.agent.status == Status::Working && self.open {
                let path = route(c.pos, c.place, None, PICKUP);
                c.go(path, Errand::ToCounter);
            }
        }
        let before = self.clawds.len();
        self.clawds.retain(|c| !c.gone);
        if self.clawds.len() != before {
            self.fill_places();
        }
    }

    /// Counts for the Menu: working, needs you, idle, done.
    pub fn menu_counts(&self) -> [usize; 4] {
        let mut n = [0; 4];
        for c in self.legend() {
            match c.agent.status {
                Status::Working => n[0] += 1,
                Status::Blocked(_) => n[1] += 1,
                Status::Idle => n[2] += 1,
                Status::Done => n[3] += 1,
                Status::Error => {}
            }
        }
        n
    }
}

/// Seats and Standing spots in the room.
pub const CAPACITY: usize = SEATS + STANDING_SPOTS;

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(block: &str, status: Status) -> Agent {
        Agent {
            block: block.into(),
            session: "s".into(),
            app: "claude-code".into(),
            name: block.into(),
            status,
            msg: None,
            since: None,
        }
    }

    fn open_cafe() -> Cafe {
        let mut cafe = Cafe::new();
        cafe.set_open(true);
        cafe
    }

    fn run(cafe: &mut Cafe, secs: f32) {
        for _ in 0..(secs * 30.0) as usize {
            cafe.update(1.0 / 30.0);
        }
    }

    fn activity(cafe: &Cafe, block: &str) -> Activity {
        cafe.clawd(block).unwrap().activity()
    }

    #[test]
    fn reset_places_everyone_without_walking() {
        let mut cafe = open_cafe();
        cafe.reset(&[agent("a", Status::Idle), agent("b", Status::Working)]);
        assert_eq!(activity(&cafe, "a"), Activity::CoffeeBreak);
        assert_eq!(activity(&cafe, "b"), Activity::Working);
        assert!(cafe.clawd("a").unwrap().is_seated());
    }

    #[test]
    fn newcomers_walk_in_then_take_a_coffee_break() {
        let mut cafe = open_cafe();
        cafe.sync(&[agent("a", Status::Idle)]);
        assert_eq!(activity(&cafe, "a"), Activity::WalkingIn);
        assert!(cafe.door_open());
        run(&mut cafe, 15.0);
        assert_eq!(activity(&cafe, "a"), Activity::CoffeeBreak);
        assert!(cafe.clawd("a").unwrap().is_seated());
    }

    #[test]
    fn a_new_turn_starts_with_coffee() {
        let mut cafe = open_cafe();
        cafe.reset(&[agent("a", Status::Idle)]);
        cafe.sync(&[agent("a", Status::Working)]);
        cafe.update(0.01);
        assert_eq!(activity(&cafe, "a"), Activity::GettingCoffee);
        run(&mut cafe, 30.0);
        assert_eq!(activity(&cafe, "a"), Activity::Working);
        assert!(cafe.clawd("a").unwrap().is_seated());
    }

    #[test]
    fn unblocking_goes_straight_back_to_work() {
        let mut cafe = open_cafe();
        cafe.reset(&[agent("a", Status::Blocked(BlockedOn::Permission))]);
        cafe.sync(&[agent("a", Status::Working)]);
        cafe.update(0.01);
        assert_eq!(activity(&cafe, "a"), Activity::Working);
    }

    #[test]
    fn blocking_interrupts_a_coffee_run() {
        let mut cafe = open_cafe();
        cafe.reset(&[agent("a", Status::Idle)]);
        cafe.sync(&[agent("a", Status::Working)]);
        run(&mut cafe, 1.0);
        cafe.sync(&[agent("a", Status::Blocked(BlockedOn::Question))]);
        assert_eq!(
            activity(&cafe, "a"),
            Activity::NeedsYou(BlockedOn::Question)
        );
        run(&mut cafe, 30.0);
        assert!(cafe.clawd("a").unwrap().is_seated());
        assert_eq!(
            activity(&cafe, "a"),
            Activity::NeedsYou(BlockedOn::Question)
        );
    }

    #[test]
    fn a_turn_that_ends_mid_errand_shows_the_latest_status_on_return() {
        let mut cafe = open_cafe();
        cafe.reset(&[agent("a", Status::Idle)]);
        cafe.sync(&[agent("a", Status::Working)]);
        run(&mut cafe, 1.0);
        cafe.sync(&[agent("a", Status::Done)]);
        assert_eq!(activity(&cafe, "a"), Activity::GettingCoffee);
        run(&mut cafe, 30.0);
        assert_eq!(activity(&cafe, "a"), Activity::Done);
    }

    #[test]
    fn leavers_walk_out_and_vanish() {
        let mut cafe = open_cafe();
        cafe.reset(&[agent("a", Status::Idle)]);
        cafe.sync(&[]);
        assert!(cafe.legend().is_empty());
        assert_eq!(cafe.clawds_in_room().count(), 1);
        run(&mut cafe, 20.0);
        assert_eq!(cafe.clawds_in_room().count(), 0);
    }

    #[test]
    fn overflow_stands_then_waits_outside_then_moves_up() {
        let mut cafe = open_cafe();
        let agents: Vec<Agent> = (0..CAPACITY + 1)
            .map(|i| agent(&format!("{i:02}"), Status::Idle))
            .collect();
        cafe.reset(&agents);
        let standing = cafe
            .legend()
            .iter()
            .filter(|c| matches!(c.place, Some(Place::Standing(_))))
            .count();
        assert_eq!(standing, STANDING_SPOTS);
        let last = format!("{:02}", CAPACITY);
        assert!(cafe.clawd(&last).unwrap().is_outside());

        cafe.sync(&agents[1..]);
        run(&mut cafe, 30.0);
        let first_standing = &agents[SEATS].block;
        assert!(cafe.clawd(first_standing).unwrap().place.unwrap().is_seat());
        assert!(!cafe.clawd(&last).unwrap().is_outside());
    }

    #[test]
    fn hats_are_unique_while_there_are_enough() {
        let mut cafe = open_cafe();
        let agents: Vec<Agent> = (0..HATS.len())
            .map(|i| agent(&i.to_string(), Status::Idle))
            .collect();
        cafe.reset(&agents);
        let mut hats: Vec<usize> = cafe.legend().iter().map(|c| c.hat).collect();
        hats.sort();
        hats.dedup();
        assert_eq!(hats.len(), HATS.len());
    }

    #[test]
    fn closing_sends_customers_back_and_skips_coffee() {
        let mut cafe = open_cafe();
        cafe.reset(&[agent("a", Status::Idle)]);
        cafe.set_open(false);
        cafe.sync(&[agent("a", Status::Working)]);
        run(&mut cafe, 2.0);
        assert_eq!(activity(&cafe, "a"), Activity::Working);
        assert!(cafe.clawd("a").unwrap().is_seated());
    }
}

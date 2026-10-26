//! Where everything is in the room, in canvas pixels. The room is 140 x 88;
//! a pixel is one terminal column wide and half a row tall.

pub const ROOM_W: usize = 140;
pub const ROOM_H: usize = 88;

/// The floor line along the back wall: Clawds walk here between the door and the counter.
pub const AISLE_Y: f32 = 47.0;
/// The gap between the two rows of tables.
pub const MID_Y: f32 = 71.0;
/// Where a Clawd stands in the doorway, feet position.
pub const DOOR: (f32, f32) = (124.0, 46.0);
/// Where a Clawd waits for its coffee, at the end of the counter.
pub const PICKUP: (f32, f32) = (48.0, AISLE_Y);

pub const COUNTER_RIGHT: i32 = 42;
pub const COUNTER_TOP: i32 = 43;
pub const COUNTER_BOTTOM: i32 = 60;

/// Barista positions, as the sprite's left edge.
pub const BARISTA_MACHINE_X: f32 = 15.0;
pub const BARISTA_HANDOFF_X: f32 = 26.0;
pub const BARISTA_Y: i32 = 28;

pub const LAMPS_X: [i32; 3] = [30, 72, 110];
pub const LAMP_Y: i32 = 9;

#[derive(Clone, Copy, Debug)]
pub struct Table {
    pub x: i32,
    pub w: i32,
    /// The table top's upper edge.
    pub y: i32,
}

pub const TABLES: [Table; 6] = [
    Table {
        x: 62,
        w: 23,
        y: 56,
    },
    Table {
        x: 93,
        w: 25,
        y: 56,
    },
    Table { x: 7, w: 25, y: 78 },
    Table {
        x: 42,
        w: 25,
        y: 78,
    },
    Table {
        x: 77,
        w: 24,
        y: 78,
    },
    Table {
        x: 111,
        w: 24,
        y: 78,
    },
];

/// Gaps between tables that Clawds use to reach the front row.
const CORRIDORS_X: [f32; 3] = [54.0, 89.0, 126.0];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Place {
    Seat(usize),
    Standing(usize),
}

pub const SEATS: usize = 12;
pub const STANDING_SPOTS: usize = 4;
const STANDING_X: [f32; STANDING_SPOTS] = [8.0, 19.0, 30.0, 41.0];
const STANDING_Y: f32 = 67.0;

/// Seats in the order they're handed out: one Clawd per table first, then
/// the second chair at each, so the room fills evenly.
pub fn seat_order() -> impl Iterator<Item = usize> {
    [0, 3, 4, 7, 8, 11, 1, 2, 5, 6, 9, 10].into_iter()
}

/// Seat `i` is table `i / 2`, left chair when even.
pub fn seat_table(i: usize) -> Table {
    TABLES[i / 2]
}

impl Place {
    /// The Clawd's feet position when it is in this place. Seated Clawds'
    /// feet are hidden behind the table top.
    pub fn spot(self) -> (f32, f32) {
        match self {
            Place::Seat(i) => {
                let t = seat_table(i);
                let x = if i % 2 == 0 { t.x + 7 } else { t.x + t.w - 7 };
                (x as f32, t.y as f32 + 1.0)
            }
            Place::Standing(i) => (STANDING_X[i], STANDING_Y),
        }
    }

    pub fn is_seat(self) -> bool {
        matches!(self, Place::Seat(_))
    }

    /// Waypoints from the back aisle to this place, ending at `spot()`.
    pub fn route_from_aisle(self) -> Vec<(f32, f32)> {
        let (x, y) = self.spot();
        match self {
            Place::Seat(i) if i < 4 => vec![(x, AISLE_Y), (x, y)],
            Place::Seat(_) => {
                let cx = nearest_corridor(x);
                vec![(cx, AISLE_Y), (cx, MID_Y), (x, MID_Y), (x, y)]
            }
            Place::Standing(_) => {
                vec![(CORRIDORS_X[0], AISLE_Y), (CORRIDORS_X[0], y), (x, y)]
            }
        }
    }
}

fn nearest_corridor(x: f32) -> f32 {
    CORRIDORS_X
        .into_iter()
        .min_by(|a, b| (a - x).abs().total_cmp(&(b - x).abs()))
        .unwrap_or(CORRIDORS_X[0])
}

/// Waypoints for walking from `from` to `to`, by way of the back aisle.
pub fn route(
    from: (f32, f32),
    from_place: Option<Place>,
    to: Option<Place>,
    to_point: (f32, f32),
) -> Vec<(f32, f32)> {
    let mut path = Vec::new();
    // Retrace the way out to the aisle.
    if let Some(p) = from_place {
        let mut back = p.route_from_aisle();
        back.pop();
        back.reverse();
        path.extend(back);
    } else if (from.1 - AISLE_Y).abs() > 0.5 {
        path.push((from.0, AISLE_Y));
    }
    match to {
        Some(p) => path.extend(p.route_from_aisle()),
        None => {
            path.push((to_point.0, AISLE_Y));
            path.push(to_point);
        }
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_place_is_in_the_room() {
        let places = (0..SEATS)
            .map(Place::Seat)
            .chain((0..STANDING_SPOTS).map(Place::Standing));
        for p in places {
            let (x, y) = p.spot();
            assert!(x >= 6.0 && x <= ROOM_W as f32 - 6.0, "{p:?} x={x}");
            assert!(y > AISLE_Y && y < ROOM_H as f32, "{p:?} y={y}");
        }
    }

    #[test]
    fn seat_order_covers_every_seat_once() {
        let mut seen: Vec<usize> = seat_order().collect();
        seen.sort();
        assert_eq!(seen, (0..SEATS).collect::<Vec<_>>());
    }

    #[test]
    fn routes_end_at_their_destination() {
        let r = route(DOOR, None, Some(Place::Seat(7)), (0.0, 0.0));
        assert_eq!(*r.last().unwrap(), Place::Seat(7).spot());
        let r = route(Place::Seat(7).spot(), Some(Place::Seat(7)), None, PICKUP);
        assert_eq!(*r.last().unwrap(), PICKUP);
    }
}

//! The room itself: walls, furniture, lighting, and the view through the window.

use std::sync::OnceLock;

use super::art::art;
use crate::cafe::geometry::{
    COUNTER_BOTTOM, COUNTER_RIGHT, COUNTER_TOP, LAMP_Y, LAMPS_X, ROOM_H, ROOM_W, TABLES, Table,
};
use crate::engine::{Canvas, Rgb};
use crate::rng::{Rng, hash01};

const CEILING: Rgb = Rgb::hex(0x2a1c12);
const MORTAR: Rgb = Rgb::hex(0x53301f);
const BRICKS: [Rgb; 4] = [
    Rgb::hex(0x9c4733),
    Rgb::hex(0x8a3d2b),
    Rgb::hex(0xa9533b),
    Rgb::hex(0x7f3727),
];
const SILL_TOP: Rgb = Rgb::hex(0xb8773f);
const SILL_FRONT: Rgb = Rgb::hex(0x8c5530);
const WAINSCOT: Rgb = Rgb::hex(0x7a4c44);
const WAINSCOT_LINE: Rgb = Rgb::hex(0x5c3832);
const WAINSCOT_RAIL: Rgb = Rgb::hex(0x9a6450);
const BASEBOARD: Rgb = Rgb::hex(0x2c1a10);
const FLOOR: [Rgb; 3] = [Rgb::hex(0x5c3a28), Rgb::hex(0x543421), Rgb::hex(0x62402b)];
const FLOOR_SEAM: Rgb = Rgb::hex(0x2e1c13);
const BOARD_FRAME: Rgb = Rgb::hex(0x5e3c22);
pub const BOARD: Rgb = Rgb::hex(0x223029);
const COUNTER_TOP_C: Rgb = Rgb::hex(0xd9cdb5);
const COUNTER_EDGE: Rgb = Rgb::hex(0xa3957f);
const COUNTER_FRONT: Rgb = Rgb::hex(0x5c3824);
const COUNTER_LINE: Rgb = Rgb::hex(0x46291a);
const TABLE_TOP: Rgb = Rgb::hex(0x9a6644);
const TABLE_EDGE: Rgb = Rgb::hex(0x6c4430);
const TABLE_LEG: Rgb = Rgb::hex(0x3a2418);
const SHELF_BACK: Rgb = Rgb::hex(0x4c2c1e);
const SHELF_WOOD: Rgb = Rgb::hex(0x7c4c2e);
const BOOKS: [Rgb; 6] = [
    Rgb::hex(0xb5473b),
    Rgb::hex(0x3f7d5c),
    Rgb::hex(0x3d5f9e),
    Rgb::hex(0xcfa443),
    Rgb::hex(0x3f8c8c),
    Rgb::hex(0x6d4c8e),
];
const DOOR_FRAME: Rgb = Rgb::hex(0x3c2315);
const DOOR_SLAB: Rgb = Rgb::hex(0x6e4126);
const DOOR_PANEL: Rgb = Rgb::hex(0x5a3320);
const DOOR_KNOB: Rgb = Rgb::hex(0xe2b450);
const DOORWAY: Rgb = Rgb::hex(0x0f0d18);
const CORD: Rgb = Rgb::hex(0x1a1410);

pub const WINDOW_PANES: [(i32, i32); 3] = [(45, 61), (63, 79), (81, 97)];
pub const WINDOW_TOP: i32 = 4;
pub const WINDOW_BOTTOM: i32 = 32;
pub const DOOR_WINDOW: (i32, i32, i32, i32) = (119, 14, 129, 25);
pub const MACHINE_AT: (i32, i32) = (5, 28);
const PASTRY_AT: (i32, i32) = (27, 37);
const PLANT_AT: (i32, i32) = (132, 30);
const MOON: (f32, f32) = (89.0, 10.0);

/// The parts of the room that never change, unlit.
pub fn background() -> &'static Canvas {
    static BG: OnceLock<Canvas> = OnceLock::new();
    BG.get_or_init(paint_background)
}

fn paint_background() -> Canvas {
    let mut c = Canvas::new(ROOM_W, ROOM_H, CEILING);
    let (w, h) = (ROOM_W as i32, ROOM_H as i32);

    // Brick wall.
    for y in 2..46 {
        let row = (y - 2) / 3;
        for x in 0..w {
            let off = if row % 2 == 0 { 0 } else { 3 };
            let col = (x + off) / 6;
            let mortar = (y - 2) % 3 == 2 || (x + off) % 6 == 5;
            let brick = BRICKS
                [(hash01(row as i64, col as i64) * BRICKS.len() as f32) as usize % BRICKS.len()];
            c.set(x, y, if mortar { MORTAR } else { brick });
        }
    }

    // Floor planks, staggered.
    for y in 46..h {
        let row = (y - 46) / 4;
        for x in 0..w {
            let off = (row * 9) % 22;
            let seam = (y - 46) % 4 == 3 || (x + off) % 22 == 0;
            let plank =
                FLOOR[(hash01(row as i64 + 50, ((x + off) / 22) as i64) * 3.0) as usize % 3];
            c.set(x, y, if seam { FLOOR_SEAM } else { plank });
        }
    }

    // Chalkboard.
    c.fill(4, 3, 34, 25, BOARD_FRAME);
    c.fill(5, 4, 32, 23, BOARD);
    let mut rng = Rng::new(3);
    for _ in 0..18 {
        let (x, y) = (
            5 + (rng.next_f32() * 32.0) as i32,
            4 + (rng.next_f32() * 23.0) as i32,
        );
        c.set(x, y, BOARD.mix(Rgb::hex(0x9aa89e), 0.12));
    }

    // Window frame and sill; the panes are filled each frame.
    c.fill(44, 3, 55, 31, DOOR_FRAME);
    c.fill(42, 33, 59, 1, SILL_TOP);
    c.fill(42, 34, 59, 2, SILL_FRONT);

    // Wainscot below the window, between the counter and the door.
    c.fill(COUNTER_RIGHT, 36, 114 - COUNTER_RIGHT, 9, WAINSCOT);
    c.hline(COUNTER_RIGHT, 36, 114 - COUNTER_RIGHT, WAINSCOT_RAIL);
    for x in (COUNTER_RIGHT + 4..114).step_by(9) {
        c.vline(x, 37, 8, WAINSCOT_LINE);
    }
    c.hline(0, 45, w, BASEBOARD);

    // Bookshelf.
    c.fill(100, 9, 14, 22, SHELF_BACK);
    c.vline(100, 9, 22, SHELF_WOOD);
    c.vline(113, 9, 22, SHELF_WOOD);
    c.hline(100, 9, 14, SHELF_WOOD);
    for shelf_y in [19, 30] {
        c.hline(100, shelf_y, 14, SHELF_WOOD);
        let mut x = 101;
        let mut i = shelf_y as usize;
        while x < 113 {
            let bw = 1 + (hash01(x as i64, shelf_y as i64) * 2.0) as i32;
            let bh = 5 + (hash01(shelf_y as i64, x as i64) * 4.0) as i32;
            if hash01(x as i64 * 3, shelf_y as i64) > 0.15 {
                c.fill(x, shelf_y - bh, bw.min(113 - x), bh, BOOKS[i % BOOKS.len()]);
            }
            x += bw + if hash01(x as i64, 9) > 0.8 { 1 } else { 0 };
            i += 1;
        }
    }

    // Door frame; the slab is drawn each frame so it can open.
    c.fill(114, 10, 20, 36, DOOR_FRAME);

    // Plant in the corner.
    let props = &art().props;
    c.sprite(
        props.get("plant"),
        PLANT_AT.0,
        PLANT_AT.1,
        &props.palette,
        &[],
    );

    // Lamp cords and shades.
    for lx in LAMPS_X {
        c.vline(lx, 0, LAMP_Y, CORD);
        c.sprite(props.get("lamp"), lx - 3, LAMP_Y, &props.palette, &[]);
    }

    // Tables.
    for t in TABLES {
        paint_table(&mut c, t);
    }
    c
}

/// The counter, drawn in front of the Barista.
pub fn paint_counter(c: &mut Canvas) {
    let props = &art().props;
    c.fill(0, COUNTER_TOP, COUNTER_RIGHT + 1, 1, COUNTER_TOP_C);
    c.fill(0, COUNTER_TOP + 1, COUNTER_RIGHT + 1, 1, COUNTER_EDGE);
    c.fill(
        0,
        COUNTER_TOP + 2,
        COUNTER_RIGHT,
        COUNTER_BOTTOM - COUNTER_TOP - 2,
        COUNTER_FRONT,
    );
    for x in (3..COUNTER_RIGHT).step_by(7) {
        c.vline(
            x,
            COUNTER_TOP + 3,
            COUNTER_BOTTOM - COUNTER_TOP - 5,
            COUNTER_LINE,
        );
    }
    c.hline(0, COUNTER_BOTTOM - 2, COUNTER_RIGHT, COUNTER_LINE);
    c.sprite(
        props.get("pastry_case"),
        PASTRY_AT.0,
        PASTRY_AT.1,
        &props.palette,
        &[],
    );
}

pub fn paint_machine(c: &mut Canvas) {
    let props = &art().props;
    c.sprite(
        props.get("espresso"),
        MACHINE_AT.0,
        MACHINE_AT.1,
        &props.palette,
        &[],
    );
}

fn paint_table(c: &mut Canvas, t: Table) {
    c.hline(t.x, t.y, t.w, TABLE_TOP);
    c.hline(t.x, t.y + 1, t.w, TABLE_EDGE);
    let mid = t.x + t.w / 2;
    c.fill(mid - 1, t.y + 2, 2, 8, TABLE_LEG);
    c.fill(mid - 4, t.y + 10, 8, 1, TABLE_LEG);
}

/// Redraws one table on top of whatever sits behind it.
pub fn repaint_table(c: &mut Canvas, t: Table) {
    paint_table(c, t);
}

pub fn paint_door(c: &mut Canvas, open: bool, time: f64) {
    if open {
        c.fill(116, 12, 17, 34, DOORWAY);
        paint_sky(c, 116, 12, 132, 30, time, 5);
        c.fill(130, 12, 3, 34, DOOR_PANEL);
        return;
    }
    c.fill(116, 12, 17, 34, DOOR_SLAB);
    for (y, hgt) in [(29, 6), (37, 6)] {
        c.hline(118, y, 13, DOOR_PANEL);
        c.hline(118, y + hgt, 13, DOOR_PANEL);
        c.vline(118, y, hgt, DOOR_PANEL);
        c.vline(130, y, hgt + 1, DOOR_PANEL);
    }
    c.set(118, 34, DOOR_KNOB);
    c.set(118, 35, DOOR_KNOB.scale(0.7));
}

/// How brightly each pixel is lit: dim ambient light plus three warm lamps.
pub fn light_map() -> &'static [[f32; 3]] {
    static MAP: OnceLock<Vec<[f32; 3]>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut v = Vec::with_capacity(ROOM_W * ROOM_H);
        for y in 0..ROOM_H {
            for x in 0..ROOM_W {
                v.push(light_at(x as f32, y as f32));
            }
        }
        v
    })
}

fn light_at(x: f32, y: f32) -> [f32; 3] {
    let mut l = [0.45, 0.41, 0.45];
    for lx in LAMPS_X {
        let lx = lx as f32;
        let by = LAMP_Y as f32 + 5.0;
        let d = y - by;
        let mut w = 0.0;
        if d > 0.0 {
            let spread = 4.0 + d * 0.5;
            let across = (1.0 - (x - lx).abs() / spread).max(0.0);
            w += across.powf(1.3) * (1.0 - d / 85.0).max(0.0) * 0.62;
        }
        let r = ((x - lx).powi(2) + (y - by).powi(2)).sqrt();
        w += (1.0 - r / 16.0).max(0.0).powi(2) * 0.55;
        l[0] += w;
        l[1] += w * 0.85;
        l[2] += w * 0.6;
    }
    // Vignette toward the edges.
    let nx = (x / ROOM_W as f32 - 0.5) * 2.0;
    let ny = ((y / ROOM_H as f32 - 0.5) * 2.0).max(0.0);
    let v = 1.0 - 0.3 * (nx * nx * 0.55 + ny * ny * 0.9).min(1.0);
    l.map(|c| (c * v).min(1.2))
}

pub fn apply_light(c: &mut Canvas) {
    let map = light_map();
    let w = c.w;
    c.map(|x, y, p| {
        let [r, g, b] = map[y * w + x];
        p.tint(r, g, b)
    });
}

/// Night sky with buildings, lit windows, and stars, in the given rectangle.
fn paint_sky(c: &mut Canvas, x0: i32, y0: i32, x1: i32, y1: i32, time: f64, seed: i64) {
    for y in y0..=y1 {
        let t = (y - y0) as f32 / (y1 - y0).max(1) as f32;
        let sky = Rgb::hex(0x121128).mix(Rgb::hex(0x231c36), t);
        for x in x0..=x1 {
            c.set(x, y, sky);
        }
    }
    // Stars.
    for i in 0..((x1 - x0) * (y1 - y0) / 26) {
        let sx = x0 + (hash01(seed, i as i64) * (x1 - x0 + 1) as f32) as i32;
        let sy = y0 + (hash01(i as i64, seed) * (y1 - y0) as f32 * 0.7) as i32;
        let tw = (((time as f32) * (0.6 + hash01(i as i64, 77) * 1.4) + i as f32).sin() * 0.5
            + 0.5)
            .powi(2);
        let base = c.get(sx.max(0) as usize, sy.max(0) as usize);
        c.set(sx, sy, base.mix(Rgb::hex(0xd8d8f0), 0.2 + tw * 0.5));
    }
}

/// The window: three panes of night city with the moon.
pub fn paint_window(c: &mut Canvas, time: f64) {
    for (pi, (x0, x1)) in WINDOW_PANES.into_iter().enumerate() {
        paint_sky(c, x0, WINDOW_TOP, x1, WINDOW_BOTTOM, time, pi as i64 + 1);
        // Buildings.
        let mut x = x0;
        let mut b = pi as i64 * 10;
        while x <= x1 {
            let bw = 3 + (hash01(b, 1) * 5.0) as i32;
            let top = 11 + (hash01(b, 2) * 14.0) as i32;
            let shade = Rgb::hex(0x0c0b1a).mix(Rgb::hex(0x17142a), hash01(b, 3));
            for bx in x..(x + bw).min(x1 + 1) {
                for by in top..=WINDOW_BOTTOM {
                    c.set(bx, by, shade);
                }
            }
            // Lit windows, a few switching on and off.
            for wy in (top + 2..WINDOW_BOTTOM).step_by(2) {
                for wx in (x + 1..(x + bw - 1).min(x1)).step_by(2) {
                    let k = (wx as i64) * 131 + wy as i64;
                    let period = 4.0 + hash01(k, 5) as f64 * 9.0;
                    let epoch = ((time + hash01(k, 6) as f64 * period) / period).floor() as i64;
                    if hash01(k, epoch) > 0.8 {
                        let warm = Rgb::hex(0xd8b25a).mix(Rgb::hex(0xb98a3c), hash01(k, 8));
                        c.set(wx, wy, warm);
                    }
                }
            }
            x += bw;
            b += 1;
        }
    }
    // The moon, in the right-hand pane.
    let (mx, my) = MOON;
    for y in (my as i32 - 5)..=(my as i32 + 5) {
        for x in (mx as i32 - 5)..=(mx as i32 + 5) {
            if !(WINDOW_PANES[2].0..=WINDOW_PANES[2].1).contains(&x) || y < WINDOW_TOP {
                continue;
            }
            let d = ((x as f32 - mx).powi(2) + (y as f32 - my).powi(2)).sqrt();
            if d <= 2.6 {
                let crater = (x == mx as i32 + 1 && y == my as i32)
                    || (x == mx as i32 - 1 && y == my as i32 + 1);
                c.set(
                    x,
                    y,
                    if crater {
                        Rgb::hex(0xc9c3a4)
                    } else {
                        Rgb::hex(0xf2eed4)
                    },
                );
            } else if d <= 5.0 {
                c.blend(x, y, Rgb::hex(0x6a6a8a), 0.25 * (1.0 - (d - 2.6) / 2.4));
            }
        }
    }
    // Faint glass reflection.
    for (x0, _) in WINDOW_PANES {
        for i in 0..6 {
            c.blend(x0 + 2 + i, WINDOW_TOP + 1 + i, Rgb::hex(0xffffff), 0.06);
        }
    }
}

pub fn paint_door_window(c: &mut Canvas, time: f64) {
    let (x0, y0, x1, y1) = DOOR_WINDOW;
    paint_sky(c, x0, y0, x1, y1, time, 9);
    c.vline((x0 + x1) / 2, y0, y1 - y0 + 1, DOOR_FRAME);
}

/// Bulbs and their halos, which the lighting pass mustn't dim.
pub fn paint_lamp_glow(c: &mut Canvas) {
    for lx in LAMPS_X {
        let by = LAMP_Y + 5;
        for dy in -3..=3 {
            for dx in -4..=4 {
                let d = ((dx * dx + dy * dy) as f32).sqrt();
                if dy >= 0 && d < 4.5 {
                    c.blend(lx + dx, by + dy, Rgb::hex(0xffd98a), 0.22 * (1.0 - d / 4.5));
                }
            }
        }
        c.hline(lx - 1, by, 3, Rgb::hex(0xfff0b8));
    }
}

/// The espresso machine's indicator lights.
pub fn paint_machine_lights(c: &mut Canvas, time: f64) {
    let (mx, my) = MACHINE_AT;
    c.set(mx + 2, my + 4, Rgb::hex(0xff5a4a));
    let blink = (time * 1.3).fract() < 0.8;
    c.set(
        mx + 5,
        my + 4,
        if blink {
            Rgb::hex(0x5af07e)
        } else {
            Rgb::hex(0x2a7040)
        },
    );
    c.hline(mx + 8, my + 4, 3, Rgb::hex(0xf2c65a));
}

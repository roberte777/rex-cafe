//! Turns the Café into a terminal frame, and knows what is under each cell.

pub mod art;
pub mod room;

use art::art;
use room::{
    BOARD, apply_light, background, light_map, paint_counter, paint_door, paint_door_window,
    paint_lamp_glow, paint_machine, paint_machine_lights, paint_window, repaint_table,
};

use crate::agents::app_display_name;
use crate::cafe::geometry::{BARISTA_Y, Place, ROOM_H, ROOM_W, TABLES, seat_table};
use crate::cafe::{Activity, Cafe, Clawd, HATS};
use crate::engine::{Canvas, Rgb, Screen, Sprite};

const OUTSIDE_BG: Rgb = Rgb::hex(0x0e0b09);
const PANEL_BG: Rgb = Rgb::hex(0x16110e);
const SELECTED_BG: Rgb = Rgb::hex(0x3a2e26);
const NAME: Rgb = Rgb::hex(0xddd3c6);
const DIM: Rgb = Rgb::hex(0x7d7368);
const FAINT: Rgb = Rgb::hex(0x5e564d);
const CHALK: Rgb = Rgb::hex(0xd9dccf);
const CHALK_DIM: Rgb = Rgb::hex(0x9aa197);
const WORKING: Rgb = Rgb::hex(0x5fb7c4);
const NEEDS_YOU: Rgb = Rgb::hex(0xf0a64a);
const DONE: Rgb = Rgb::hex(0x79c27a);
const SPILLED: Rgb = Rgb::hex(0xe0675a);
const BUBBLE_BG: Rgb = Rgb::hex(0xefe6d6);
const BUBBLE_FG: Rgb = Rgb::hex(0x2a1d18);
const BUBBLE_DIM: Rgb = Rgb::hex(0x6f5f55);

const LEGEND_COLS: usize = 4;
const ROOM_ROWS: usize = ROOM_H / 2;

/// What the user's pointer is over.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Agent(String),
    Door,
}

/// Transient things the view shows that the Café doesn't own.
#[derive(Clone, Debug, Default)]
pub struct Ui {
    pub selected: Option<String>,
    pub hover: Option<String>,
    pub notice: Option<String>,
    pub demo: bool,
}

pub struct Frame {
    pub screen: Screen,
    hits: Vec<(i32, i32, i32, i32, Target)>,
}

impl Frame {
    /// The topmost thing at a cell.
    pub fn hit(&self, col: u16, row: u16) -> Option<&Target> {
        let (c, r) = (col as i32, row as i32);
        self.hits
            .iter()
            .rev()
            .find(|(x0, y0, x1, y1, _)| c >= *x0 && c < *x1 && r >= *y0 && r < *y1)
            .map(|h| &h.4)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Layout {
    scale: usize,
    ox: i32,
    oy: i32,
    legend_rows: usize,
}

fn layout(cols: u16, rows: u16, agents: usize) -> Option<Layout> {
    let wanted = agents.div_ceil(LEGEND_COLS).max(1);
    for scale in [2, 1] {
        let (room_w, room_h) = (ROOM_W * scale, ROOM_ROWS * scale);
        if (cols as usize) < room_w || (rows as usize) < room_h + 4 {
            continue;
        }
        let legend_rows = wanted.min((rows as usize - room_h - 2) / 2);
        let total = room_h + 1 + legend_rows * 2 + 1;
        return Some(Layout {
            scale,
            ox: ((cols as usize - room_w) / 2) as i32,
            oy: ((rows as usize - total) / 2) as i32,
            legend_rows,
        });
    }
    None
}

/// The smallest terminal that fits the room.
pub const MIN_SIZE: (usize, usize) = (ROOM_W, ROOM_ROWS + 4);

pub fn compose(cafe: &Cafe, ui: &Ui, cols: u16, rows: u16, wall_now: f64) -> Frame {
    let mut screen = Screen::new(cols, rows, OUTSIDE_BG);
    let mut hits = Vec::new();
    let legend = cafe.legend();
    let Some(l) = layout(cols, rows, legend.len()) else {
        compose_list(cafe, ui, &mut screen, &mut hits, wall_now);
        return Frame { screen, hits };
    };
    let s = l.scale as i32;
    let cell = |x: i32, y: i32| (l.ox + x * s, l.oy + y * s / 2);

    let canvas = paint(cafe, ui.selected.as_deref());
    screen.blit(&canvas, l.ox, l.oy, l.scale);

    // The Menu, chalked on the board.
    let counts = cafe.menu_counts();
    let (mx, my) = cell(6, 8);
    screen.text(mx + 3, my, "TODAY'S MENU", CHALK, Some(BOARD));
    let items = [
        ("Espresso", counts[0]),
        ("Cortado", counts[1]),
        ("Flat white", counts[2]),
        ("Done", counts[3]),
    ];
    for (i, (drink, n)) in items.iter().enumerate() {
        let dots = ".".repeat(17 - drink.len() - 1);
        screen.text(
            mx,
            my + 2 + i as i32,
            &format!("{drink} {dots} {n}"),
            CHALK_DIM,
            Some(BOARD),
        );
    }
    screen.text(mx, my + 7, "wifi: rex-cafe", CHALK_DIM, Some(BOARD));

    if !cafe.open {
        let (sx, sy) = cell(121, 18);
        screen.text(
            sx,
            sy,
            "CLOSED",
            Rgb::hex(0xb3352c),
            Some(Rgb::hex(0xe9dcc0)),
        );
    }

    // Hit areas: the door, then Clawds back to front.
    let (dx0, dy0) = cell(114, 10);
    let (dx1, dy1) = cell(134, 46);
    hits.push((dx0, dy0, dx1, dy1, Target::Door));
    for c in depth_sorted(cafe) {
        if c.is_leaving() {
            continue;
        }
        let (x0, y0, x1, y1) = clawd_box(c);
        let (cx0, cy0) = cell(x0, y0);
        let (cx1, cy1) = cell(x1, y1 + 1);
        hits.push((cx0, cy0, cx1, cy1 + 1, Target::Agent(c.agent.block.clone())));
    }

    // Legend.
    let room_w = (ROOM_W * l.scale) as i32;
    let top = l.oy + (ROOM_ROWS * l.scale) as i32 + 1;
    screen.fill(
        l.ox,
        top - 1,
        room_w,
        (l.legend_rows * 2 + 2) as i32,
        PANEL_BG,
    );
    let col_w = room_w / LEGEND_COLS as i32;
    let slots = l.legend_rows * LEGEND_COLS;
    let overflow = legend.len() > slots;
    for (i, c) in legend.iter().enumerate() {
        if overflow && i + 1 >= slots {
            let x = l.ox + 2 + (i % LEGEND_COLS) as i32 * col_w;
            let y = top + (i / LEGEND_COLS) as i32 * 2;
            screen.text(x, y, &format!("+{} more", legend.len() - i), DIM, None);
            break;
        }
        let x = l.ox + 2 + (i % LEGEND_COLS) as i32 * col_w;
        let y = top + (i / LEGEND_COLS) as i32 * 2;
        let selected = ui.selected.as_deref() == Some(c.agent.block.as_str());
        legend_entry(
            &mut screen,
            c,
            x,
            y,
            col_w - 3,
            selected,
            cafe.time,
            wall_now,
        );
        hits.push((
            x,
            y,
            x + col_w - 2,
            y + 2,
            Target::Agent(c.agent.block.clone()),
        ));
    }
    footer(
        &mut screen,
        cafe,
        ui,
        l.ox + 2,
        top + (l.legend_rows * 2) as i32,
        room_w - 4,
    );

    // Hover bubble last, over everything.
    if let Some(c) = ui.hover.as_deref().and_then(|b| cafe.clawd(b)) {
        let lines = bubble_lines(c, cafe.time, wall_now);
        let (anchor_x, anchor_y) = if c.is_outside() {
            let i = legend
                .iter()
                .position(|l| l.agent.block == c.agent.block)
                .unwrap_or(0);
            (
                l.ox + 2 + (i % LEGEND_COLS) as i32 * col_w + 4,
                top + (i / LEGEND_COLS) as i32 * 2 - 1,
            )
        } else {
            let (x0, y0, x1, _) = clawd_box(c);
            let (ax, ay) = cell((x0 + x1) / 2, y0 - 9);
            (ax, ay)
        };
        bubble(&mut screen, &lines, anchor_x, anchor_y, l.ox, l.ox + room_w);
    }
    Frame { screen, hits }
}

/// The pixel scene: room, characters, lighting, and glowing things.
pub fn paint(cafe: &Cafe, selected: Option<&str>) -> Canvas {
    let a = art();
    let time = cafe.time;
    let mut c = background().clone();
    paint_door(&mut c, cafe.door_open(), time);
    paint_machine(&mut c);
    if cafe.barista.present() {
        let bx = cafe.barista.x.round() as i32;
        let name = if cafe.barista.serving() {
            "barista_serve"
        } else {
            "barista"
        };
        c.sprite(a.chars.get(name), bx, BARISTA_Y, &a.chars.palette, &[]);
        if cafe.barista.holding_cup() {
            let cx = if cafe.barista.serving() {
                bx + 15
            } else {
                bx + 12
            };
            c.sprite(a.chars.get("cup"), cx, BARISTA_Y + 8, &a.chars.palette, &[]);
        }
    }
    paint_counter(&mut c);

    enum Item<'a> {
        Table(usize),
        Clawd(&'a Clawd),
    }
    let mut items: Vec<(f32, Item)> = TABLES
        .iter()
        .enumerate()
        .map(|(i, t)| (t.y as f32, Item::Table(i)))
        .collect();
    items.extend(cafe.clawds_in_room().map(|cl| (depth(cl), Item::Clawd(cl))));
    items.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, item) in &items {
        match item {
            Item::Table(i) => repaint_table(&mut c, TABLES[*i]),
            Item::Clawd(cl) => draw_clawd(&mut c, cl, time),
        }
    }

    apply_light(&mut c);

    paint_window(&mut c, time);
    if !cafe.door_open() {
        paint_door_window(&mut c, time);
    }
    if !cafe.open {
        c.fill(118, 18, 14, 2, Rgb::hex(0xe9dcc0));
        c.vline(121, 15, 3, Rgb::hex(0x8a7a60));
        c.vline(128, 15, 3, Rgb::hex(0x8a7a60));
    }
    let cat = &cafe.ambient.cat;
    let cat_sprite = if cat.sitting {
        a.props.get("cat_sit").clone()
    } else if (cat.stride * 4.0) as i32 % 2 == 0 {
        a.props.get("cat_a").clone()
    } else {
        a.props.get("cat_b").clone()
    };
    let cat_sprite = if cat.facing_left {
        cat_sprite.flipped()
    } else {
        cat_sprite
    };
    sprite_lit(
        &mut c,
        &cat_sprite,
        cat.x.round() as i32,
        28,
        &a.props.palette,
        1.15,
    );
    paint_lamp_glow(&mut c);
    paint_machine_lights(&mut c, time);

    // Steam from the machine while brewing.
    if cafe.barista.brewing() {
        for i in 0..3 {
            let t = (time * 2.0 + i as f64 * 0.33).fract() as f32;
            let x = room::MACHINE_AT.0 + 3 + i * 4 + ((time * 3.0 + i as f64).sin() * 1.2) as i32;
            c.blend(
                x,
                room::MACHINE_AT.1 - 1 - (t * 6.0) as i32,
                Rgb::hex(0xe8e2da),
                0.5 * (1.0 - t),
            );
        }
    }

    // Dust in the lamplight.
    for m in &cafe.ambient.motes {
        let tw = ((time as f32 * 1.7 + m.phase).sin() * 0.5 + 0.5) * 0.35 + 0.1;
        c.blend(
            m.x.round() as i32,
            m.y.round() as i32,
            Rgb::hex(0xffe9b8),
            tw,
        );
    }

    for cl in cafe.clawds_in_room() {
        overlays(&mut c, cl, time, selected == Some(cl.agent.block.as_str()));
    }
    c
}

fn depth(c: &Clawd) -> f32 {
    if c.is_seated() {
        c.pos.1 - 2.0
    } else {
        c.pos.1
    }
}

fn depth_sorted(cafe: &Cafe) -> Vec<&Clawd> {
    let mut v: Vec<&Clawd> = cafe.clawds_in_room().collect();
    v.sort_by(|a, b| depth(a).total_cmp(&depth(b)));
    v
}

/// A Clawd's on-screen bounds in canvas pixels, hat included: `(x0, y0, x1, y1)`.
fn clawd_box(c: &Clawd) -> (i32, i32, i32, i32) {
    let left = c.pos.0.round() as i32 - 6;
    let top = clawd_top(c);
    (
        left,
        top - 3,
        left + 12,
        if c.is_seated() { top + 7 } else { top + 9 },
    )
}

fn clawd_top(c: &Clawd) -> i32 {
    match (c.is_seated(), c.place) {
        (true, Some(Place::Seat(i))) => seat_table(i).y - 8,
        _ => c.pos.1.round() as i32 - 9,
    }
}

fn hat_overrides(c: &Clawd) -> [(char, Rgb); 2] {
    let hat = HATS[c.hat];
    [('H', hat.color), ('h', hat.shade)]
}

fn draw_clawd(cv: &mut Canvas, c: &Clawd, time: f64) {
    let a = art();
    let p = &a.chars.palette;
    let hat = hat_overrides(c);
    let left = c.pos.0.round() as i32 - 6;
    let top = clawd_top(c);
    let activity = c.activity();
    let phase = |rate: f64| (time * rate) as i64 % 2 == 0;

    if c.is_seated() {
        let t = match c.place {
            Some(Place::Seat(i)) => seat_table(i),
            _ => unreachable!("seated Clawds have a Seat"),
        };
        let body = match activity {
            Activity::CoffeeBreak => {
                if (time * 0.4 + c.order as f64).fract() < 0.08 {
                    "clawd_sit"
                } else {
                    "clawd_sit_sleepy"
                }
            }
            Activity::Working => {
                if phase(5.0) {
                    "clawd_type_a"
                } else {
                    "clawd_type_b"
                }
            }
            Activity::NeedsYou(_) => {
                if phase(2.5) {
                    "clawd_wave"
                } else {
                    "clawd_sit"
                }
            }
            Activity::Done => {
                if (time * 0.5).fract() < 0.3 {
                    "clawd_cheer"
                } else {
                    "clawd_sit"
                }
            }
            Activity::Spilled => "clawd_sad",
            _ => "clawd_sit",
        };
        cv.sprite(a.chars.get(body), left, top, p, &hat);
        cv.sprite(a.chars.get(c_hat(c)), left, top - 3, p, &hat);
        let cx = left + 5;
        match activity {
            Activity::Working => cv.sprite(a.chars.get("laptop"), cx - 4, t.y - 5, p, &[]),
            Activity::Spilled => cv.sprite(a.chars.get("cup_tipped"), cx - 2, t.y - 2, p, &[]),
            _ => cv.sprite(a.chars.get("cup"), cx, t.y - 3, p, &[]),
        }
        return;
    }

    let walking = c.is_walking();
    let body = if walking && (c.stride * 7.0) as i32 % 2 == 1 {
        "clawd_walk"
    } else {
        "clawd_stand"
    };
    let bob = if walking && (c.stride * 7.0) as i32 % 2 == 1 {
        -1
    } else {
        0
    };
    cv.sprite(a.chars.get(body), left, top + bob, p, &hat);
    cv.sprite(a.chars.get(c_hat(c)), left, top - 3 + bob, p, &hat);
    let holding =
        c.has_cup || (!walking && matches!(activity, Activity::CoffeeBreak | Activity::Done));
    if holding {
        let x = if c.facing_left { left - 2 } else { left + 11 };
        cv.sprite(a.chars.get("cup"), x, top + 3 + bob, p, &[]);
    }
}

fn c_hat(c: &Clawd) -> &'static str {
    HATS[c.hat].sprite
}

/// Bright things above a Clawd's head, drawn after lighting.
fn overlays(cv: &mut Canvas, c: &Clawd, time: f64, selected: bool) {
    let a = art();
    let p = &a.chars.palette;
    let (x0, y0, _, _) = clawd_box(c);
    let bob = if (time * 2.0).fract() < 0.5 { 0 } else { -1 };
    let activity = c.activity();
    match activity {
        Activity::NeedsYou(_) => {
            cv.sprite(a.chars.get("bubble_alert"), x0 + 8, y0 - 7 + bob, p, &[])
        }
        Activity::Done => cv.sprite(a.chars.get("bubble_done"), x0 + 8, y0 - 7 + bob, p, &[]),
        Activity::Spilled => {
            cv.sprite(a.chars.get("cloud"), x0 + 2, y0 - 7, p, &[]);
            for i in 0..3 {
                let t = ((time * 1.6) + i as f64 / 3.0).fract();
                cv.blend(
                    x0 + 3 + i * 2,
                    y0 - 4 + (t * 3.0) as i32,
                    Rgb::hex(0x7fa6e0),
                    0.8,
                );
            }
        }
        Activity::CoffeeBreak if c.is_seated() => {
            let t = ((time * 0.5) + c.order as f64 * 0.37).fract() as f32;
            if t < 0.6 {
                let cup_x = x0 + 6;
                let y = y0 + 3 + 9 - 4 - (t * 7.0) as i32;
                cv.blend(
                    cup_x + ((t * 9.0).sin() * 1.0) as i32,
                    y,
                    Rgb::hex(0xe8e2da),
                    0.35 * (1.0 - t / 0.6),
                );
            }
        }
        _ => {}
    }
    if selected {
        let y = y0 - 5 + bob;
        let x = if matches!(activity, Activity::NeedsYou(_) | Activity::Done) {
            x0 + 1
        } else {
            x0 + 3
        };
        cv.sprite(a.chars.get("marker"), x, y, p, &[]);
    }
}

/// Draws a sprite with the room's lighting applied, for things drawn after the lighting pass.
fn sprite_lit(
    cv: &mut Canvas,
    s: &Sprite,
    x: i32,
    y: i32,
    palette: &std::collections::HashMap<char, Rgb>,
    boost: f32,
) {
    let map = light_map();
    for sy in 0..s.h {
        for sx in 0..s.w {
            let Some(col) = s.key(sx, sy).and_then(|k| palette.get(&k)) else {
                continue;
            };
            let (px, py) = (x + sx as i32, y + sy as i32);
            if px < 0 || py < 0 || px as usize >= ROOM_W || py as usize >= ROOM_H {
                continue;
            }
            let [r, g, b] = map[py as usize * ROOM_W + px as usize];
            cv.set(px, py, col.tint(r * boost, g * boost, b * boost));
        }
    }
}

pub fn ago(since: Option<f64>, now: f64) -> Option<String> {
    let secs = (now - since?).max(0.0) as u64;
    Some(match secs {
        0..60 => "just now".to_string(),
        60..3600 => format!("{} min", secs / 60),
        _ => format!("{}h {}m", secs / 3600, secs / 60 % 60),
    })
}

/// The Legend's second line for a Clawd, and its colour.
pub fn activity_text(c: &Clawd, time: f64, wall_now: f64) -> (String, Rgb) {
    let since = |label: &str| match ago(c.agent.since, wall_now) {
        Some(a) => format!("{label} · {a}"),
        None => label.to_string(),
    };
    let spinner = ['◐', '◓', '◑', '◒'][(time * 6.0) as usize % 4];
    let (text, color) = match c.activity() {
        Activity::WalkingIn => ("→ walking in".to_string(), DIM),
        Activity::CoffeeBreak => (since("∗ coffee break"), DIM),
        Activity::GettingCoffee => ("→ getting coffee".to_string(), DIM),
        Activity::Working => (format!("{spinner} working"), WORKING),
        Activity::NeedsYou(on) => (format!("! needs you · {}", on.label()), NEEDS_YOU),
        Activity::Done => (since("✓ done"), DONE),
        Activity::Spilled => (since("✗ spilled"), SPILLED),
        Activity::Leaving => ("← leaving".to_string(), DIM),
    };
    if c.is_outside() {
        (format!("outside · {text}"), color)
    } else {
        (text, color)
    }
}

fn display_name(c: &Clawd) -> String {
    if c.agent.app == "claude-code" {
        c.agent.name.clone()
    } else {
        format!(
            "{} ({})",
            c.agent.name,
            app_display_name(&c.agent.app).to_lowercase()
        )
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
    t.push('…');
    t
}

#[allow(clippy::too_many_arguments)]
fn legend_entry(
    screen: &mut Screen,
    c: &Clawd,
    x: i32,
    y: i32,
    width: i32,
    selected: bool,
    time: f64,
    wall_now: f64,
) {
    let hat = HATS[c.hat];
    if selected {
        screen.fill(x - 1, y, width + 2, 2, SELECTED_BG);
    }
    let bg = if selected { SELECTED_BG } else { PANEL_BG };
    screen.text(x, y, "██", hat.color, Some(bg));
    let name_fg = if selected { Rgb::hex(0xffffff) } else { NAME };
    screen.text(
        x + 3,
        y,
        &truncate(&display_name(c), (width - 3).max(1) as usize),
        name_fg,
        Some(bg),
    );
    let (text, color) = activity_text(c, time, wall_now);
    screen.text(
        x + 3,
        y + 1,
        &truncate(&text, (width - 3).max(1) as usize),
        color,
        Some(bg),
    );
}

fn footer(screen: &mut Screen, cafe: &Cafe, ui: &Ui, x: i32, y: i32, width: i32) {
    let keys = "tab select · ⏎ jump · n new agent · r resync · q quit";
    let keys = if ui.demo {
        format!("demo · {keys}")
    } else {
        keys.to_string()
    };
    screen.text(x, y, &keys, FAINT, None);
    let right = if !cafe.open && !ui.demo {
        Some(("CLOSED · reconnecting to Rex…".to_string(), SPILLED))
    } else {
        ui.notice.clone().map(|n| (n, NEEDS_YOU))
    };
    if let Some((text, color)) = right {
        let text = truncate(
            &text,
            (width - keys.chars().count() as i32 - 3).max(0) as usize,
        );
        let len = text.chars().count() as i32;
        screen.text(x + width - len, y, &text, color, None);
    }
}

fn bubble_lines(c: &Clawd, time: f64, wall_now: f64) -> Vec<(String, Rgb)> {
    let mut lines = vec![(display_name(c), BUBBLE_FG)];
    match &c.agent.msg {
        Some(msg) => lines.push((msg.clone(), BUBBLE_DIM)),
        None => lines.push((activity_text(c, time, wall_now).0, BUBBLE_DIM)),
    }
    lines
}

/// A speech bubble whose bottom edge sits at `(cx, bottom)`, kept between `min_x` and `max_x`.
fn bubble(
    screen: &mut Screen,
    lines: &[(String, Rgb)],
    cx: i32,
    bottom: i32,
    min_x: i32,
    max_x: i32,
) {
    let width = lines
        .iter()
        .map(|(l, _)| l.chars().count())
        .max()
        .unwrap_or(0)
        .min(44) as i32
        + 2;
    let x = (cx - width / 2).clamp(min_x, (max_x - width).max(min_x));
    let y = (bottom - lines.len() as i32 + 1).max(0);
    for (i, (line, fg)) in lines.iter().enumerate() {
        let text = format!(
            " {:<w$} ",
            truncate(line, (width - 2) as usize),
            w = (width - 2) as usize
        );
        screen.text(x, y + i as i32, &text, *fg, Some(BUBBLE_BG));
    }
    screen.text(
        cx.clamp(x + 1, x + width - 2),
        y + lines.len() as i32,
        "▼",
        BUBBLE_BG,
        None,
    );
}

/// A plain list for terminals too small to fit the room.
fn compose_list(
    cafe: &Cafe,
    ui: &Ui,
    screen: &mut Screen,
    hits: &mut Vec<(i32, i32, i32, i32, Target)>,
    wall_now: f64,
) {
    let (cols, rows) = (screen.cols as i32, screen.rows as i32);
    let title = format!(
        "Rex Café · make this pane {}×{} to see the room",
        MIN_SIZE.0, MIN_SIZE.1
    );
    screen.text(
        1,
        0,
        &truncate(&title, (cols - 2).max(0) as usize),
        DIM,
        None,
    );
    let legend = cafe.legend();
    let visible = (rows - 3).max(0) as usize;
    for (i, c) in legend.iter().take(visible).enumerate() {
        let y = 2 + i as i32;
        let selected = ui.selected.as_deref() == Some(c.agent.block.as_str());
        let bg = if selected { SELECTED_BG } else { OUTSIDE_BG };
        if selected {
            screen.fill(0, y, cols, 1, bg);
        }
        screen.text(1, y, "██", HATS[c.hat].color, Some(bg));
        let name_w = ((cols - 6) / 2).max(4) as usize;
        screen.text(4, y, &truncate(&display_name(c), name_w), NAME, Some(bg));
        let (text, color) = activity_text(c, cafe.time, wall_now);
        let x = 5 + name_w as i32;
        screen.text(
            x,
            y,
            &truncate(&text, (cols - x - 1).max(0) as usize),
            color,
            Some(bg),
        );
        hits.push((0, y, cols, y + 1, Target::Agent(c.agent.block.clone())));
    }
    if legend.is_empty() {
        let text = if cafe.open {
            "No agents yet."
        } else {
            "CLOSED · reconnecting to Rex…"
        };
        screen.text(1, 2, text, if cafe.open { DIM } else { SPILLED }, None);
    }
    footer(screen, cafe, ui, 1, rows - 1, cols - 2);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_picks_the_biggest_scale_that_fits() {
        assert_eq!(layout(120, 40, 3), None);
        let l = layout(160, 52, 5).unwrap();
        assert_eq!(l.scale, 1);
        assert_eq!(l.legend_rows, 2);
        assert_eq!(l.ox, 10);
        assert_eq!(layout(300, 100, 1).unwrap().scale, 2);
    }

    #[test]
    fn ago_reads_naturally() {
        assert_eq!(ago(Some(100.0), 130.0).as_deref(), Some("just now"));
        assert_eq!(ago(Some(0.0), 150.0).as_deref(), Some("2 min"));
        assert_eq!(
            ago(Some(0.0), 3600.0 * 2.0 + 300.0).as_deref(),
            Some("2h 5m")
        );
        assert_eq!(ago(None, 1.0), None);
    }
}

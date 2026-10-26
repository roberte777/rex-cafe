//! Turns the Café into a terminal frame, and knows what is under each cell.

pub mod art;
pub mod room;

use art::art;
use room::{
    apply_flash, apply_light, background, light_map, paint_counter, paint_door, paint_door_window,
    paint_hanging_lamps, paint_lamp_glow, paint_machine, paint_machine_lights, paint_window,
    repaint_table,
};

use crate::agents::app_display_name;
use crate::audio::{Channel, LEVEL_STEP, Levels};
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
const STEAM: Rgb = Rgb::hex(0xe8e2da);

const LEGEND_COLS: usize = 4;
const ROOM_ROWS: usize = ROOM_H / 2;

/// What the user's pointer is over.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Agent(String),
    Door,
    /// Anywhere on the volume menu, so clicks there don't reach the room behind.
    VolumePanel,
    VolumeRow(Channel),
    /// A spot on a slider, in tenths.
    Volume(Channel, u8),
}

/// Transient things the view shows that the Café doesn't own.
#[derive(Clone, Debug, Default)]
pub struct Ui {
    pub selected: Option<String>,
    pub hover: Option<String>,
    pub notice: Option<String>,
    pub demo: bool,
    /// Whether sound is on; `None` when there's no sound to toggle.
    pub sound: Option<bool>,
    pub levels: Levels,
    /// The volume menu, when open, with the channel being adjusted.
    pub volume_menu: Option<Channel>,
}

/// A clickable rectangle in cells, `[x0, x1) x [y0, y1)`, and what's there.
type Hit = (i32, i32, i32, i32, Target);

pub struct Frame {
    pub screen: Screen,
    hits: Vec<Hit>,
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

#[derive(Clone, Copy, Debug, PartialEq)]
struct Layout {
    scale: f32,
    ox: i32,
    oy: i32,
    /// The room's size in cells.
    room_w: i32,
    room_h: i32,
    legend_rows: usize,
}

/// A whole-number scale wins when it keeps at least this much of the best fit,
/// trading a little size for crisp pixels.
const SNAP: f32 = 0.9;
/// Legend rows the room leaves space for, so its size doesn't jump as Agents come and go.
const RESERVED_LEGEND_ROWS: usize = 2;

/// Fits the room to the terminal: as large as it can be while keeping its
/// shape, never smaller than one cell per pixel.
fn layout(cols: u16, rows: u16, agents: usize) -> Option<Layout> {
    let (cols, rows) = (cols as i32, rows as i32);
    // The gap above the Legend and the footer below it, plus Legend rows.
    let chrome = |legend: usize| 2 + 2 * legend as i32;
    let fit = |legend: usize| {
        (cols as f32 / ROOM_W as f32).min((rows - chrome(legend)) as f32 / ROOM_ROWS as f32)
    };
    let reserved = if fit(RESERVED_LEGEND_ROWS) >= 1.0 {
        RESERVED_LEGEND_ROWS
    } else {
        1
    };
    let mut scale = fit(reserved);
    if scale < 1.0 {
        return None;
    }
    if scale.floor() / scale >= SNAP {
        scale = scale.floor();
    }
    // Matches the size `Screen::blit` draws.
    let room_w = (ROOM_W as f32 * scale + 1e-3).floor() as i32;
    let room_h = (ROOM_H as f32 * scale / 2.0 + 1e-3).floor() as i32;
    let oy = ((rows - room_h - chrome(reserved)) / 2).max(0);
    let room_for_legend = ((rows - oy - room_h - 2) / 2).max(1) as usize;
    Some(Layout {
        scale,
        ox: (cols - room_w) / 2,
        oy,
        room_w,
        room_h,
        legend_rows: agents.div_ceil(LEGEND_COLS).max(1).min(room_for_legend),
    })
}

/// The smallest terminal that fits the room.
pub const MIN_SIZE: (usize, usize) = (ROOM_W, ROOM_ROWS + 4);

pub fn compose(cafe: &Cafe, ui: &Ui, cols: u16, rows: u16, wall_now: f64) -> Frame {
    let mut screen = Screen::new(cols, rows, OUTSIDE_BG);
    let mut hits = Vec::new();
    let legend = cafe.legend();
    let Some(l) = layout(cols, rows, legend.len()) else {
        compose_list(cafe, ui, &mut screen, &mut hits, wall_now);
        volume_menu(&mut screen, &mut hits, ui, cols as i32 - VOLUME_W - 1, 1);
        return Frame { screen, hits };
    };
    // Where a canvas pixel lands on screen.
    let cell = |x: i32, y: i32| {
        (
            l.ox + (x as f32 * l.scale).floor() as i32,
            l.oy + (y as f32 * l.scale / 2.0).floor() as i32,
        )
    };

    let canvas = paint(cafe, ui.selected.as_deref());
    screen.blit(&canvas, l.ox, l.oy, l.scale);

    // The Menu, chalked on the board.
    let counts = cafe.menu_counts();
    // Even spacing in whole rows, so fractional scales don't bunch the lines.
    let gap = l.scale.round().max(1.0) as i32;
    let (tx, ty) = cell(9, 8);
    let (mx, _) = cell(6, 8);
    screen.text(tx, ty, "TODAY'S MENU", CHALK, None);
    let items = [
        ("Espresso", counts[0]),
        ("Cortado", counts[1]),
        ("Flat white", counts[2]),
        ("Done", counts[3]),
    ];
    for (i, (drink, n)) in items.iter().enumerate() {
        let dots = ".".repeat(17 - drink.len() - 1);
        let y = ty + gap * (2 + i as i32);
        screen.text(mx, y, &format!("{drink} {dots} {n}"), CHALK_DIM, None);
    }
    screen.text(mx, ty + gap * 7, "wifi: rex-cafe", CHALK_DIM, None);

    if !cafe.open {
        let (sign_left, sy) = cell(118, 18);
        let (sign_right, _) = cell(132, 18);
        screen.text(
            (sign_left + sign_right) / 2 - 3,
            sy,
            "CLOSED",
            Rgb::hex(0xb3352c),
            None,
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
    let room_w = l.room_w;
    let top = l.oy + l.room_h + 1;
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
    volume_menu(
        &mut screen,
        &mut hits,
        ui,
        l.ox + room_w - VOLUME_W - 2,
        l.oy + 1,
    );
    Frame { screen, hits }
}

const VOLUME_W: i32 = 33;
const MENU_BG: Rgb = Rgb::hex(0x1d1611);
const MENU_EDGE: Rgb = Rgb::hex(0x6e4a32);
const SLIDER_ON: Rgb = Rgb::hex(0xf0c27a);
const SLIDER_STEPS: i32 = 10;

/// The volume menu with its top-left at `(x, y)`, when it's open.
fn volume_menu(screen: &mut Screen, hits: &mut Vec<Hit>, ui: &Ui, x: i32, y: i32) {
    let Some(current) = ui.volume_menu else {
        return;
    };
    let x = x.max(0);
    let muted = ui.sound == Some(false);
    let inner = (VOLUME_W - 2) as usize;
    let rows = Channel::ALL.len() as i32;
    hits.push((x, y, x + VOLUME_W, y + rows + 4, Target::VolumePanel));

    let title = if muted {
        " volume · muted "
    } else {
        " volume "
    };
    let top = format!(
        "╭─{title}{}╮",
        "─".repeat(inner - 1 - title.chars().count())
    );
    screen.text(x, y, &top, MENU_EDGE, Some(MENU_BG));
    screen.text(
        x + 2,
        y,
        title,
        if muted { NEEDS_YOU } else { CHALK },
        Some(MENU_BG),
    );

    for (i, ch) in Channel::ALL.into_iter().enumerate() {
        let row = y + 1 + i as i32;
        let selected = ch == current;
        let bg = if selected { SELECTED_BG } else { MENU_BG };
        screen.text(x, row, "│", MENU_EDGE, Some(MENU_BG));
        screen.text(x + 1, row, &" ".repeat(inner), NAME, Some(bg));
        screen.text(x + VOLUME_W - 1, row, "│", MENU_EDGE, Some(MENU_BG));
        let marker = if selected { "▸" } else { " " };
        screen.text(
            x + 2,
            row,
            &format!("{marker} {:<7}", ch.label()),
            NAME,
            Some(bg),
        );
        let level = ui.levels.get(ch);
        let filled = (level / LEVEL_STEP).round() as i32;
        let slider_x = x + 13;
        for k in 0..SLIDER_STEPS {
            let (glyph, fg) = if k < filled {
                ("━", SLIDER_ON)
            } else {
                ("─", FAINT)
            };
            let fg = if muted { fg.mix(FAINT, 0.6) } else { fg };
            screen.text(slider_x + k, row, glyph, fg, Some(bg));
            hits.push((
                slider_x + k,
                row,
                slider_x + k + 1,
                row + 1,
                Target::Volume(ch, (k + 1) as u8),
            ));
        }
        screen.text(
            slider_x + SLIDER_STEPS + 2,
            row,
            &format!("{:>4}", format!("{}%", filled * 10)),
            DIM,
            Some(bg),
        );
        hits.push((x + 1, row, slider_x - 1, row + 1, Target::VolumeRow(ch)));
        hits.push((slider_x - 1, row, slider_x, row + 1, Target::Volume(ch, 0)));
    }

    let help = y + 1 + rows;
    screen.text(
        x,
        help,
        &format!("│{}│", " ".repeat(inner)),
        MENU_EDGE,
        Some(MENU_BG),
    );
    screen.text(
        x + 2,
        help,
        "←→ adjust · m mute · v close",
        FAINT,
        Some(MENU_BG),
    );
    screen.text(
        x,
        help + 1,
        &format!("╰{}╯", "─".repeat(inner)),
        MENU_EDGE,
        Some(MENU_BG),
    );
}

/// The pixel scene: room, characters, lighting, and glowing things.
pub fn paint(cafe: &Cafe, selected: Option<&str>) -> Canvas {
    let a = art();
    let time = cafe.time;
    let mut c = background().clone();
    let flash = cafe.ambient.storm.flash();
    paint_door(&mut c, cafe.door_open(), time, flash);
    let brew = cafe.barista.brew_progress();
    paint_machine(&mut c, brew, time);
    let barista_cup = cafe
        .barista
        .present()
        .then(|| draw_barista(&mut c, cafe))
        .flatten();
    paint_counter(&mut c);
    if let Some((x, y)) = barista_cup {
        c.sprite(a.chars.get("cup"), x, y, &a.chars.palette, &[]);
    }

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
    apply_flash(&mut c, flash);

    paint_window(&mut c, time, flash, cafe.ambient.storm.bolt());
    paint_hanging_lamps(&mut c);
    if !cafe.door_open() {
        paint_door_window(&mut c, time, flash);
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
    paint_machine_lights(&mut c, time, brew);

    // Steam: from the wand while milk steams, and from the drink in hand.
    let steaming = match (cafe.barista.drink(), brew) {
        (Some(drink), Some(p)) => {
            // Steaming starts just after the shot does and runs for the drink's steam time.
            let start = room::SHOT_STARTS + 0.08;
            p > start && p < start + drink.steam_secs() / 4.2
        }
        _ => false,
    };
    if steaming {
        let (wx, wy) = room::STEAM_WAND;
        for i in 0..4 {
            let t = (time * 1.8 + i as f64 / 4.0).fract() as f32;
            let x = wx + ((time * 2.5 + i as f64 * 1.7).sin() * 1.5) as i32;
            let y = wy - (t * 9.0) as i32;
            c.blend(x, y, STEAM, 0.6 * (1.0 - t));
            c.blend(x + 1, y, STEAM, 0.35 * (1.0 - t));
        }
    }
    if let Some((x, y)) = barista_cup {
        steam(&mut c, x + 1, y - 1, time, 0);
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

/// Draws the Barista; returns where the drink it's carrying goes, which is
/// drawn later so the counter doesn't hide it.
fn draw_barista(cv: &mut Canvas, cafe: &Cafe) -> Option<(i32, i32)> {
    let a = art();
    let b = &cafe.barista;
    let x = b.x.round() as i32;
    let reaching = b.serving() || b.brewing();
    let name = if reaching {
        "barista_serve"
    } else if (cafe.time * 0.35).fract() < 0.04 {
        "barista_blink"
    } else {
        "barista"
    };
    let sprite = a.chars.get(name);
    let sprite = if b.facing_left() {
        sprite.flipped()
    } else {
        sprite.clone()
    };
    cv.sprite(&sprite, x, BARISTA_Y, &a.chars.palette, &[]);
    b.holding_cup().then(|| {
        let reach = if b.serving() { 13 } else { 10 };
        let hand = if b.facing_left() {
            x + 15 - reach - 3
        } else {
            x + reach
        };
        (hand, BARISTA_Y + 10)
    })
}

/// A thin curl of steam rising from `(x, y)`.
fn steam(cv: &mut Canvas, x: i32, y: i32, time: f64, seed: u64) {
    for k in 0..2 {
        let t = ((time * 0.55) + seed as f64 * 0.37 + k as f64 * 0.5).fract() as f32;
        let dx = ((t * 7.0 + seed as f32).sin() * 1.2).round() as i32;
        cv.blend(x + dx, y - (t * 6.0) as i32, STEAM, 0.42 * (1.0 - t));
    }
}

/// A seated Clawd on a coffee break lifts its cup for a sip every few seconds.
fn sipping(c: &Clawd, time: f64) -> bool {
    ((time + c.order as f64 * 1.7) / 7.0).fract() < 0.16
}

fn cheering(time: f64) -> bool {
    (time * 0.5).fract() < 0.3
}

/// Where a Clawd's drink is, if it has one: top-left of the cup sprite.
fn cup_pose(c: &Clawd, time: f64) -> Option<(i32, i32)> {
    let left = c.pos.0.round() as i32 - 6;
    let top = clawd_top(c);
    let activity = c.activity();
    if c.is_seated() {
        let Some(Place::Seat(i)) = c.place else {
            return None;
        };
        let table = seat_table(i).y;
        return match activity {
            Activity::Spilled => None,
            Activity::CoffeeBreak if sipping(c, time) => Some((left + 5, top + 3)),
            Activity::Done if cheering(time) => Some((left + 10, top - 3)),
            Activity::Working => Some((left + 10, table - 3)),
            _ => Some((left + 5, table - 3)),
        };
    }
    let walking = c.is_walking();
    let bob = if walking && (c.stride * 7.0) as i32 % 2 == 1 {
        -1
    } else {
        0
    };
    let holding =
        c.has_cup || (!walking && matches!(activity, Activity::CoffeeBreak | Activity::Done));
    holding.then(|| {
        (
            if c.facing_left { left - 2 } else { left + 11 },
            top + 3 + bob,
        )
    })
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
        let Some(Place::Seat(i)) = c.place else {
            return;
        };
        let table = seat_table(i);
        let body = match activity {
            Activity::CoffeeBreak if sipping(c, time) => "clawd_sit_sleepy",
            Activity::CoffeeBreak => {
                if (time * 0.4 + c.order as f64).fract() < 0.08 {
                    "clawd_sit"
                } else {
                    "clawd_sit_sleepy"
                }
            }
            Activity::Working if phase(5.0) => "clawd_type_a",
            Activity::Working => "clawd_type_b",
            Activity::NeedsYou(_) if phase(2.5) => "clawd_wave",
            Activity::Done if cheering(time) => "clawd_cheer",
            Activity::Spilled => "clawd_sad",
            _ => "clawd_sit",
        };
        cv.sprite(a.chars.get(body), left, top, p, &hat);
        cv.sprite(a.chars.get(c_hat(c)), left, top - 3, p, &hat);
        match activity {
            Activity::Working => cv.sprite(a.chars.get("laptop"), left + 1, table.y - 5, p, &[]),
            Activity::Spilled => {
                cv.sprite(a.chars.get("cup_tipped"), left + 3, table.y - 2, p, &[])
            }
            _ => {}
        }
    } else {
        let walking = c.is_walking();
        let stepping = walking && (c.stride * 7.0) as i32 % 2 == 1;
        let body = if stepping {
            "clawd_walk"
        } else {
            "clawd_stand"
        };
        let bob = if stepping { -1 } else { 0 };
        cv.sprite(a.chars.get(body), left, top + bob, p, &hat);
        cv.sprite(a.chars.get(c_hat(c)), left, top - 3 + bob, p, &hat);
    }
    if let Some((x, y)) = cup_pose(c, time) {
        cv.sprite(a.chars.get("cup"), x, y, p, &[]);
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
        _ => {}
    }
    let lifted = activity == Activity::CoffeeBreak && c.is_seated() && sipping(c, time);
    if let Some((x, y)) = cup_pose(c, time).filter(|_| !lifted) {
        steam(cv, x + 1, y - 1, time, c.order);
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
pub(crate) fn sprite_lit(
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
    let sound = match ui.sound {
        Some(true) => " · m mute · v volume",
        Some(false) => " · m unmute · v volume",
        None => "",
    };
    let keys = format!("tab select · ⏎ jump · n new agent{sound} · r resync · q quit");
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
fn compose_list(cafe: &Cafe, ui: &Ui, screen: &mut Screen, hits: &mut Vec<Hit>, wall_now: f64) {
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
    fn layout_fills_the_terminal_and_snaps_to_whole_scales() {
        assert_eq!(layout(120, 40, 3), None);
        assert_eq!(layout(140, 48, 1).unwrap().scale, 1.0);

        let l = layout(160, 52, 5).unwrap();
        assert_eq!((l.scale, l.ox, l.legend_rows), (1.0, 10, 2));

        assert_eq!(layout(300, 100, 1).unwrap().scale, 2.0);

        let l = layout(238, 87, 5).unwrap();
        assert!((l.scale - 1.7).abs() < 1e-4);
        assert_eq!((l.room_w, l.room_h), (238, 74));
    }

    #[test]
    fn the_room_keeps_its_size_as_the_legend_grows() {
        let few = layout(238, 87, 2).unwrap();
        let many = layout(238, 87, 14).unwrap();
        assert_eq!((few.scale, few.oy), (many.scale, many.oy));
        assert!(many.legend_rows > few.legend_rows);
    }

    #[test]
    fn the_volume_menu_draws_its_levels_and_maps_clicks_to_them() {
        let mut ui = Ui {
            sound: Some(true),
            volume_menu: Some(Channel::Music),
            ..Ui::default()
        };
        ui.levels.set(Channel::Rain, 0.4);
        let frame = compose(&Cafe::new(), &ui, 140, 48, 0.0);
        // At 1x the 140-column room starts at column 0, so the menu sits at 105.
        let (x, slider) = (105, 118);
        assert_eq!(
            frame.hit(slider + 4, 3),
            Some(&Target::Volume(Channel::Music, 5))
        );
        assert_eq!(
            frame.hit(slider - 1, 2),
            Some(&Target::Volume(Channel::Master, 0))
        );
        assert_eq!(frame.hit(x + 3, 4), Some(&Target::VolumeRow(Channel::Rain)));
        assert_eq!(frame.hit(x + 1, 7), Some(&Target::VolumePanel));
        let text = frame.screen.to_text();
        let rain = text.lines().nth(4).unwrap();
        assert!(
            rain.contains("rain") && rain.contains("━━━━──────") && rain.contains("40%"),
            "{rain}"
        );
        assert!(text.lines().nth(3).unwrap().contains("▸ music"));
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

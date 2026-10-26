//! Replays the demo on a fixed clock and snapshots what it looks like, so
//! changes to choreography or art show up as reviewable diffs.

use std::fmt::Write as _;

use rex_cafe::replay::{WALL, replay};
use rex_cafe::view::{self, Ui};

/// Each Agent's Activity every two seconds through one pass of the script.
#[test]
fn demo_choreography() {
    let times: Vec<f64> = (0..=32).map(|i| i as f64 * 2.0).collect();
    let mut out = String::new();
    replay(&times, |t, model| {
        let row: Vec<String> = model
            .cafe
            .legend()
            .iter()
            .map(|c| format!("{}={:?}", &c.agent.block[5..], c.activity()))
            .collect();
        let _ = writeln!(out, "{t:>4}s  {}", row.join("  "));
    });
    insta::assert_snapshot!(out);
}

/// The Menu, Legend, and footer while one Agent needs you.
#[test]
fn demo_text_layer() {
    replay(&[21.0], |t, model| {
        let ui = Ui { demo: true, ..Ui::default() };
        let frame = view::compose(&model.cafe, &ui, 150, 52, WALL + t);
        let text: String = frame
            .screen
            .to_text()
            .lines()
            .map(|l| l.replace('▀', " ").trim_end().to_string())
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        insta::assert_snapshot!(text);
    });
}

/// The room as brightness, one character per pixel.
fn luma_art(canvas: &rex_cafe::engine::Canvas) -> String {
    const RAMP: &[u8] = b" .,:;-=+*#%@";
    let mut out = String::new();
    for y in 0..canvas.h {
        for x in 0..canvas.w {
            let l = canvas.get(x, y).luma() / 256.0;
            out.push(RAMP[((l * RAMP.len() as f32) as usize).min(RAMP.len() - 1)] as char);
        }
        out.push('\n');
    }
    out
}

#[test]
fn demo_frames() {
    replay(&[0.0, 36.0], |t, model| {
        let art = luma_art(&view::paint(&model.cafe, None));
        insta::assert_snapshot!(format!("frame_{t:02}"), art);
    });
}

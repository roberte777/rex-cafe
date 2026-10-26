use std::fmt::Write as _;

use super::canvas::Canvas;
use super::color::Rgb;

/// The upper half block: its foreground is the top pixel, its background the bottom one.
pub const UPPER_HALF: char = '▀';

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub fg: Rgb,
    pub bg: Rgb,
}

impl Cell {
    pub fn blank(bg: Rgb) -> Cell {
        Cell {
            ch: ' ',
            fg: bg,
            bg,
        }
    }

    /// The colour a viewer perceives for the cell's background area.
    fn backdrop(&self) -> Rgb {
        if self.ch == UPPER_HALF {
            self.fg.mix(self.bg, 0.5)
        } else {
            self.bg
        }
    }
}

/// A grid of terminal cells, composed fresh every frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Screen {
    pub cols: u16,
    pub rows: u16,
    cells: Vec<Cell>,
}

impl Screen {
    pub fn new(cols: u16, rows: u16, bg: Rgb) -> Screen {
        Screen {
            cols,
            rows,
            cells: vec![Cell::blank(bg); cols as usize * rows as usize],
        }
    }

    pub fn get(&self, col: u16, row: u16) -> Cell {
        self.cells[row as usize * self.cols as usize + col as usize]
    }

    fn cell_mut(&mut self, col: i32, row: i32) -> Option<&mut Cell> {
        if col < 0 || row < 0 || col >= self.cols as i32 || row >= self.rows as i32 {
            return None;
        }
        Some(&mut self.cells[row as usize * self.cols as usize + col as usize])
    }

    /// Draws `canvas` with its top-left at `(col, row)`. Each canvas pixel
    /// becomes `scale` columns wide and `scale` half-rows tall.
    pub fn blit(&mut self, canvas: &Canvas, col: i32, row: i32, scale: usize) {
        let cols = canvas.w * scale;
        let rows = canvas.h * scale / 2;
        for cy in 0..rows {
            for cx in 0..cols {
                let top = canvas.get(cx / scale, cy * 2 / scale);
                let bottom = canvas.get(cx / scale, (cy * 2 + 1) / scale);
                if let Some(cell) = self.cell_mut(col + cx as i32, row + cy as i32) {
                    *cell = if top == bottom {
                        Cell::blank(top)
                    } else {
                        Cell {
                            ch: UPPER_HALF,
                            fg: top,
                            bg: bottom,
                        }
                    };
                }
            }
        }
    }

    /// Writes text one cell per character. With no `bg` the text sits on
    /// whatever is already drawn underneath.
    pub fn text(&mut self, col: i32, row: i32, s: &str, fg: Rgb, bg: Option<Rgb>) {
        for (i, ch) in s.chars().enumerate() {
            if let Some(cell) = self.cell_mut(col + i as i32, row) {
                let bg = bg.unwrap_or_else(|| cell.backdrop());
                *cell = Cell { ch, fg, bg };
            }
        }
    }

    pub fn fill(&mut self, col: i32, row: i32, w: i32, h: i32, bg: Rgb) {
        for r in row..row + h {
            for c in col..col + w {
                if let Some(cell) = self.cell_mut(c, r) {
                    *cell = Cell::blank(bg);
                }
            }
        }
    }

    /// Just the characters, one line per row, for tests.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for row in 0..self.rows {
            let line: String = (0..self.cols).map(|c| self.get(c, row).ch).collect();
            out.push_str(line.trim_end());
            out.push('\n');
        }
        out
    }
}

/// Turns screens into escape sequences, sending only cells that changed
/// since the last frame.
#[derive(Default)]
pub struct Renderer {
    prev: Option<Screen>,
}

impl Renderer {
    /// Forgets the last frame so the next one is drawn in full.
    pub fn invalidate(&mut self) {
        self.prev = None;
    }

    pub fn render(&mut self, screen: &Screen, out: &mut String) {
        let full = self
            .prev
            .as_ref()
            .is_none_or(|p| p.cols != screen.cols || p.rows != screen.rows);
        out.push_str("\x1b[?2026h");
        if full {
            out.push_str("\x1b[0m\x1b[2J");
        }
        let mut fg = None;
        let mut bg = None;
        for row in 0..screen.rows {
            let mut cursor_here = false;
            for col in 0..screen.cols {
                let cell = screen.get(col, row);
                if !full && self.prev.as_ref().is_some_and(|p| p.get(col, row) == cell) {
                    cursor_here = false;
                    continue;
                }
                if !cursor_here {
                    let _ = write!(out, "\x1b[{};{}H", row + 1, col + 1);
                    cursor_here = true;
                }
                if cell.ch != ' ' && fg != Some(cell.fg) {
                    let Rgb(r, g, b) = cell.fg;
                    let _ = write!(out, "\x1b[38;2;{r};{g};{b}m");
                    fg = Some(cell.fg);
                }
                if bg != Some(cell.bg) {
                    let Rgb(r, g, b) = cell.bg;
                    let _ = write!(out, "\x1b[48;2;{r};{g};{b}m");
                    bg = Some(cell.bg);
                }
                out.push(cell.ch);
            }
        }
        out.push_str("\x1b[?2026l");
        self.prev = Some(screen.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blit_pairs_pixels_into_half_blocks() {
        let mut c = Canvas::new(2, 2, Rgb(1, 1, 1));
        c.set(1, 1, Rgb(9, 9, 9));
        let mut s = Screen::new(2, 1, Rgb::BLACK);
        s.blit(&c, 0, 0, 1);
        assert_eq!(s.get(0, 0), Cell::blank(Rgb(1, 1, 1)));
        assert_eq!(
            s.get(1, 0),
            Cell {
                ch: UPPER_HALF,
                fg: Rgb(1, 1, 1),
                bg: Rgb(9, 9, 9)
            }
        );
    }

    #[test]
    fn blit_scales() {
        let mut c = Canvas::new(1, 1, Rgb(5, 5, 5));
        c.set(0, 0, Rgb(5, 5, 5));
        let mut s = Screen::new(3, 2, Rgb::BLACK);
        s.blit(&c, 0, 0, 2);
        assert_eq!(s.get(1, 0), Cell::blank(Rgb(5, 5, 5)));
        assert_eq!(s.get(2, 0), Cell::blank(Rgb::BLACK));
    }

    #[test]
    fn renderer_sends_only_changes() {
        let mut r = Renderer::default();
        let mut s = Screen::new(4, 1, Rgb::BLACK);
        let mut out = String::new();
        r.render(&s, &mut out);
        s.text(2, 0, "x", Rgb(255, 255, 255), None);
        out.clear();
        r.render(&s, &mut out);
        assert!(out.contains("\x1b[1;3H"));
        assert!(out.ends_with("x\x1b[?2026l"));
        assert!(!out.contains("\x1b[2J"));
    }
}

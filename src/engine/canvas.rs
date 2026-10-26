use std::collections::HashMap;

use super::color::Rgb;
use super::sprite::Sprite;

/// A pixel buffer. Two vertically adjacent pixels become one terminal cell.
#[derive(Clone, Debug, PartialEq)]
pub struct Canvas {
    pub w: usize,
    pub h: usize,
    px: Vec<Rgb>,
}

impl Canvas {
    pub fn new(w: usize, h: usize, fill: Rgb) -> Canvas {
        Canvas {
            w,
            h,
            px: vec![fill; w * h],
        }
    }

    pub fn get(&self, x: usize, y: usize) -> Rgb {
        self.px[y * self.w + x]
    }

    pub fn set(&mut self, x: i32, y: i32, c: Rgb) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            self.px[y as usize * self.w + x as usize] = c;
        }
    }

    /// Blends `c` over the pixel with opacity `a`.
    pub fn blend(&mut self, x: i32, y: i32, c: Rgb, a: f32) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            let i = y as usize * self.w + x as usize;
            self.px[i] = self.px[i].mix(c, a);
        }
    }

    pub fn fill(&mut self, x: i32, y: i32, w: i32, h: i32, c: Rgb) {
        for yy in y..y + h {
            for xx in x..x + w {
                self.set(xx, yy, c);
            }
        }
    }

    pub fn hline(&mut self, x: i32, y: i32, w: i32, c: Rgb) {
        self.fill(x, y, w, 1, c);
    }

    pub fn vline(&mut self, x: i32, y: i32, h: i32, c: Rgb) {
        self.fill(x, y, 1, h, c);
    }

    /// Draws a sprite with its top-left corner at `(x, y)`. Keys are resolved
    /// through `overrides` first, then `palette`; unknown keys are skipped.
    pub fn sprite(
        &mut self,
        s: &Sprite,
        x: i32,
        y: i32,
        palette: &HashMap<char, Rgb>,
        overrides: &[(char, Rgb)],
    ) {
        for sy in 0..s.h {
            for sx in 0..s.w {
                let Some(k) = s.key(sx, sy) else { continue };
                let c = overrides
                    .iter()
                    .find(|(ok, _)| *ok == k)
                    .map(|(_, c)| *c)
                    .or_else(|| palette.get(&k).copied());
                if let Some(c) = c {
                    self.set(x + sx as i32, y + sy as i32, c);
                }
            }
        }
    }

    /// Applies a per-pixel transform, such as lighting.
    pub fn map(&mut self, mut f: impl FnMut(usize, usize, Rgb) -> Rgb) {
        for y in 0..self.h {
            for x in 0..self.w {
                let i = y * self.w + x;
                self.px[i] = f(x, y, self.px[i]);
            }
        }
    }

    /// Copies `src` onto this canvas at `(x, y)`.
    pub fn paste(&mut self, src: &Canvas, x: i32, y: i32) {
        for sy in 0..src.h {
            for sx in 0..src.w {
                self.set(x + sx as i32, y + sy as i32, src.get(sx, sy));
            }
        }
    }

    pub fn pixels(&self) -> &[Rgb] {
        &self.px
    }

    /// Writes the canvas as a binary PPM, for inspecting art outside a terminal.
    pub fn to_ppm(&self, scale: usize) -> Vec<u8> {
        let mut out = format!("P6\n{} {}\n255\n", self.w * scale, self.h * scale).into_bytes();
        for y in 0..self.h * scale {
            for x in 0..self.w * scale {
                let c = self.get(x / scale, y / scale);
                out.extend([c.0, c.1, c.2]);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::sprite::Sheet;

    #[test]
    fn sprite_respects_transparency_and_overrides() {
        let sheet = Sheet::parse("palette\n  A #ff0000\n\nsprite s\n  A.H\n").unwrap();
        let mut c = Canvas::new(3, 1, Rgb::BLACK);
        c.sprite(
            sheet.get("s"),
            0,
            0,
            &sheet.palette,
            &[('H', Rgb(0, 0, 255))],
        );
        assert_eq!(c.get(0, 0), Rgb(255, 0, 0));
        assert_eq!(c.get(1, 0), Rgb::BLACK);
        assert_eq!(c.get(2, 0), Rgb(0, 0, 255));
    }

    #[test]
    fn clips_out_of_bounds() {
        let mut c = Canvas::new(2, 2, Rgb::BLACK);
        c.fill(-5, -5, 20, 20, Rgb(1, 2, 3));
        assert!(c.pixels().iter().all(|p| *p == Rgb(1, 2, 3)));
    }
}

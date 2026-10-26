//! Sprites are authored as palette-indexed text grids.
//!
//! A sheet is plain text:
//!
//! ```text
//! # comment
//! palette
//!   O #d97757
//!   K #1a1a1a
//!
//! sprite clawd
//!   ..OOOO..
//!   .OKOOKO.
//! ```
//!
//! `.` is transparent. A section runs until the next blank line. Every row of a
//! sprite must be the same width. Characters with no palette entry must be
//! supplied as overrides when drawing, which is how Hats get their colour.

use std::collections::HashMap;

use super::color::Rgb;

#[derive(Clone, Debug, PartialEq)]
pub struct Sprite {
    pub w: usize,
    pub h: usize,
    /// Row-major palette keys; `None` is transparent.
    pub keys: Vec<Option<char>>,
}

impl Sprite {
    pub fn key(&self, x: usize, y: usize) -> Option<char> {
        self.keys[y * self.w + x]
    }

    /// The same sprite mirrored left to right.
    pub fn flipped(&self) -> Sprite {
        let mut keys = Vec::with_capacity(self.keys.len());
        for y in 0..self.h {
            for x in (0..self.w).rev() {
                keys.push(self.key(x, y));
            }
        }
        Sprite {
            w: self.w,
            h: self.h,
            keys,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Sheet {
    pub palette: HashMap<char, Rgb>,
    sprites: HashMap<String, Sprite>,
}

impl Sheet {
    pub fn parse(src: &str) -> Result<Sheet, String> {
        let mut sheet = Sheet::default();
        let mut lines = src.lines().enumerate().peekable();
        while let Some((n, line)) = lines.next() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let body: Vec<(usize, &str)> = std::iter::from_fn(|| {
                lines
                    .next_if(|(_, l)| !l.trim().is_empty())
                    .map(|(n, l)| (n, l.trim()))
            })
            .filter(|(_, l)| !l.starts_with('#'))
            .collect();
            if line == "palette" {
                for (n, entry) in body {
                    let mut parts = entry.split_whitespace();
                    let (Some(key), Some(hex)) = (parts.next(), parts.next()) else {
                        return Err(format!("line {}: expected `<char> #rrggbb`", n + 1));
                    };
                    let mut chars = key.chars();
                    let (Some(key), None) = (chars.next(), chars.next()) else {
                        return Err(format!("line {}: palette key must be one character", n + 1));
                    };
                    let rgb = Rgb::parse(hex)
                        .ok_or_else(|| format!("line {}: bad colour {hex}", n + 1))?;
                    sheet.palette.insert(key, rgb);
                }
            } else if let Some(name) = line.strip_prefix("sprite ") {
                let name = name.trim().to_string();
                let w = body.first().map(|(_, l)| l.chars().count()).unwrap_or(0);
                if w == 0 {
                    return Err(format!("line {}: sprite {name} is empty", n + 1));
                }
                let mut keys = Vec::new();
                for (n, row) in &body {
                    if row.chars().count() != w {
                        return Err(format!(
                            "line {}: sprite {name} rows differ in width",
                            n + 1
                        ));
                    }
                    keys.extend(row.chars().map(|c| (c != '.').then_some(c)));
                }
                let h = body.len();
                if sheet
                    .sprites
                    .insert(name.clone(), Sprite { w, h, keys })
                    .is_some()
                {
                    return Err(format!("line {}: sprite {name} defined twice", n + 1));
                }
            } else {
                return Err(format!(
                    "line {}: expected `palette` or `sprite <name>`",
                    n + 1
                ));
            }
        }
        Ok(sheet)
    }

    /// Merges another sheet in; its palette entries and sprites win.
    pub fn extend(&mut self, other: Sheet) {
        self.palette.extend(other.palette);
        self.sprites.extend(other.sprites);
    }

    pub fn get(&self, name: &str) -> &Sprite {
        self.sprites
            .get(name)
            .unwrap_or_else(|| panic!("no sprite named {name}"))
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.sprites.keys().map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "
# test sheet
palette
  O #d97757
  K #000000

sprite blob
  .OO.
  OKKO
";

    #[test]
    fn parses_palette_and_sprites() {
        let sheet = Sheet::parse(SRC).unwrap();
        assert_eq!(sheet.palette[&'O'], Rgb(0xd9, 0x77, 0x57));
        let blob = sheet.get("blob");
        assert_eq!((blob.w, blob.h), (4, 2));
        assert_eq!(blob.key(0, 0), None);
        assert_eq!(blob.key(1, 1), Some('K'));
    }

    #[test]
    fn flips() {
        let sheet = Sheet::parse("sprite s\n  AB.\n").unwrap();
        let f = sheet.get("s").flipped();
        assert_eq!(f.keys, vec![None, Some('B'), Some('A')]);
    }

    #[test]
    fn rejects_ragged_rows() {
        assert!(Sheet::parse("sprite s\n  AB\n  A\n").is_err());
    }
}

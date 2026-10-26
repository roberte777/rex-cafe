/// A 24-bit colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const BLACK: Rgb = Rgb(0, 0, 0);

    pub const fn hex(v: u32) -> Rgb {
        Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
    }

    /// Parses `#rrggbb`.
    pub fn parse(s: &str) -> Option<Rgb> {
        let s = s.strip_prefix('#')?;
        if s.len() != 6 {
            return None;
        }
        u32::from_str_radix(s, 16).ok().map(Rgb::hex)
    }

    /// Multiplies each channel, so `0.5` halves the brightness.
    pub fn scale(self, f: f32) -> Rgb {
        let c = |v: u8| (v as f32 * f).round().clamp(0.0, 255.0) as u8;
        Rgb(c(self.0), c(self.1), c(self.2))
    }

    /// Multiplies channel-wise by a light colour expressed in 0..=1 per channel.
    pub fn tint(self, r: f32, g: f32, b: f32) -> Rgb {
        let c = |v: u8, f: f32| (v as f32 * f).round().clamp(0.0, 255.0) as u8;
        Rgb(c(self.0, r), c(self.1, g), c(self.2, b))
    }

    /// Linear blend: `t = 0` is `self`, `t = 1` is `other`.
    pub fn mix(self, other: Rgb, t: f32) -> Rgb {
        let t = t.clamp(0.0, 1.0);
        let c = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Rgb(c(self.0, other.0), c(self.1, other.1), c(self.2, other.2))
    }

    pub fn luma(self) -> f32 {
        0.299 * self.0 as f32 + 0.587 * self.1 as f32 + 0.114 * self.2 as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex() {
        assert_eq!(Rgb::parse("#d97757"), Some(Rgb(0xd9, 0x77, 0x57)));
        assert_eq!(Rgb::parse("d97757"), None);
        assert_eq!(Rgb::parse("#d977"), None);
    }

    #[test]
    fn mixes_and_scales() {
        assert_eq!(Rgb(0, 0, 0).mix(Rgb(200, 100, 50), 0.5), Rgb(100, 50, 25));
        assert_eq!(Rgb(200, 100, 50).scale(0.5), Rgb(100, 50, 25));
        assert_eq!(Rgb(200, 200, 200).scale(2.0), Rgb(255, 255, 255));
    }
}

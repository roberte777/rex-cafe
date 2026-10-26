use std::sync::OnceLock;

use crate::engine::Sheet;

/// The sprite sheets, compiled into the binary.
pub struct Art {
    pub chars: Sheet,
    pub props: Sheet,
}

pub fn art() -> &'static Art {
    static ART: OnceLock<Art> = OnceLock::new();
    ART.get_or_init(|| Art {
        chars: Sheet::parse(include_str!("../../assets/sprites/characters.txt"))
            .expect("characters.txt"),
        props: Sheet::parse(include_str!("../../assets/sprites/props.txt")).expect("props.txt"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cafe::HATS;

    #[test]
    fn sheets_parse_and_every_hat_exists() {
        let a = art();
        for hat in HATS {
            assert!(
                a.chars.get(hat.sprite).w <= 12,
                "{} is wider than a Clawd",
                hat.sprite
            );
        }
        assert!(a.props.names().count() > 0);
    }

    #[test]
    fn every_sprite_key_has_a_colour() {
        let a = art();
        for sheet in [&a.chars, &a.props] {
            for name in sheet.names() {
                let s = sheet.get(name);
                for k in s.keys.iter().flatten() {
                    assert!(
                        sheet.palette.contains_key(k) || matches!(k, 'H' | 'h'),
                        "{name} uses {k} with no colour"
                    );
                }
            }
        }
    }
}

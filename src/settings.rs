//! What the café remembers between runs, and what you can tune by hand:
//! how loud it is, and how often lightning strikes.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::audio::Levels;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub levels: Levels,
    pub muted: bool,
    pub lightning: Lightning,
}

/// How long to wait between lightning strikes, picked at random between the two.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Lightning {
    pub min_secs: f32,
    pub max_secs: f32,
}

impl Default for Lightning {
    fn default() -> Lightning {
        Lightning {
            min_secs: 25.0,
            max_secs: 300.0,
        }
    }
}

/// Strikes closer together than this would be a strobe, not a storm.
const SHORTEST_GAP_SECS: f32 = 2.0;

impl Lightning {
    /// The wait range to use: at least a couple of seconds, with the bounds in order.
    pub fn gap(&self) -> (f32, f32) {
        let usable = |v: f32| {
            if v.is_finite() {
                v.max(SHORTEST_GAP_SECS)
            } else {
                SHORTEST_GAP_SECS
            }
        };
        let (a, b) = (usable(self.min_secs), usable(self.max_secs));
        (a.min(b), a.max(b))
    }
}

/// `$XDG_CONFIG_HOME/rex-cafe/settings.json`, or `~/.config/rex-cafe/settings.json`.
pub fn path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("rex-cafe").join("settings.json"))
}

impl Settings {
    /// Reads settings, falling back to defaults when the file is missing or unreadable.
    pub fn load(path: &Path) -> Settings {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)? + "\n")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::Channel;

    #[test]
    fn lightning_gaps_are_kept_sane() {
        let gap = |min_secs, max_secs| Lightning { min_secs, max_secs }.gap();
        assert_eq!(gap(5.0, 15.0), (5.0, 15.0));
        assert_eq!(gap(30.0, 10.0), (10.0, 30.0));
        assert_eq!(gap(0.0, 0.5), (2.0, 2.0));
        assert_eq!(gap(f32::NAN, 8.0), (2.0, 8.0));
    }

    #[test]
    fn round_trips_and_tolerates_missing_or_partial_files() {
        let dir = std::env::temp_dir().join(format!("rex-cafe-settings-{}", std::process::id()));
        let path = dir.join("nested").join("settings.json");
        assert_eq!(Settings::load(&path), Settings::default());

        let mut s = Settings::default();
        s.levels.set(Channel::Rain, 0.3);
        s.muted = true;
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path), s);

        std::fs::write(&path, r#"{"levels": {"music": 0.5}}"#).unwrap();
        let partial = Settings::load(&path);
        assert_eq!(partial.levels.music, 0.5);
        assert_eq!(partial.levels.rain, 1.0);
        assert!(!partial.muted);

        std::fs::write(&path, r#"{"lightning": {"min_secs": 5}}"#).unwrap();
        assert_eq!(Settings::load(&path).lightning.gap(), (5.0, 300.0));

        std::fs::write(&path, "not json").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
        let _ = std::fs::remove_dir_all(dir);
    }
}

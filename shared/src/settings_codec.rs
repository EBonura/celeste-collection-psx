//! The collection's persisted options as a `psx_settings::Profile`, and the reader for the private
//! `CCS1` file earlier releases wrote. No hardware: the host test in `tools/settings-test` includes
//! this file and runs it over real card images.

use psx_display::Brightness;
use psx_settings::Profile;

/// The profile shape: no action bindings and no high scores.
pub type Record = Profile<0, 0>;

/// Encoded size of the record.
pub const RECORD_LEN: usize = Record::encoded_len();

/// The magic of the private file earlier releases wrote.
const LEGACY_MAGIC: [u8; 4] = *b"CCS1";
/// magic(4) + sfx + music + pixel scale + borders preset + brightness.
const LEGACY_LEN: usize = 9;
/// The same file before brightness existed.
const LEGACY_LEN_V1: usize = 8;

/// Index of the pixel scale (1 or 2) in the profile's game bytes.
const GAME_PIXEL_SCALE: usize = 0;
/// Index of the borders preset in the profile's game bytes.
const GAME_SIDE_PRESET: usize = 1;

/// The options the collection keeps across sessions. Volumes are the sliders' 0..=8 steps.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    /// SFX volume, 0..=8.
    pub sfx: u8,
    /// Music volume, 0..=8.
    pub music: u8,
    /// Pixel scale: 1 or 2.
    pub pixel_scale: u8,
    /// Borders preset (the backend wraps it).
    pub side_preset: u8,
    /// Brightness step.
    pub brightness: Brightness,
}

/// A slider step as the profile's percentage.
const fn step_to_percent(step: u8) -> u8 {
    let step = if step > 8 { 8 } else { step };
    ((step as u16 * 100 + 4) / 8) as u8
}

/// A profile percentage as the nearest slider step.
const fn percent_to_step(percent: u8) -> u8 {
    let percent = if percent > 100 { 100 } else { percent };
    ((percent as u16 * 8 + 50) / 100) as u8
}

impl Settings {
    /// The profile that carries these options (everything else at the shared defaults).
    pub fn to_profile(&self) -> Record {
        let mut profile = Record::new(psx_pad::ActionMap::new([]));
        profile.sfx_volume = step_to_percent(self.sfx);
        profile.music_volume = step_to_percent(self.music);
        profile.brightness = self.brightness;
        profile.game_bytes[GAME_PIXEL_SCALE] = self.pixel_scale;
        profile.game_bytes[GAME_SIDE_PRESET] = self.side_preset;
        profile
    }

    /// The options a profile carries.
    pub fn from_profile(profile: &Record) -> Self {
        Self {
            sfx: percent_to_step(profile.sfx_volume),
            music: percent_to_step(profile.music_volume),
            pixel_scale: profile.game_bytes[GAME_PIXEL_SCALE],
            side_preset: profile.game_bytes[GAME_SIDE_PRESET],
            brightness: profile.brightness,
        }
    }

    /// The card payload for these options.
    pub fn encode(&self) -> Option<[u8; RECORD_LEN]> {
        let mut out = [0u8; RECORD_LEN];
        self.to_profile().encode(&mut out).ok()?;
        Some(out)
    }

    /// Options from a card file: the profile record, or else the private `CCS1` file of earlier
    /// releases. The second value is true when the file was the old one (it should be rewritten).
    pub fn decode(bytes: &[u8]) -> Option<(Self, bool)> {
        if let Ok(profile) = Record::decode(bytes) {
            return Some((Self::from_profile(&profile), false));
        }
        Self::decode_legacy(bytes).map(|settings| (settings, true))
    }

    /// The private file: `CCS1`, SFX, music, pixel scale, borders, then (from the release that
    /// added it) brightness. A file without the ninth byte has the default brightness.
    fn decode_legacy(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < LEGACY_LEN_V1 || bytes[0..4] != LEGACY_MAGIC {
            return None;
        }
        Some(Self {
            sfx: bytes[4],
            music: bytes[5],
            pixel_scale: bytes[6],
            side_preset: bytes[7],
            brightness: if bytes.len() >= LEGACY_LEN {
                Brightness::from_byte(bytes[8])
            } else {
                Brightness::DEFAULT
            },
        })
    }
}

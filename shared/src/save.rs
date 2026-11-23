//! Collection settings on the PS1 memory card (via `psx-mc`).
//!
//! One tiny save file persists the player options that outlive a session:
//! SFX volume, music volume, pixel scale (1x/2x), the borders preset and the
//! brightness step.
//! (Berry/death counters are per-run in both carts and reset by the games
//! themselves, so they are deliberately not saved.)
//!
//! Policy, sized for a shipped product:
//! - [`load`] runs once at boot. A missing, unformatted or foreign card is
//!   never an error: every failure path silently keeps the defaults.
//! - [`save`] runs when a settings menu closes with a change pending, never
//!   per slider tick (card writes are slow). It compares against the last
//!   payload synced with the card and skips the write when nothing differs.
//! - The card is formatted only when it affirmatively reports unformatted
//!   AND the player is saving; read errors never trigger a format.

use crate::settings_codec::{Settings, RECORD_LEN};
use crate::{backend, sfx};
use psx_mc::{Card, ControllerPort, HardwareCard, Slot};

/// BIOS file name (region + product code + label). PS1 names cap at 20
/// chars, so the label is CELSTCC1 (CELESTE CLASSIC COLLECTION, slot 1).
const FILE_NAME: &str = "BESLES-00000CELSTCC1";
/// Human-readable title shown by the console's memory-card manager.
const TITLE: &str = "CELESTE COLLECTION";

/// The payload as last synced with the card (loaded or written), so [`save`]
/// can skip the slow write when the live settings already match. `None` until
/// a load or write succeeds.
static mut SYNCED: Option<[u8; RECORD_LEN]> = None;

/// The live settings.
pub fn current() -> Settings {
    Settings {
        sfx: sfx::sfx_volume().min(8) as u8,
        music: sfx::music_volume().min(8) as u8,
        pixel_scale: backend::pixel_scale() as u8,
        side_preset: backend::side_preset(),
        brightness: backend::brightness(),
    }
}

/// The card, on port 1's slot one.
fn card() -> Card<HardwareCard> {
    // SAFETY: a token is a logic guard, not a memory-safety one, and nothing else drives SIO0
    // while the collection loads or saves its settings.
    Card::new(HardwareCard::on_port(
        unsafe { ControllerPort::steal() },
        Slot::One,
    ))
}

/// Load the settings file and apply it. Call once at boot, before the menu.
/// Any failure (no card, unformatted, no file, bad payload) keeps defaults.
pub fn load() {
    let mut buf = [0u8; psx_settings::MAX_RECORD_LEN];
    let Ok(len) = card().read(FILE_NAME, &mut buf) else {
        return;
    };
    let Some((settings, legacy)) = Settings::decode(&buf[..len]) else {
        return;
    };
    apply(settings);
    // A file from an earlier release stays unsynced, so the next save rewrites it as a record.
    set_synced(!legacy);
}

/// Make `settings` the live settings. Every setter clamps/wraps on its own, so a stale payload
/// from a future layout can nudge a setting but never wedge the game.
pub fn apply(settings: Settings) {
    sfx::set_sfx_volume(settings.sfx as u16); // clamps to 0..=8
    sfx::set_music_volume(settings.music as u16); // clamps to 0..=8
    backend::set_pixel_scale(settings.pixel_scale as i16); // clamps to 1|2
    backend::set_side_preset(settings.side_preset); // wraps % preset count
    backend::set_brightness(settings.brightness);
}

/// Record whether the card holds the live settings: true marks them synced, false forces the
/// next [`save`] to write.
pub fn set_synced(on: bool) {
    unsafe { SYNCED = if on { current().encode() } else { None } };
}

/// Write the settings file if the live settings differ from the card's.
/// Call when a settings menu closes after a change; failures are silent
/// (the settings simply stay RAM-only for this session).
pub fn save() {
    let Some(snap) = current().encode() else {
        return;
    };
    if unsafe { SYNCED } == Some(snap) {
        return;
    }
    let mut card = card();
    match card.is_formatted() {
        Ok(true) => {}
        // Fresh card + an explicit player save: the one case we format.
        Ok(false) => {
            if card.format().is_err() {
                return;
            }
        }
        // No card / unreadable: don't format, don't insist.
        Err(_) => return,
    }
    if card.write(FILE_NAME, TITLE, &snap).is_ok() {
        unsafe { SYNCED = Some(snap) };
    }
}

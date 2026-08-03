//! One-shot UI sound effects for the menus, separate from the PICO-8 music/SFX
//! engine. The sounds are CC0 (Kenney "Interface Sounds", public domain),
//! resampled to 22050 Hz mono and stored as one-shot PS1 SPU ADPCM (see
//! `tools/gen_menu_sfx.py`). They play on dedicated voices 16..18 -- outside the
//! 0..15 the [`crate::sfx`] engine uses -- so the launcher and the in-game pause
//! overlay can fire UI sounds without disturbing the game's audio.

use psx_sfx::{OneShot, Sample as SfxSample};
use psx_spu as spu;
use spu::{Adsr, SpuAddr, Voice, Volume};

/// Decoded samples in one ADPCM block, for turning MENU_SFX's sample counts
/// into the block count psx-sfx times a cutoff from.
const SAMPLES_PER_BLOCK: u32 = 28;

include!("menu_sfx_data.rs"); // MENU_SFX_ADPCM, MENU_SFX, SFX_NAV / CONFIRM / TRANSITION

const SPU_BASE: u32 = 0x4000; // clear of the PICO-8 waveforms (0x1010..~0x1910)
const VOL_BASE: i16 = 0x2800; // pre-scale level (scaled by the SFX-volume setting)
const VOICES: [u8; 3] = [16, 17, 18]; // round-robin so quick sounds don't cut each other

static mut NEXT: usize = 0;

/// Upload the menu SFX bank to SPU RAM. Call once after `crate::sfx::init`
/// (which does `spu::init`); re-call if the SPU is re-initialised (e.g. the
/// launcher after returning from a game).
pub fn init() {
    spu::upload_adpcm(SpuAddr::new(SPU_BASE), &MENU_SFX_ADPCM.0);
}

/// Fire menu sound `id` (`SFX_NAV` / `SFX_CONFIRM` / `SFX_TRANSITION`). Volume is
/// scaled by the shared SFX-volume setting, so the pause slider affects it.
pub fn play(id: usize) {
    if id >= MENU_SFX.len() {
        return;
    }
    unsafe {
        let (off, samples) = MENU_SFX[id];
        let v = VOICES[NEXT % VOICES.len()];
        NEXT = NEXT.wrapping_add(1);
        let vol = (VOL_BASE as i32 * crate::sfx::sfx_volume() as i32 / 8) as i16;
        // Through psx-sfx so the repeat address is written. These are
        // one-shots, last block flag 0x01 and no loop, and on END silicon
        // jumps to that register rather than carrying on -- so without it the
        // voice landed in whatever a PICO-8 wavetable had left there. The
        // instant release below is why that was never audible here, which
        // makes it luck rather than correctness.
        //
        // 22050 Hz is what the bank is cooked at, so configure_sample derives
        // the same 0x0800 pitch PITCH_22K spelt out by hand.
        let sample = SfxSample::resident(
            SpuAddr::new(SPU_BASE + off),
            22_050,
            samples.div_ceil(SAMPLES_PER_BLOCK),
        );
        OneShot::new(sample, Volume(vol))
            // Instant attack, full sustain, fastest release the hardware
            // encodes.
            .with_adsr(Adsr { lower: 0x000F, upper: 0x0000 })
            .play(Voice::new(v));
    }
}

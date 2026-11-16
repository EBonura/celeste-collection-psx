//! The pause overlay loop and the offline audio test loops that Celeste and
//! Celeste 2 share. A game passes what differs: its audio data, the cursor blip,
//! the clear colour and its draw routine.

use crate::pause::{self, Exit, Pause};
use crate::screen::Screen;
use crate::sfx::{self, wait_vblank, AudioData};
use psx_gpu as gpu;
use psx_gpu::display::{Resolution, VideoMode};

/// Pause overlay: freeze the game, show the volume/quit menu, and keep the SPU
/// advancing so the music plays on at the chosen volume. Returns true if the
/// player picked "quit to menu". `cursor_sfx` is the slider feedback blip, `bg`
/// the clear colour behind the frozen frame and `draw` the game's draw routine.
pub fn run_pause(fb: &mut Screen, cursor_sfx: i32, bg: (u8, u8, u8), draw: fn()) -> bool {
    let mut menu = Pause::new(cursor_sfx, true); // show the debug FLY row
    loop {
        match menu.update(pause::mask(crate::input::poll_buttons())) {
            Some(Exit::Resume) => return false,
            Some(Exit::QuitToMenu) => return true,
            None => {}
        }

        fb.clear(bg.0, bg.1, bg.2);
        draw(); // frozen game behind the overlay
        menu.draw();
        gpu::draw_sync();
        fb.swap();
        sfx::update(); // keep music/SFX alive (and audible at the new volume)
    }
}

/// Offline single-SFX test: play `sfx(id)` once then idle, looping. For checking
/// an isolated SFX's notes/timbre against PICO-8.
pub fn run_sfx_test(audio: AudioData, id: i32) {
    gpu::init(VideoMode::Ntsc, Resolution::R320X240);
    psx_rt::interrupts::install_vblank_counter();
    sfx::init(audio);
    loop {
        sfx::play(id);
        for _ in 0..240 {
            sfx::update();
            wait_vblank();
        }
    }
}

/// Number of silent frames played between SFX in the soundtest. Doubles as
/// the split marker for the host capture (a clear gap between clips).
pub const SOUNDTEST_GAP_FRAMES: u32 = 18; // ~0.3s

/// Offline SFX soundtest: play SFX `0..frames.len()` one at a time, each for a
/// FIXED number of frames `frames[n]` (so the host can split the captured SPU
/// output at exact offsets), separated by [`SOUNDTEST_GAP_FRAMES`] of silence.
/// Diffed against the PICO-8 reference recordings. Not part of the game;
/// driven by the `soundtest` binary + `tools/psx-audio-capture`.
pub fn run_sfx_soundtest(audio: AudioData, frames: &[u16]) {
    gpu::init(VideoMode::Ntsc, Resolution::R320X240);
    sfx::init(audio);
    psx_rt::interrupts::install_vblank_counter(); // real 60Hz (gpu::vsync is ~65fps)

    let mut n: usize = 0;
    loop {
        sfx::play(-1);
        for _ in 0..SOUNDTEST_GAP_FRAMES {
            sfx::update();
            wait_vblank();
        }

        if n >= frames.len() {
            return;
        }

        sfx::play(n as i32);
        for _ in 0..frames[n] {
            sfx::update();
            wait_vblank();
        }
        n += 1;
    }
}

/// Offline music test: play `music(pattern)` and run the sequencer forever, so
/// the host can capture the exact same song the cart plays and compare it,
/// note-aligned, with a PICO-8 recording of `music(pattern)`. Not part of the
/// game; driven by the `musictest` binary + `tools/psx-audio-capture`.
pub fn run_music_test(audio: AudioData, pattern: i32) {
    gpu::init(VideoMode::Ntsc, Resolution::R320X240);
    sfx::init(audio);
    // Advance one sequencer step per REAL vblank (60Hz) like the game -- gpu::vsync()
    // busy-waits 242 hblanks (~65fps), which played the music ~8% fast vs PICO-8.
    psx_rt::interrupts::install_vblank_counter();
    sfx::music(pattern, 0, 0);
    loop {
        sfx::update();
        wait_vblank();
    }
}

/// Offline per-instrument isolation: play `pattern` five times in fixed windows --
/// full mix, then each music channel 0..3 SOLO'd -- separated by silence, so the
/// host can split the capture and compare each instrument to a channel-soloed
/// PICO-8 recording. Not part of the game.
pub fn run_music_iso(audio: AudioData, pattern: i32) {
    gpu::init(VideoMode::Ntsc, Resolution::R320X240);
    sfx::init(audio);
    psx_rt::interrupts::install_vblank_counter();
    let masks = [0u8, 0x0e, 0x0d, 0x0b, 0x07];
    let mut i = 0;
    loop {
        sfx::set_music_mute(masks[i]);
        sfx::music(pattern, 0, 0);
        for _ in 0..420 {
            sfx::update();
            wait_vblank();
        }
        sfx::music(-1, 0, 0);
        sfx::set_music_mute(0);
        for _ in 0..72 {
            sfx::update();
            wait_vblank();
        }
        i += 1;
        if i >= masks.len() {
            return;
        }
    }
}

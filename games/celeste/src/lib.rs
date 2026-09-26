//! `celeste` -- PICO-8 Celeste Classic, demade natively in Rust for the
//! PlayStation 1 on the PSoXide SDK.
//!
//! The game logic (game.rs) is a faithful port of ccleste. The PICO-8 runtime
//! (draw/input/audio backend, fixed-point, RNG) lives in the shared `pico8`
//! crate; this crate supplies Celeste's own assets and wires them in as the
//! active [`Cart`] / [`AudioData`].
//!
//! Exposed as a library so the collection launcher can link it in and call
//! [`run`]; the standalone `main` just calls it. Holding Select+Start returns
//! from [`run`] (quit to the launcher).

#![no_std]
#![allow(static_mut_refs)]

pub mod assets;
mod game;

use assets::audio_data::{MUSIC_DATA, SFX_DATA};
use assets::gfx::GFX_DATA;
use assets::tilemap::{MAP_W, TILEMAP_DATA, TILE_FLAGS};
use pico8::backend::{self, Cart};
use pico8::pause::{self, Exit, Pause};
use pico8::sfx::{self, AudioData};
use psx_gpu::{self as gpu, framebuf::FrameBuffer, Resolution, VideoMode};
use psx_pad::button;
// The SDK's `gpu::vsync()` busy-waits a fixed 242 hblanks (~15.4ms) from when
// it's called instead of syncing to the display, which left only ~1.3ms of
// per-frame compute before dropping below 60fps. The VBlank IRQ counter
// (already installed for audio) gives the full ~16.6ms frame.
use pico8::sfx::wait_vblank; // renders audio ahead while it waits

/// Celeste's spritesheet + tilemap as the active PICO-8 cart.
const CART: Cart = Cart {
    gfx: &GFX_DATA,
    tilemap: &TILEMAP_DATA,
    tile_flags: &TILE_FLAGS,
    map_w: MAP_W,
};

/// Celeste's PICO-8 sound data (raw cart sound RAM). Public so the collection launcher
/// can reuse it as the menu's sound bank (cursor-move / select blips).
pub const AUDIO: AudioData = AudioData {
    sfx: &SFX_DATA,
    music: &MUSIC_DATA,
};

/// Poll the pad and map it to PICO-8's 6 buttons: arrows, Cross=jump (O),
/// Circle=dash (X).
fn pad_mask() -> u8 {
    let b = pico8::input::poll_buttons();
    let mut mask = 0u8;
    if b.is_held(button::LEFT) {
        mask |= 1 << 0;
    }
    if b.is_held(button::RIGHT) {
        mask |= 1 << 1;
    }
    if b.is_held(button::UP) {
        mask |= 1 << 2;
    }
    if b.is_held(button::DOWN) {
        mask |= 1 << 3;
    }
    if b.is_held(button::CROSS) {
        mask |= 1 << 4;
    }
    if b.is_held(button::CIRCLE) {
        mask |= 1 << 5;
    }
    if b.is_held(button::TRIANGLE) {
        mask |= 1 << 6; // debug fly (only acts when fly mode is on)
    }
    mask
}

/// Boot Celeste and run its 60fps frame loop until Select+Start is held.
pub fn run() {
    gpu::init(VideoMode::Ntsc, Resolution::R320X240);
    let mut fb = FrameBuffer::new(320, 240);
    gpu::set_draw_area(0, 0, 319, 239);
    gpu::set_draw_offset(0, 0);
    backend::upload_assets(CART);
    sfx::init(AUDIO);
    pico8::menusfx::init(); // dedicated UI sample bank for the pause overlay

    // Seed the RNG before init (clouds/particles use it), like main.cpp.
    pico8::rng::srand(42);
    game::init();

    // The launcher launched us with Cross still held; prime the input model so
    // btnp on the title screen waits for a fresh press instead of auto-starting.
    pico8::input::prime(pad_mask());

    // Drive the audio sequencer off real VBlanks, not render frames, so the
    // music keeps PICO-8's hardware tempo even when rendering can't hold 60fps.
    psx_rt::interrupts::install_vblank_counter();
    // Prime the initial 40 audio blocks after init selects the title music,
    // while loading and before the first visible update. This preserves all
    // startup samples without charging their synthesis to the first frame.
    sfx::update();
    let mut prev_start = true; // require a fresh press before the first pause

    loop {
        // Quit to the launcher: Select+Start held together.
        let b = pico8::input::poll_buttons();
        if b.is_held(button::SELECT) && b.is_held(button::START) {
            return;
        }

        // Start alone (a fresh press, no Select) opens the pause menu.
        let start = b.is_held(button::START);
        if start && !prev_start {
            if run_pause(&mut fb) {
                return; // player chose "quit to menu"
            }
            prev_start = true; // wait for release before it can pause again
            continue;
        }
        prev_start = start;

        game::set_input(pad_mask());

        game::update();

        // Freeze frames (dash/orb): hold the last drawn frame on screen by not
        // redrawing or swapping -- exactly the PICO-8 freeze effect (the cart's
        // `_draw` returns at once while `freeze > 0`).
        if game::freeze() > 0 {
            wait_vblank();
        } else {
            fb.clear(0, 0, 0);
            backend::set_deferred(true); // list the frame, draw it by DMA
            game::draw();
            backend::submit(); // the GPU draws while the VBlank wait renders audio
            wait_vblank();
            backend::set_deferred(false); // waits for the GPU
            fb.swap();
        }

        sfx::update(); // stream the next slice of PICO-8 audio to the SPU
    }
}

/// Pause overlay: freeze the game, show the volume/quit menu, and keep the SPU
/// advancing so the music plays on at the chosen volume. Returns true if the
/// player picked "quit to menu". SFX 2 (the cursor blip) is the slider feedback.
fn run_pause(fb: &mut FrameBuffer) -> bool {
    let mut menu = Pause::new(2, true); // show the debug FLY row
    loop {
        match menu.update(pause::mask(pico8::input::poll_buttons())) {
            Some(Exit::Resume) => return false,
            Some(Exit::QuitToMenu) => return true,
            None => {}
        }

        fb.clear(0, 0, 0);
        game::draw(); // frozen game behind the overlay
        menu.draw();
        gpu::draw_sync();
        wait_vblank();
        fb.swap();
        sfx::update(); // keep music/SFX alive (and audible at the new volume)
    }
}

/// Offline single-SFX test: play `sfx(id)` once then idle, looping. For checking
/// an isolated SFX's notes/timbre against PICO-8.
pub fn run_sfx_test(id: i32) {
    gpu::init(VideoMode::Ntsc, Resolution::R320X240);
    psx_rt::interrupts::install_vblank_counter();
    sfx::init(AUDIO);
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
pub fn run_sfx_soundtest(frames: &[u16]) {
    gpu::init(VideoMode::Ntsc, Resolution::R320X240);
    sfx::init(AUDIO);
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
pub fn run_music_test(pattern: i32) {
    gpu::init(VideoMode::Ntsc, Resolution::R320X240);
    sfx::init(AUDIO);
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
pub fn run_music_iso(pattern: i32) {
    gpu::init(VideoMode::Ntsc, Resolution::R320X240);
    sfx::init(AUDIO);
    psx_rt::interrupts::install_vblank_counter();
    let masks = [0u8, 0x0E, 0x0D, 0x0B, 0x07];
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

/// Offline scene capture: boot straight into room (`x`, `y`) with no input and
/// the follow-pan off (fixed 8px crop top and bottom at 2x). Driven by the
/// `scene` binary + frametest; compared with a PICO-8 frame by tools/scene_cmp.py.
pub fn run_scene(x: i32, y: i32) {
    gpu::init(VideoMode::Ntsc, Resolution::R320X240);
    let mut fb = FrameBuffer::new(320, 240);
    gpu::set_draw_area(0, 0, 319, 239);
    gpu::set_draw_offset(0, 0);
    backend::upload_assets(CART);
    sfx::init(AUDIO);
    backend::set_screen_follow(false);
    pico8::rng::srand(42);
    game::start_at_room(x, y);
    psx_rt::interrupts::install_vblank_counter();
    loop {
        game::set_input(0);
        game::update();
        if game::freeze() > 0 {
            wait_vblank();
        } else {
            fb.clear(0, 0, 0);
            backend::set_deferred(true); // list the frame, draw it by DMA
            game::draw();
            backend::submit(); // the GPU draws while the VBlank wait renders audio
            wait_vblank();
            backend::set_deferred(false); // waits for the GPU
            fb.swap();
        }
        sfx::update();
    }
}

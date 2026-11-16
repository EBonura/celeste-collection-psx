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
use pico8::screen::Screen;
use pico8::sfx::{self, AudioData};
use psx_gpu as gpu;
use psx_gpu::display::{Resolution, VideoMode};
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
    let mut fb = Screen::new();
    // Prime the initial 40 audio blocks after init selects the title music,
    // while loading and before the first visible update. This preserves all
    // startup samples without charging their synthesis to the first frame.
    sfx::update();
    let mut prev_start = true; // require a fresh press before the first pause
                               // Cart logic runs on the shared psx-tick clock, one update per VBlank. A
                               // frame that missed VBlanks catches up (up to four updates before it draws,
                               // the rest over the next frames) instead of slowing the game down.
    let mut clock = psx_tick::FixedClock::new(
        psx_tick::TickConfig::new(psx_tick::TickRate::HZ60).with_interleave(4),
        psx_rt::interrupts::vblank_count(),
    );

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
                               // The game was frozen behind the menu; do not catch its time up.
            clock.realign(psx_rt::interrupts::vblank_count());
            continue;
        }
        prev_start = start;

        game::set_input(pad_mask());

        let now = psx_rt::interrupts::vblank_count();
        while clock.due(now) {
            game::update();
            game::observe_update();
        }
        clock.end_frame();

        // Freeze frames (dash/orb): hold the last drawn frame on screen by not
        // redrawing or swapping -- exactly the PICO-8 freeze effect (the cart's
        // `_draw` returns at once while `freeze > 0`).
        if game::freeze() > 0 {
            game::observe();
            wait_vblank();
        } else {
            fb.clear(0, 0, 0);
            backend::set_deferred(true); // list the frame, draw it by DMA
            game::draw();
            game::observe();
            backend::submit(); // the GPU draws; swap waits for it and renders audio meanwhile
            backend::set_deferred(false); // waits for the GPU
            fb.swap();
        }

        sfx::update(); // stream the next slice of PICO-8 audio to the SPU
    }
}

/// Pause overlay: see [`pico8::harness::run_pause`]. SFX 2 (the cursor blip) is the slider feedback.
fn run_pause(fb: &mut Screen) -> bool {
    pico8::harness::run_pause(fb, 2, (0, 0, 0), game::draw)
}

/// Offline single-SFX test (see [`pico8::harness::run_sfx_test`]).
pub fn run_sfx_test(id: i32) {
    pico8::harness::run_sfx_test(AUDIO, id)
}

/// Number of silent frames played between SFX in the soundtest.
pub const SOUNDTEST_GAP_FRAMES: u32 = pico8::harness::SOUNDTEST_GAP_FRAMES;

/// Offline SFX soundtest (see [`pico8::harness::run_sfx_soundtest`]).
pub fn run_sfx_soundtest(frames: &[u16]) {
    pico8::harness::run_sfx_soundtest(AUDIO, frames)
}

/// Offline music test (see [`pico8::harness::run_music_test`]).
pub fn run_music_test(pattern: i32) {
    pico8::harness::run_music_test(AUDIO, pattern)
}

/// Offline per-instrument isolation (see [`pico8::harness::run_music_iso`]).
pub fn run_music_iso(pattern: i32) {
    pico8::harness::run_music_iso(AUDIO, pattern)
}

/// Offline scene capture: boot straight into room (`x`, `y`) with no input and
/// the follow-pan off (fixed 8px crop top and bottom at 2x). Driven by the
/// `scene` binary + frametest; compared with a PICO-8 frame by tools/scene_cmp.py.
pub fn run_scene(x: i32, y: i32) {
    gpu::init(VideoMode::Ntsc, Resolution::R320X240);
    gpu::set_draw_area(0, 0, 319, 239);
    gpu::set_draw_offset(0, 0);
    backend::upload_assets(CART);
    sfx::init(AUDIO);
    backend::set_screen_follow(false);
    pico8::rng::srand(42);
    game::start_at_room(x, y);
    psx_rt::interrupts::install_vblank_counter();
    let mut fb = Screen::new();
    loop {
        game::set_input(0);
        game::update();
        if game::freeze() > 0 {
            wait_vblank();
        } else {
            fb.clear(0, 0, 0);
            backend::set_deferred(true); // list the frame, draw it by DMA
            game::draw();
            backend::submit(); // the GPU draws; swap waits for it and renders audio meanwhile
            backend::set_deferred(false); // waits for the GPU
            fb.swap();
        }
        sfx::update();
    }
}

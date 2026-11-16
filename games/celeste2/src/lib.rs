//! `celeste2` -- PICO-8 Celeste Classic 2: Lani's Trek, demade natively in
//! Rust for the PlayStation 1 on the PSoXide SDK.
//!
//! Built on the shared `pico8` runtime: this crate supplies Celeste 2's assets
//! (registered as a [`Cart`]) and game logic (`game.rs`). The logic runs at
//! 60fps with PICO-8 velocities kept at face value, accelerations + per-frame
//! moves halved, and frame timers doubled (matching the Celeste 1 port).
//!
//! Ported in full: the object engine, the player (including the grapple
//! state machine), the object set, the complete PX9-decoded level set, and
//! the PICO-8 audio engine (see game.rs for the inventory).
//!
//! Exposed as a library so the collection launcher can link it in and call
//! [`run`]; holding Select+Start returns from [`run`] (quit to the launcher).

#![no_std]

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
// The SDK's `gpu::vsync()` busy-waits a fixed 242 hblanks (~15.4ms) instead of
// syncing to the display, leaving almost no per-frame compute budget; the
// VBlank IRQ counter gives the full ~16.6ms frame.
use pico8::sfx::wait_vblank; // renders audio ahead while it waits

/// Celeste 2's spritesheet + tilemap as the active PICO-8 cart.
pub const CART: Cart = Cart {
    gfx: &GFX_DATA,
    tilemap: &TILEMAP_DATA,
    tile_flags: &TILE_FLAGS,
    map_w: MAP_W,
};

/// Celeste 2's PICO-8 sound data (raw cart sound RAM).
pub const AUDIO: AudioData = AudioData {
    sfx: &SFX_DATA,
    music: &MUSIC_DATA,
};

/// Boot Celeste 2 and run its 60fps frame loop until Select+Start is held.
pub fn run() {
    gpu::init(VideoMode::Ntsc, Resolution::R320X240);
    gpu::set_draw_area(0, 0, 319, 239);
    gpu::set_draw_offset(0, 0);

    backend::upload_assets(CART);
    sfx::init(AUDIO);
    pico8::menusfx::init(); // dedicated UI sample bank for the pause overlay
    game::init();

    // Drive audio off real VBlanks so the music keeps tempo when rendering can't
    // hold 60fps (see celeste1).
    psx_rt::interrupts::install_vblank_counter();
    let mut fb = Screen::new();
    let mut prev_start = true; // require a fresh press before the first pause
                               // Cart logic runs on the shared psx-tick clock, one update per VBlank. A
                               // frame that missed VBlanks catches up (up to four updates before it draws,
                               // the rest over the next frames) instead of slowing the game down.
    let mut clock = psx_tick::FixedClock::new(
        psx_tick::TickConfig::new(psx_tick::TickRate::HZ60).with_interleave(4),
        psx_rt::interrupts::vblank_count(),
    );

    loop {
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

        // PICO-8 buttons: arrows, Cross = jump (btn 4), Circle = grapple (btn 5).
        let mut mask = 0u8;
        if b.is_held(button::LEFT) {
            mask |= game::IN_LEFT;
        }
        if b.is_held(button::RIGHT) {
            mask |= game::IN_RIGHT;
        }
        if b.is_held(button::UP) {
            mask |= game::IN_UP;
        }
        if b.is_held(button::DOWN) {
            mask |= game::IN_DOWN;
        }
        if b.is_held(button::CROSS) {
            mask |= game::IN_JUMP;
        }
        if b.is_held(button::CIRCLE) {
            mask |= game::IN_GRAPPLE;
        }
        if b.is_held(button::TRIANGLE) {
            mask |= game::IN_FLY; // debug fly (only acts when fly mode is on)
        }
        game::set_input(mask);

        let now = psx_rt::interrupts::vblank_count();
        while clock.due(now) {
            game::update();
            game::observe_update();
        }
        clock.end_frame();

        let (r, g, b) = game::bg_rgb();
        fb.clear(r, g, b); // cls(level.bg); the side bars repaint the margins
        backend::set_deferred(true); // list the frame, draw it by DMA
        game::draw();
        game::observe();
        backend::submit(); // the GPU draws; swap waits for it and renders audio meanwhile
        backend::set_deferred(false); // waits for the GPU
        fb.swap();

        sfx::update(); // stream the next slice of PICO-8 audio to the SPU
    }
}

/// Pause overlay: see [`pico8::harness::run_pause`]. `psfx(7,..)` (jump) is the slider feedback.
fn run_pause(fb: &mut Screen) -> bool {
    pico8::harness::run_pause(fb, 7, (0, 0, 16), game::draw)
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

/// Offline scene capture: boot straight into level `level` with no input, the
/// follow-pan off (so the 2x playfield is a fixed 8px crop top and bottom), and
/// run the game loop. Driven by the `scene` binary + tools/psx-audio-capture's
/// frametest; compared with a PICO-8 frame by tools/scene_cmp.py.
pub fn run_scene(level: i32) {
    gpu::init(VideoMode::Ntsc, Resolution::R320X240);
    gpu::set_draw_area(0, 0, 319, 239);
    gpu::set_draw_offset(0, 0);
    backend::upload_assets(CART);
    sfx::init(AUDIO);
    backend::set_screen_follow(false);
    pico8::rng::srand(42);
    game::start_at_level(level);
    psx_rt::interrupts::install_vblank_counter();
    let mut fb = Screen::new();
    loop {
        game::set_input(0);
        game::update();
        let (r, g, b) = game::bg_rgb();
        fb.clear(r, g, b);
        backend::set_deferred(true); // list the frame, draw it by DMA
        game::draw();
        backend::submit(); // the GPU draws; swap waits for it and renders audio meanwhile
        backend::set_deferred(false); // waits for the GPU
        fb.swap();
        sfx::update();
    }
}

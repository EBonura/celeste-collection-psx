//! PICO-8 audio on the PS1: a fixed-point port of the PICO-8 synthesiser
//! (four channels of oscillators, the sfx/music sequencer, custom instruments
//! and effects, as reverse-engineered by zepto8), mixed on the CPU at 22050 Hz
//! and streamed to ONE SPU voice through a ring of ADPCM blocks in SPU RAM.
//!
//! Why software: the SPU can only replay sampled wavetables through a 4-bit
//! ADPCM decoder and a gaussian interpolator, which blurs PICO-8's raw
//! oscillators (low notes lose harmonics, the LFSR noise is narrowband, the
//! phaser can't beat, per-note re-triggers click). Rendering PICO-8's actual
//! waveforms in software and streaming the mix keeps every detail of the
//! original; the only loss left is the ADPCM quantisation of the final mix.
//!
//! Timing: the sequencer is clocked by the SPU's own sample consumption (the
//! stream position is fed back through the SPU IRQ-address latch), so the
//! tempo is exactly 183 samples per speed unit like PICO-8, independent of
//! the frame rate. `update` must be called every frame; it polls the latch,
//! renders as many 28-sample blocks as the play head has consumed, and DMAs
//! them into the ring ahead of it.
//!
//! The sound data is the cart's own 0x3100/0x3200 RAM (tools/p8_audio.py);
//! the synthesiser itself lives in [`crate::synth`], this module is the stream.

#![allow(static_mut_refs)]

use crate::synth::{self, BLOCK_BYTES, BLOCK_SAMPLES, SAMPLE_RATE};
use psx_io::spu::{SPUCNT, SPUSTAT};
use psx_spu as spu;
use spu::{Adsr, Pitch, SpuAddr, Voice, Volume};

pub use crate::synth::{
    music, music_volume, play, play_ch, set_music_mute, set_music_volume, set_sfx_volume,
    sfx_volume, AudioData,
};

// ---------------------------------------------------------------------------
// Stream geometry
// ---------------------------------------------------------------------------

/// Ring of ADPCM blocks the stream voice loops over: 128 blocks = 3584
/// samples = 162 ms. Sits right after psx-spu's silence block; the menu one-shot
/// bank starts at 0x4000.
const RING_BLOCKS: u32 = 128;
const RING_BASE: u32 = 0x1010;
const STREAM_VOICE: u8 = 0;
/// How far ahead of the play head the writer stays, in blocks (~51 ms). This is
/// the SFX latency; PICO-8's own output buffer is of the same order. A dropped
/// frame eats 13 blocks, so this survives a ~3-frame hitch before the play head
/// catches the writer.
const LEAD_BLOCKS: u32 = 40;
/// Blocks the play head consumes per 60 Hz frame, Q16 (22050 / 60 / 28). The
/// IRQ latch corrects the real ratio; this only extrapolates between polls.
const BLOCKS_PER_FRAME_Q16: i64 = 860_160;
/// IRQ marker distance ahead of the estimate: more than a frame's consumption
/// so a lagging estimate is pulled forward by every latch.
const MARK_AHEAD: u32 = 16;
/// Most blocks rendered in one update (bounds the staging buffer + CPU spike).
const MAX_BLOCKS_PER_UPDATE: u32 = 64;
const SPU_IRQ_ADDR: u32 = 0x1F80_1DA4;
const SPUCNT_IRQ_ENABLE: u16 = 1 << 6;
const SPUSTAT_IRQ_FLAG: u16 = 1 << 6;

// ---------------------------------------------------------------------------
// Stream state
// ---------------------------------------------------------------------------
// Stream state (block units; monotonic counters, ring index = n % RING_BLOCKS).
static mut STARTED: bool = false;
static mut PLAY_Q16: i64 = 0; // estimated play head
static mut WRITE: u32 = 0; // next block to render
static mut MARK: u32 = 0; // block the SPU IRQ latch is armed on
static mut LAST_VBLANK: u32 = 0;

/// DMA source for the ring uploads (word-aligned, whole ring so init can fill
/// it in one go).
#[repr(C, align(4))]
struct Stage([u8; (RING_BLOCKS as usize) * BLOCK_BYTES]);
static mut STAGE: Stage = Stage([0; (RING_BLOCKS as usize) * BLOCK_BYTES]);

// ---------------------------------------------------------------------------
// Stream
// ---------------------------------------------------------------------------

#[inline]
fn reg_read(addr: u32) -> u16 {
    unsafe { core::ptr::read_volatile(addr as *const u16) }
}
#[inline]
fn reg_write(addr: u32, v: u16) {
    unsafe { core::ptr::write_volatile(addr as *mut u16, v) }
}

#[inline]
fn ring_addr(block: u32) -> u32 {
    RING_BASE + (block % RING_BLOCKS) * BLOCK_BYTES as u32
}

/// Loop flags for the ring block at monotonic index `block`.
#[inline]
fn ring_flags(block: u32) -> u8 {
    match block % RING_BLOCKS {
        0 => 0x04,                         // loop start
        b if b == RING_BLOCKS - 1 => 0x03, // end + repeat -> back to the start
        _ => 0x00,
    }
}

/// The runtime's VBlank counter; the SDK only builds it for the console, and
/// the host-side `cargo check` of this crate gets a stub.
#[cfg(target_arch = "mips")]
fn vblank_count() -> u32 {
    psx_rt::interrupts::vblank_count()
}
#[cfg(not(target_arch = "mips"))]
fn vblank_count() -> u32 {
    0
}

/// Arm the SPU IRQ latch on ring block `MARK` (and clear a stale latch).
unsafe fn arm_mark() {
    reg_write(SPU_IRQ_ADDR, (ring_addr(MARK) >> 3) as u16);
    let cnt = reg_read(SPUCNT);
    reg_write(SPUCNT, cnt & !SPUCNT_IRQ_ENABLE); // clearing the enable acks the flag
    reg_write(SPUCNT, cnt | SPUCNT_IRQ_ENABLE);
}

/// Render blocks `WRITE..WRITE+n` into the stage buffer and DMA them into the
/// ring (in one or two runs around the wrap).
unsafe fn render_and_upload(n: u32) {
    let mut done = 0u32;
    while done < n {
        let first = WRITE % RING_BLOCKS;
        let run = (n - done).min(RING_BLOCKS - first);
        for b in 0..run {
            let mut pcm = [0i16; BLOCK_SAMPLES];
            synth::render_block(&mut pcm);
            let off = (b as usize) * BLOCK_BYTES;
            synth::encode_block(
                &pcm,
                ring_flags(WRITE),
                &mut STAGE.0[off..off + BLOCK_BYTES],
            );
            WRITE += 1;
        }
        let bytes = (run as usize) * BLOCK_BYTES;
        spu::upload_adpcm(SpuAddr::new(ring_addr(WRITE - run)), &STAGE.0[..bytes]);
        done += run;
    }
}

/// Key the stream voice on at block 0 (after the first blocks are in the ring).
unsafe fn start_stream() {
    let v = Voice::new(STREAM_VOICE);
    v.set_volume(Volume::MAX, Volume::MAX);
    v.set_pitch(Pitch::for_sample_rate(SAMPLE_RATE));
    v.set_adsr(Adsr {
        lower: 0x000F, // instant attack, full sustain
        upper: 0x0000,
    });
    v.set_start_addr(SpuAddr::new(RING_BASE));
    v.set_loop_addr(SpuAddr::new(RING_BASE));
    Voice::key_on(1 << STREAM_VOICE);
    PLAY_Q16 = 0;
    MARK = MARK_AHEAD;
    arm_mark();
    LAST_VBLANK = vblank_count();
    STARTED = true;
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------
/// Initialise the SPU and load the game's [`AudioData`]. Call once after boot.
pub fn init(audio: AudioData) {
    synth::load(audio);
    unsafe {
        STARTED = false;
        WRITE = 0;

        spu::init();
        spu::set_main_volume(Volume::MAX, Volume::MAX);
        // Silent ring, with its loop flags in place.
        for b in 0..RING_BLOCKS {
            let off = (b as usize) * BLOCK_BYTES;
            STAGE.0[off] = 0;
            STAGE.0[off + 1] = ring_flags(b);
            for k in 2..BLOCK_BYTES {
                STAGE.0[off + k] = 0;
            }
        }
        spu::upload_adpcm(SpuAddr::new(RING_BASE), &STAGE.0);
    }
}

/// Advance the stream: call once per frame. Polls the SPU play-head latch,
/// renders the blocks consumed since last time and uploads them ahead of it.
pub fn update() {
    unsafe {
        if !STARTED {
            render_and_upload(LEAD_BLOCKS);
            start_stream();
            return;
        }
        // Extrapolate the play head by the VBlanks elapsed (a dropped frame
        // consumed two frames of audio), then correct it from the IRQ latch: the
        // flag means the head is at or past MARK; no flag means it is not yet.
        let vb = vblank_count();
        let elapsed = vb.wrapping_sub(LAST_VBLANK).clamp(1, 8) as i64;
        LAST_VBLANK = vb;
        PLAY_Q16 += BLOCKS_PER_FRAME_Q16 * elapsed;
        let mark_q16 = (MARK as i64) << 16;
        if reg_read(SPUSTAT) & SPUSTAT_IRQ_FLAG != 0 {
            if PLAY_Q16 < mark_q16 {
                PLAY_Q16 = mark_q16;
            }
            MARK = (PLAY_Q16 >> 16) as u32 + MARK_AHEAD;
            arm_mark();
        } else if PLAY_Q16 >= mark_q16 {
            PLAY_Q16 = mark_q16 - 32768;
        }

        let play = (PLAY_Q16 >> 16) as u32;
        if WRITE < play + 2 {
            // The head overran the writer (a long stall): skip ahead. The ring's
            // stale lap plays for the gap; the decoder history is stale too.
            WRITE = play + 4;
            synth::reset_encoder();
        }
        let target = play + LEAD_BLOCKS;
        if target > WRITE {
            render_and_upload((target - WRITE).min(MAX_BLOCKS_PER_UPDATE));
        }
    }
}

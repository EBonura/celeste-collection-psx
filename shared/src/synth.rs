//! The PICO-8 synthesiser: a fixed-point port of the four-channel oscillator
//! bank, the sfx/music sequencer, effects and custom instruments as
//! reverse-engineered by zepto8 (src/synth.cpp, src/pico8/sfx.cpp), rendering
//! 22050 Hz PCM in 28-sample blocks, plus the SPU ADPCM encoder for them.
//!
//! Pure: no hardware access, so it also compiles on the host for the tests at
//! the bottom (`rustc --test shared/src/synth.rs`), which render the games'
//! SFX to WAV for comparison with PICO-8 recordings.

#![allow(static_mut_refs)]

pub const SAMPLE_RATE: u32 = 22_050;
pub const BLOCK_SAMPLES: usize = 28;
pub const BLOCK_BYTES: usize = 16;

/// A game's PICO-8 sound RAM, byte-for-byte.
#[derive(Clone, Copy)]
pub struct AudioData {
    /// 64 SFX x 68 bytes: 32 note words (LE u16: pitch 0-5, wave 6-8, vol 9-11,
    /// effect 12-14, custom bit 15), then editor-mode, speed, loop start, loop end.
    pub sfx: &'static [[u8; 68]; 64],
    /// 64 patterns x 4 channel bytes: sfx 0-5, bit 6 = off, bit 7 = flag
    /// (loop start / loop end / stop on bytes 0 / 1 / 2).
    pub music: &'static [[u8; 4]; 64],
}

static EMPTY_SFX: [[u8; 68]; 64] = [[0; 68]; 64];
static EMPTY_MUSIC: [[u8; 4]; 64] = [[0x40; 4]; 64];
const EMPTY_AUDIO: AudioData = AudioData {
    sfx: &EMPTY_SFX,
    music: &EMPTY_MUSIC,
};

// ---------------------------------------------------------------------------
// Synth constants
// ---------------------------------------------------------------------------

/// PICO-8 key 0..63 -> Hz in Q8 (440 * 2^((key-33)/12)).
const KEY_FREQ: [i32; 64] = [
    16744, 17740, 18795, 19912, 21096, 22351, 23680, 25088, 26580, 28160, 29834, 31609, 33488,
    35479, 37589, 39824, 42192, 44701, 47359, 50175, 53159, 56320, 59669, 63217, 66976, 70959,
    75178, 79649, 84385, 89402, 94719, 100351, 106318, 112640, 119338, 126434, 133952, 141918,
    150356, 159297, 168769, 178805, 189437, 200702, 212636, 225280, 238676, 252868, 267905, 283835,
    300713, 318594, 337539, 357610, 378874, 401403, 425272, 450560, 477352, 505737, 535809, 567670,
    601425, 637188,
];
/// 2^32 / 22050: Hz -> phase increment per sample (Q32 turns).
const HZ_TO_INC: u64 = 194_783;
/// One note row in the Q32 sequencer offset.
const ROW: i64 = 1 << 32;
/// Rows advanced per block at speed 1: 28 samples / 183 samples-per-unit, Q32.
const ROWS_PER_BLOCK_Q32: i64 = 657_153_466;
/// 7.5 * 183 / 22050 in Q16: vibrato/arpeggio clock per (row * speed).
const VIB_RATE_Q16: i64 = 4079;
/// pow(2, 1/12) - 1 in Q16: vibrato depth (half a semitone each way).
const SEMITONE_Q16: i64 = 3897;
/// Crossfade rate on a harsh parameter change: 130/s, per sample, Q16.
const FADE_STEP_Q16: i32 = 386;
/// Noise lowpass "scale" per key, Q16 (the one-pole coefficient is
/// scale/(1+scale)). Fitted to PICO-8 recordings of noise sweeps: 1.86 *
/// (freq / freq(63))^1.4. zepto8 uses freq / freq(63), which is too dull above
/// ~key 40 and too bright below.
const NOISE_SCALE_Q16: [i32; 64] = [
    747, 810, 878, 952, 1033, 1119, 1214, 1316, 1427, 1547, 1677, 1819, 1972, 2138, 2318, 2513,
    2725, 2954, 3203, 3473, 3766, 4083, 4427, 4799, 5204, 5642, 6117, 6632, 7191, 7797, 8453, 9165,
    9937, 10774, 11682, 12666, 13732, 14889, 16143, 17503, 18977, 20575, 22308, 24187, 26225,
    28433, 30828, 33425, 36240, 39293, 42602, 46190, 50081, 54299, 58872, 63831, 69207, 75036,
    81356, 88209, 95638, 103694, 112428, 121897,
];
/// Q16 fade-volume step per block for a 1 ms music fade (65536 * 28000 / 22050).
const MUSIC_FADE_Q16_PER_MS_BLOCK: i64 = 83_220;

const FX_SLIDE: u8 = 1;
const FX_VIBRATO: u8 = 2;
const FX_DROP: u8 = 3;
const FX_FADE_IN: u8 = 4;
const FX_FADE_OUT: u8 = 5;
const FX_ARP_FAST: u8 = 6;
const FX_ARP_SLOW: u8 = 7;

const INST_NOISE: u8 = 6;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Oscillator parameters for one channel (zepto8's synth_param).
#[derive(Clone, Copy)]
struct Synth {
    key: u8,
    instr: u8,
    custom: bool,
    is_music: bool,
    freq: i32, // Hz, Q8
    vol: i32,  // Q12, 4096 = 1.0
    effect: u8,
    phase: u32,
    phase2: u32,     // phaser's second (detuned) triangle
    noise_last: i32, // Q16
}
const SYNTH0: Synth = Synth {
    key: 0,
    instr: 0,
    custom: false,
    is_music: false,
    freq: 0,
    vol: 0,
    effect: 0,
    phase: 0,
    phase2: 0,
    noise_last: 0,
};

/// A playing SFX (main note track or a custom-instrument macro).
#[derive(Clone, Copy)]
struct SfxState {
    sfx: i32,     // -1 = idle
    offset: i64,  // rows, Q32
    time: i64,    // rows since launch, Q32
    prev_key: u8, // for slide
    prev_vol: i32,
}
const SFX0: SfxState = SfxState {
    sfx: -1,
    offset: 0,
    time: 0,
    prev_key: 24,
    prev_vol: 0,
};

#[derive(Clone, Copy)]
struct Channel {
    main: SfxState,
    custom: SfxState,
    length: i64, // rows (Q32) to play, 0 = whole sfx
    can_loop: bool,
    is_music: bool,
    sfx_music: i32, // music sfx parked while an sfx() borrows the channel
    last_instr: u8,
    last_key: u8,
    last_effect: u8,
    last: Synth,
    fade: i32, // Q16 crossfade weight of fade_synth, 0 = none
    fade_synth: Synth,
    ident: i64, // which note rows (main, custom) the last block played
    ramp: bool, // same note continuing: interpolate freq/vol across the block
    prev_freq: i32,
    prev_vol: i32,
}
const CH0: Channel = Channel {
    main: SFX0,
    custom: SFX0,
    length: 0,
    can_loop: true,
    is_music: false,
    sfx_music: -1,
    last_instr: 0xFF,
    last_key: 0xFF,
    last_effect: 0,
    last: SYNTH0,
    fade: 0,
    fade_synth: SYNTH0,
    ident: -1,
    ramp: false,
    prev_freq: 0,
    prev_vol: 0,
};

struct Music {
    pattern: i32,
    offset: i64, // row*speed units, Q32
    length: i64,
    fade_vol: i32,  // Q16
    fade_step: i32, // Q16 per block (negative = fading out)
    mask: u8,
}

static mut AUDIO: AudioData = EMPTY_AUDIO;

/// The sequencer and oscillator state. The synth reads and writes it on every
/// block, so on the console it lives in the 1 KiB scratchpad behind the mix
/// and PCM buffers (main RAM stalls every load); the host keeps a static.
#[repr(C)]
struct State {
    channels: [Channel; 4],
    music: Music,
}
const STATE0: State = State {
    channels: [CH0; 4],
    music: Music {
        pattern: -1,
        offset: 0,
        length: 0,
        fade_vol: 65536,
        fade_step: 0,
        mask: 0,
    },
};
#[cfg(target_arch = "mips")]
unsafe fn state() -> &'static mut State {
    const _: () = assert!(
        0xC0 + core::mem::size_of::<State>() <= 1024,
        "synth state overflows the scratchpad"
    );
    &mut *(0x1F80_00C0 as *mut State)
}
#[cfg(not(target_arch = "mips"))]
unsafe fn state() -> &'static mut State {
    static mut STATE: State = STATE0;
    &mut *core::ptr::addr_of_mut!(STATE)
}
#[allow(non_snake_case)]
#[inline(always)]
unsafe fn CHANNELS() -> &'static mut [Channel; 4] {
    &mut state().channels
}
#[allow(non_snake_case)]
#[inline(always)]
unsafe fn MUSIC() -> &'static mut Music {
    &mut state().music
}
/// Index just past the last audible note of each sfx (0 = all silent).
static mut LAST_NOTE: [u8; 64] = [0; 64];
/// Rows (Q32) one block advances at each SFX speed: 28 * ROW / (183 * speed).
/// Tabulated at load; the 64-bit division cost more than a note's rendering.
static mut ROWS_PER_BLOCK_AT_SPEED: [i64; 256] = [0; 256];
/// Pause-menu gains in eighths (8 = unity).
static mut MUSIC_GAIN: i32 = 8;
static mut SFX_GAIN: i32 = 8;
/// Test harness: bit c silences music on channel c.
static mut MUSIC_MUTE: u8 = 0;
static mut RNG: u32 = 0x9E37_79B9;

static mut ENC_S1: i32 = 0;
static mut ENC_FILTER: usize = 0; // last predictor picked by the search
static mut ENC_BLOCK: u32 = 0; // blocks encoded (the search runs every fourth)
static mut ENC_S2: i32 = 0;

// ---------------------------------------------------------------------------
// Cart data accessors
// ---------------------------------------------------------------------------

#[inline]
unsafe fn note(sfx: usize, n: usize) -> u16 {
    let s = &AUDIO.sfx[sfx];
    s[n * 2] as u16 | ((s[n * 2 + 1] as u16) << 8)
}
#[inline]
fn note_key(w: u16) -> u8 {
    (w & 0x3F) as u8
}
#[inline]
fn note_instr(w: u16) -> u8 {
    ((w >> 6) & 7) as u8
}
#[inline]
fn note_vol(w: u16) -> i32 {
    ((w >> 9) & 7) as i32
}
#[inline]
fn note_effect(w: u16) -> u8 {
    ((w >> 12) & 7) as u8
}
#[inline]
fn note_custom(w: u16) -> bool {
    w & 0x8000 != 0
}
#[inline]
unsafe fn sfx_speed(sfx: usize) -> i32 {
    (AUDIO.sfx[sfx][65] as i32).max(1)
}
#[inline]
unsafe fn sfx_loop(sfx: usize) -> (i32, i32) {
    (AUDIO.sfx[sfx][66] as i32, AUDIO.sfx[sfx][67] as i32)
}

// ---------------------------------------------------------------------------
// Sequencer (per 28-sample block)
// ---------------------------------------------------------------------------

#[inline]
fn mix_q16(a: i32, b: i32, t: i32) -> i32 {
    a + (((b - a) as i64 * t as i64) >> 16) as i32
}

/// Advance one SFX track by a block and, if it is sounding, fill `out` with the
/// note's oscillator parameters. `freq_factor` (Q16) scales the pitch (custom
/// instruments transpose around C2).
unsafe fn update_sfx(
    st: &mut SfxState,
    out: &mut Synth,
    freq_factor: i32,
    length: i64,
    is_music: bool,
    can_loop: bool,
) -> i32 {
    if st.sfx < 0 {
        return -1;
    }
    let index = st.sfx as usize;
    let speed = sfx_speed(index);
    let (loop_start, loop_end) = sfx_loop(index);
    let per_block = ROWS_PER_BLOCK_AT_SPEED[speed as usize];
    let offset = st.offset;
    let mut next_offset = offset + per_block;
    let next_time = st.time + per_block;

    let loop_range = loop_end - loop_start;
    if loop_range > 0 && next_offset >= (loop_end as i64) * ROW && can_loop {
        let ls = (loop_start as i64) * ROW;
        next_offset = (next_offset - ls) % ((loop_range as i64) * ROW) + ls;
    }

    let mut has_end = false;
    let mut end_time = 32 * ROW;
    if length > 0 {
        has_end = true;
        end_time = length;
    }
    // PICO-8 quirk: a loop-start without loop-end is a LENGTH marker, but only
    // outside music (music only uses it for the pattern length).
    if !is_music && loop_end == 0 && loop_start > 0 {
        has_end = true;
        end_time = end_time.min((loop_start as i64) * ROW);
    }
    if loop_range <= 0 {
        has_end = true;
        if !is_music {
            end_time = end_time.min((LAST_NOTE[index] as i64) * ROW);
        }
    }

    let mut played = -1;
    if offset < 32 * ROW {
        let note_id = (offset >> 32) as usize;
        let next_note_id = (next_offset >> 32) as usize;
        let w = note(index, note_id);
        let key = note_key(w);
        let vol7 = note_vol(w);
        let mut volume = vol7 * 4096 / 7;
        let mut freq = ((KEY_FREQ[key as usize] as i64 * freq_factor as i64) >> 16) as i32;

        if volume > 0 {
            let frac = ((offset >> 16) & 0xFFFF) as i32; // Q16 position within the row
            match note_effect(w) {
                FX_SLIDE => {
                    // Documented as "slide to the next note", actually FROM the
                    // previous one. (zepto8 mixes from the unscaled previous key;
                    // for a custom-instrument macro that would glide from the wrong
                    // octave, so the previous key is scaled the same as this one.)
                    let from =
                        ((KEY_FREQ[st.prev_key as usize] as i64 * freq_factor as i64) >> 16) as i32;
                    freq = mix_q16(from, freq, frac);
                    if st.prev_vol > 0 {
                        volume = mix_q16(st.prev_vol, volume, frac);
                    }
                }
                FX_VIBRATO => {
                    // 7.5 Hz, half a semitone: t = |frac(7.5 * offset / ops) - 0.5| - 0.25
                    let v = ((offset >> 16) * speed as i64 * VIB_RATE_Q16) >> 16;
                    let t = ((v & 0xFFFF) as i32 - 32768).abs() - 16384;
                    freq += ((freq as i64 * SEMITONE_Q16 * t as i64) >> 32) as i32;
                }
                FX_DROP => freq -= ((freq as i64 * frac as i64) >> 16) as i32,
                FX_FADE_IN => volume = ((volume as i64 * frac as i64) >> 16) as i32,
                FX_FADE_OUT => volume = ((volume as i64 * (65536 - frac) as i64) >> 16) as i32,
                FX_ARP_FAST | FX_ARP_SLOW => {
                    // Cycle the 4-note group at a fixed real-time rate (halved
                    // for speed <= 8).
                    let m = (if speed <= 8 { 32 } else { 16 })
                        / (if note_effect(w) == FX_ARP_FAST { 4 } else { 8 });
                    let v = ((offset >> 16) * speed as i64 * VIB_RATE_Q16) >> 16;
                    let n = ((m as i64 * v) >> 16) as usize;
                    let arp = (note_id & !3) | (n & 3);
                    freq = ((KEY_FREQ[note_key(note(index, arp)) as usize] as i64
                        * freq_factor as i64)
                        >> 16) as i32;
                }
                _ => {}
            }
            out.key = key;
            out.freq = freq;
            out.instr = note_instr(w);
            out.custom = note_custom(w);
            out.vol = volume;
            out.effect = note_effect(w);
            out.is_music = is_music;
            played = (index * 32 + note_id) as i32;
        }

        if next_note_id != note_id {
            st.prev_key = key;
            st.prev_vol = vol7 * 4096 / 7;
        }
    }

    st.offset = next_offset;
    st.time = next_time;
    if has_end && next_time >= end_time {
        st.sfx = -1;
    }
    played
}

/// Pattern length in row*speed units: the first non-looping channel decides,
/// else the slowest looping one.
unsafe fn pattern_duration(pattern: usize) -> i64 {
    let mut dur_loop: i64 = -1;
    let mut dur_noloop: i64 = -1;
    for c in 0..4 {
        let b = AUDIO.music[pattern][c];
        if b & 0x40 != 0 {
            continue;
        }
        let n = (b & 0x3F) as usize;
        let (ls, le) = sfx_loop(n);
        let speed = AUDIO.sfx[n][65] as i64;
        if le > 0 && le > ls {
            dur_loop = dur_loop.max(32 * speed);
        } else {
            let mut end = 32i64;
            if le == 0 && ls > 0 {
                end = end.min(ls as i64);
            }
            dur_noloop = end * speed;
            break;
        }
    }
    if dur_noloop > 0 {
        dur_noloop
    } else {
        dur_loop
    }
}

unsafe fn launch_sfx(sfx: i32, chan: usize, offset: i64, length: i64, is_music: bool) {
    let ch = &mut CHANNELS()[chan];
    ch.main.sfx = sfx;
    ch.main.offset = offset.max(0);
    ch.main.time = 0;
    ch.length = length.max(0);
    ch.can_loop = true;
    ch.is_music = is_music;
    ch.last_instr = 0xFF;
    ch.last_key = 0xFF;
    // C2 is the "previous key" a fresh slide starts from (no audible glide).
    ch.main.prev_key = 24;
    ch.main.prev_vol = 0;
}

unsafe fn set_music_pattern(pattern: i32) {
    for ch in CHANNELS().iter_mut() {
        if ch.is_music {
            ch.main.sfx = -1;
            ch.sfx_music = -1;
        }
    }
    if !(0..64).contains(&pattern) {
        MUSIC().pattern = -1;
        MUSIC().offset = 0;
        MUSIC().mask = 0;
        MUSIC().length = 0;
        return;
    }
    let duration = pattern_duration(pattern as usize);
    if duration <= 0 {
        MUSIC().pattern = -1;
        MUSIC().offset = 0;
        MUSIC().mask = 0;
        MUSIC().length = 0;
        return;
    }
    MUSIC().pattern = pattern;
    MUSIC().offset = 0;
    MUSIC().length = duration * ROW;
    for c in 0..4 {
        let b = AUDIO.music[pattern as usize][c];
        if b & 0x40 != 0 {
            continue;
        }
        let n = (b & 0x3F) as i32;
        if CHANNELS()[c].main.sfx == -1 {
            launch_sfx(n, c, 0, 0, true);
        } else {
            // An sfx() is borrowing the channel: the music resumes when it ends.
            CHANNELS()[c].sfx_music = n;
        }
    }
}

/// Advance the music clock by one block (called before the channels).
unsafe fn advance_music() {
    if MUSIC().pattern < 0 {
        return;
    }
    MUSIC().offset += ROWS_PER_BLOCK_Q32;
    MUSIC().fade_vol = (MUSIC().fade_vol + MUSIC().fade_step).clamp(0, 65536);
    if MUSIC().fade_step < 0 && MUSIC().fade_vol <= 0 {
        set_music_pattern(-1);
    } else if MUSIC().offset >= MUSIC().length {
        let pat = MUSIC().pattern as usize;
        let mut next = MUSIC().pattern + 1;
        if AUDIO.music[pat][2] & 0x80 != 0 {
            next = -1; // stop flag
        } else if AUDIO.music[pat][1] & 0x80 != 0 {
            // loop-end: back to the nearest loop-start at or before this pattern
            while {
                next -= 1;
                next > 0 && AUDIO.music[next as usize][0] & 0x80 == 0
            } {}
        }
        set_music_pattern(next);
    }
}

/// Resume a parked music sfx on an idle channel, at where the music clock is now.
unsafe fn resume_music_sfx(c: usize) {
    let ch = &mut CHANNELS()[c];
    if ch.main.sfx != -1 || ch.sfx_music == -1 {
        return;
    }
    let index = ch.sfx_music as usize;
    let speed = sfx_speed(index) as i64;
    let mut new_offset = MUSIC().offset / speed;
    let (ls, le) = sfx_loop(index);
    let loop_range = (le - ls) as i64;
    let mut want_play = true;
    if loop_range > 0 && ch.can_loop {
        if new_offset > (ls as i64) * ROW {
            new_offset = (new_offset - (ls as i64) * ROW) % (loop_range * ROW) + (ls as i64) * ROW;
        }
    } else if new_offset > 32 * ROW {
        want_play = false;
    }
    let sfx = ch.sfx_music;
    ch.sfx_music = -1;
    if want_play {
        launch_sfx(sfx, c, new_offset, 0, true);
    }
}

/// Sequence channel `c` one block: resolve its main note (+ custom-instrument
/// macro) into new oscillator parameters, detecting harsh changes for the
/// de-click crossfade.
unsafe fn sequence_channel(c: usize) {
    let ch = &mut CHANNELS()[c];
    let last = ch.last;
    let mut new = Synth {
        phase: last.phase,
        phase2: last.phase2,
        noise_last: last.noise_last,
        ..SYNTH0
    };
    let base_offset = ch.main.offset;
    let (length, is_music, can_loop) = (ch.length, ch.is_music, ch.can_loop);
    let row_main = update_sfx(&mut ch.main, &mut new, 65536, length, is_music, can_loop);
    let mut row_custom = -1;

    // A custom instrument restarts when the note's key or instrument changes, or
    // when a DROP row starts (PICO-8 recordings: celeste2 sfx59 re-hits its noise
    // drum on the e3 row but not on the e5 rows; sfx56's vibrato/fade rows on a
    // held bass sustain it). A finished one-shot instrument stays finished for
    // repeated rows (zepto8 re-fires it every row; the recording shows it doesn't).
    let mut restart_custom = new.instr != ch.last_instr
        || new.key != ch.last_key
        || (new.effect == FX_DROP && ch.last_effect != FX_DROP);
    ch.last_instr = new.instr;
    ch.last_key = new.key;
    ch.last_effect = new.effect;

    if new.custom {
        // Custom instrument: run SFX `instr` as a macro under the note. Restart it
        // on a note change even while the note's own volume is momentarily 0 (a
        // fade-in starts at 0; zepto8 skips the restart there and lets the
        // previous note's macro leak into the new one).
        if ch.main.offset < base_offset {
            restart_custom = true; // the main sfx looped
        }
        if restart_custom {
            ch.custom = SfxState {
                sfx: new.instr as i32,
                ..SFX0
            };
        }
        // Custom instruments transpose around C2: inner pitch * outer / C2.
        let ff = ((new.freq as i64) << 16) / KEY_FREQ[24] as i64;
        let main_vol = new.vol;
        // A rest inside the instrument, or a one-shot instrument that has already
        // finished, is silence (zepto8 falls through to the main note's parameters
        // there and plays the instrument index as a plain waveform).
        new.vol = 0;
        if ch.custom.sfx >= 0 {
            row_custom = update_sfx(&mut ch.custom, &mut new, ff as i32, 0, false, true);
            new.vol = new.vol * main_vol >> 12;
        }
    }

    // A note change (new row, arpeggio step, instrument) with a volume step >
    // 0.1, a pitch step > 1% or an instrument change is "harsh": crossfade from
    // the old parameters over ~8 ms instead of clicking (zepto8 tests this per
    // sample; per block, a smooth slide/drop/fade would trip the thresholds, so
    // it only applies on a change of note, and a continuing note ramps instead).
    let ident = ((row_main as i64) << 32) | (row_custom as i64 & 0xFFFF_FFFF);
    let changed = ident != ch.ident
        || new.key != last.key
        || new.instr != last.instr
        || new.custom != last.custom;
    ch.ident = ident;
    let thresh = new.freq.min(last.freq) / 100;
    let harsh = changed
        && ((new.vol - last.vol).abs() > 410
            || (new.freq - last.freq).abs() > thresh
            || new.instr != last.instr);
    if harsh {
        if ch.fade <= 0 {
            ch.fade_synth = last; // avoid chaining fades (it upsets the noise)
        }
        ch.fade = 65536;
        // The phaser's second triangle is re-derived from the phase on a note
        // change (zepto8 re-folds phi), which restarts its beat.
        new.phase2 = ((new.phase as u64) * 109 / 110) as u32;
    }
    ch.ramp = !harsh && last.vol > 0 && new.vol > 0;
    ch.prev_freq = last.freq;
    ch.prev_vol = last.vol;
    ch.last = new;
}

// ---------------------------------------------------------------------------
// Oscillators
// ---------------------------------------------------------------------------

#[inline]
fn rand_q16() -> i32 {
    // xorshift32 -> uniform in [-1, 1) Q16
    unsafe {
        let mut x = RNG;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        RNG = x;
        (x >> 15) as i32 - 65536
    }
}

/// Per-block noise coefficients (Q16): the lowpass pole `a` for the note's
/// current frequency (interpolated between the per-key table entries, so pitch
/// effects sweep it), and the level gain 1.24 * (1 + (1 - key/63)^2) -- PICO-8's
/// noise gets louder as well as darker toward low keys (fitted to recordings;
/// zepto8's 1.5 is ~20% hot).
#[inline]
fn noise_coefs(freq: i32, key: u8) -> (i32, i32) {
    let scale: i64 = if freq <= KEY_FREQ[0] {
        NOISE_SCALE_Q16[0] as i64 * freq as i64 / KEY_FREQ[0] as i64
    } else if freq >= KEY_FREQ[63] {
        NOISE_SCALE_Q16[63] as i64 * freq as i64 / KEY_FREQ[63] as i64
    } else {
        let mut k = 0usize;
        let mut step = 32;
        while step > 0 {
            if k + step < 64 && KEY_FREQ[k + step] <= freq {
                k += step;
            }
            step >>= 1;
        }
        let (f0, f1) = (KEY_FREQ[k] as i64, KEY_FREQ[k + 1] as i64);
        let (s0, s1) = (NOISE_SCALE_Q16[k] as i64, NOISE_SCALE_Q16[k + 1] as i64);
        s0 + (s1 - s0) * (freq as i64 - f0) / (f1 - f0)
    };
    let a = (scale << 16) / (65536 + scale);
    let f = 65536 - key as i64 * 1040;
    let gain = (81264 * (65536 + ((f * f) >> 16))) >> 16;
    (a as i32, gain as i32)
}

/// One oscillator sample in Q15 (32768 = 1.0). PICO-8's waveforms are the raw
/// functions of phase, unfiltered, aliasing included -- that IS the sound.
#[inline(always)]
fn wave(
    instr: u8,
    phase: u32,
    phase2: u32,
    noise_last: &mut i32,
    noise_a: i32,
    noise_gain: i32,
) -> i32 {
    let x = (phase >> 16) as i32; // Q16 in [0, 1)
    match instr {
        0 => (65536 - (4 * x - 131072).abs()) >> 2, // triangle * 0.5
        1 => {
            // tilted saw, peak at 0.875
            let v = if x < 57344 {
                x * 16 / 7 - 65536
            } else {
                (65536 - x) * 16 - 65536
            };
            v >> 2
        }
        2 => {
            // saw * 0.653
            let v = if x < 32768 { x } else { x - 65536 };
            (v * 21398) >> 16
        }
        3 => {
            if x < 32768 {
                8192
            } else {
                -8192
            }
        }
        4 => {
            // pulse, duty 0.316
            if x < 20709 {
                8192
            } else {
                -8192
            }
        }
        5 => {
            // organ: two triangles / 9
            let v = if x < 32768 {
                196608 - (24 * x - 393216).abs()
            } else {
                65536 - (16 * x - 786432).abs()
            };
            (v * 3641) >> 16
        }
        6 => {
            // brown-ish noise: one-pole lowpass of white noise, cutoff tracks pitch
            // Q16 state and coefficients, folded to Q14 x Q14 so the products
            // stay in 32 bits (the PS1 has no 64-bit multiply).
            let r = rand_q16();
            let nl =
                ((*noise_last >> 2) * ((65536 - noise_a) >> 2) + (r >> 2) * (noise_a >> 2)) >> 12;
            *noise_last = nl;
            ((nl >> 2) * (noise_gain >> 2)) >> 13
        }
        _ => {
            // phaser: a triangle plus a second one at 109/110 the pitch, / 6
            let x2 = (phase2 >> 16) as i32;
            let v = (131072 - (8 * x - 262144).abs()) + (65536 - (4 * x2 - 131072).abs());
            (v * 5461) >> 16
        }
    }
}

/// Effective Q12 gain of a synth (note volume x music fade x pause-menu slider).
#[inline]
unsafe fn synth_gain(s: &Synth, c: usize) -> i32 {
    if s.is_music {
        if MUSIC_MUTE & (1 << c) != 0 {
            return 0;
        }
        ((s.vol as i64 * MUSIC().fade_vol as i64 * MUSIC_GAIN as i64) >> 19) as i32
    } else {
        s.vol * SFX_GAIN / 8
    }
}

/// Q12 gain -> Q15 wave scaling: PICO-8's oscillators peak at half of zepto8's
/// (measured: every recorded level is exactly half), so `>> 13`, not `>> 12`.
const GAIN_SHIFT: u32 = 13;

#[inline]
fn hz_to_inc(freq: i32) -> u32 {
    ((freq.max(0) as u64 * HZ_TO_INC) >> 8) as u32
}

/// Render one block of one channel into `mix` (i32 accumulator).
unsafe fn render_channel(c: usize, mix: &mut [i32; BLOCK_SAMPLES]) {
    let ch = &mut CHANNELS()[c];
    let gain = synth_gain(&ch.last, c);
    let fade_gain = if ch.fade > 0 {
        synth_gain(&ch.fade_synth, c)
    } else {
        0
    };
    if gain == 0 && (ch.fade <= 0 || fade_gain == 0) {
        // Silent: keep the phase running (a continuing note stays continuous) and
        // let a pending fade expire.
        let inc = hz_to_inc(ch.last.freq);
        ch.last.phase = ch
            .last
            .phase
            .wrapping_add(inc.wrapping_mul(BLOCK_SAMPLES as u32));
        ch.last.phase2 = ch
            .last
            .phase2
            .wrapping_add((inc / 110 * 109).wrapping_mul(BLOCK_SAMPLES as u32));
        if ch.fade > 0 {
            ch.fade = (ch.fade - FADE_STEP_Q16 * BLOCK_SAMPLES as i32).max(0);
        }
        return;
    }
    let s = &mut ch.last;
    let instr = s.instr;
    let (na, ng) = if instr == INST_NOISE {
        noise_coefs(s.freq, s.key)
    } else {
        (0, 0)
    };
    // A continuing note interpolates pitch and level from the previous block's
    // values (smooth slides/drops/fades, like PICO-8's per-sample evaluation).
    let (f, df, mut g, dg) = if ch.ramp {
        let g0 = synth_gain(
            &Synth {
                vol: ch.prev_vol,
                ..*s
            },
            c,
        );
        (
            ch.prev_freq,
            (s.freq - ch.prev_freq) / BLOCK_SAMPLES as i32,
            g0,
            (gain - g0) / BLOCK_SAMPLES as i32,
        )
    } else {
        (s.freq, 0, gain, 0)
    };
    // The phase increment is linear in the pitch, so a ramp steps it per sample
    // rather than recomputing it (a 64-bit multiply and, for the phaser's
    // detuned voice, a division) for every sample: on the PS1 that arithmetic
    // was most of the synth's cost.
    let mut inc = hz_to_inc(f);
    let mut inc2 = inc / 110 * 109;
    let dinc = ((df as i64 * HZ_TO_INC as i64) >> 8) as i32;
    let dinc2 = dinc / 110 * 109;

    if ch.fade > 0 {
        let fsy = &mut ch.fade_synth;
        let finc = hz_to_inc(fsy.freq);
        let finc2 = finc / 110 * 109;
        let (fna, fng) = if fsy.instr == INST_NOISE {
            noise_coefs(fsy.freq, fsy.key)
        } else {
            (0, 0)
        };
        let finstr = fsy.instr;
        let mut fade = ch.fade;
        for m in mix.iter_mut() {
            inc = inc.wrapping_add_signed(dinc);
            inc2 = inc2.wrapping_add_signed(dinc2);
            g += dg;
            s.phase = s.phase.wrapping_add(inc);
            s.phase2 = s.phase2.wrapping_add(inc2);
            let v = (wave(instr, s.phase, s.phase2, &mut s.noise_last, na, ng) * g >> GAIN_SHIFT)
                .clamp(-32767, 32767);
            fsy.phase = fsy.phase.wrapping_add(finc);
            fsy.phase2 = fsy.phase2.wrapping_add(finc2);
            let fv = (wave(finstr, fsy.phase, fsy.phase2, &mut fsy.noise_last, fna, fng)
                * fade_gain
                >> GAIN_SHIFT)
                .clamp(-32767, 32767);
            let w = fade.max(0) as i64;
            *m += v + (((fv - v) as i64 * w) >> 16) as i32;
            fade -= FADE_STEP_Q16;
        }
        ch.fade = fade.max(0);
    } else {
        let osc = Osc {
            inc,
            dinc,
            inc2,
            dinc2,
            g,
            dg,
            na,
            ng,
        };
        // One loop per waveform so the dispatch is hoisted out of the samples.
        match instr {
            0 => render_osc::<0>(s, mix, osc),
            1 => render_osc::<1>(s, mix, osc),
            2 => render_osc::<2>(s, mix, osc),
            3 => render_osc::<3>(s, mix, osc),
            4 => render_osc::<4>(s, mix, osc),
            5 => render_osc::<5>(s, mix, osc),
            6 => render_osc::<6>(s, mix, osc),
            _ => render_osc::<7>(s, mix, osc),
        }
    }
}

/// Per-block oscillator ramp: phase increments and gain, each stepped per sample.
#[derive(Clone, Copy)]
struct Osc {
    inc: u32,
    dinc: i32,
    inc2: u32,
    dinc2: i32,
    g: i32,
    dg: i32,
    na: i32,
    ng: i32,
}

/// Add one block of waveform `W` to `mix`. A channel's level is at most
/// 8192 (|wave| <= 16384 at gain 4096 >> 13), so it needs no clamp of its own:
/// the mix is clamped once at the end.
#[inline(never)]
fn render_osc<const W: u8>(s: &mut Synth, mix: &mut [i32; BLOCK_SAMPLES], o: Osc) {
    let Osc {
        mut inc,
        dinc,
        mut inc2,
        dinc2,
        mut g,
        dg,
        na,
        ng,
    } = o;
    let (mut phase, mut phase2, mut noise) = (s.phase, s.phase2, s.noise_last);
    for m in mix.iter_mut() {
        inc = inc.wrapping_add_signed(dinc);
        inc2 = inc2.wrapping_add_signed(dinc2);
        g += dg;
        phase = phase.wrapping_add(inc);
        phase2 = phase2.wrapping_add(inc2);
        *m += wave(W, phase, phase2, &mut noise, na, ng) * g >> GAIN_SHIFT;
    }
    s.phase = phase;
    s.phase2 = phase2;
    s.noise_last = noise;
}

// ---------------------------------------------------------------------------
// ADPCM encoder
// ---------------------------------------------------------------------------

const ADPCM_FILTERS: [(i32, i32); 5] = [(0, 0), (60, 0), (115, -52), (98, -55), (122, -60)];

/// Encode 28 PCM samples into one SPU ADPCM block, carrying the decoder history
/// (`ENC_S1/S2`) across blocks so the stream decodes continuously. Picks the
/// predictor by open-loop residual peak, then quantises closed-loop.
pub(crate) fn encode_block(pcm: &[i16; BLOCK_SAMPLES], flags: u8, out: &mut [u8; BLOCK_BYTES]) {
    let (s1, s2) = unsafe { (ENC_S1, ENC_S2) };
    // ponytail: the predictor is re-picked every fourth block (a note is
    // stationary for far longer than 5 ms; the SNR cost measured 1 dB on the
    // host bench); the other blocks keep the last winner and only measure its
    // residual peak for the shift. The search pass shares each filter's
    // products across all five candidates. (Reusing the shift too was tried:
    // another 1.5 dB, not worth 2% of the frame.)
    let search = unsafe { ENC_BLOCK & 3 == 0 };
    let mut best_f = unsafe { ENC_FILTER };
    let mut best_max = 0i32;
    let (mut p1, mut p2) = (s1, s2);
    if search {
        let mut rmax = [0i32; 5];
        for &x in pcm.iter() {
            let x = x as i32;
            let r = [
                x,
                x - ((p1 * 60) >> 6),
                x - ((p1 * 115) >> 6) - ((p2 * -52) >> 6),
                x - ((p1 * 98) >> 6) - ((p2 * -55) >> 6),
                x - ((p1 * 122) >> 6) - ((p2 * -60) >> 6),
            ];
            for f in 0..5 {
                rmax[f] = rmax[f].max(r[f].abs());
            }
            p2 = p1;
            p1 = x;
        }
        best_f = 0;
        best_max = rmax[0];
        for f in 1..5 {
            if rmax[f] < best_max {
                best_max = rmax[f];
                best_f = f;
            }
        }
    } else {
        let (k0, k1) = ADPCM_FILTERS[best_f];
        for &x in pcm.iter() {
            let x = x as i32;
            best_max = best_max.max((x - ((p1 * k0) >> 6) - ((p2 * k1) >> 6)).abs());
            p2 = p1;
            p1 = x;
        }
    }
    unsafe {
        ENC_FILTER = best_f;
        ENC_BLOCK = ENC_BLOCK.wrapping_add(1);
    }
    let shift = shift_for(best_max);
    let (k0, k1) = ADPCM_FILTERS[best_f];
    quantize(pcm, k0, k1, shift, s1, s2, out);
    out[0] = ((best_f as u8) << 4) | shift as u8;
    out[1] = flags;
}

/// Smallest shift whose 4-bit range covers a residual peak.
fn shift_for(rmax: i32) -> u32 {
    let mut shift = 12u32;
    while shift > 0 && rmax > (7 << (12 - shift)) {
        shift -= 1;
    }
    shift
}

/// Closed-loop quantisation of one block with predictor `(k0, k1)` and `shift`,
/// from decoder history `(s1, s2)`. Writes the nibbles and the new history.
fn quantize(
    pcm: &[i16; BLOCK_SAMPLES],
    k0: i32,
    k1: i32,
    shift: u32,
    s1: i32,
    s2: i32,
    out: &mut [u8; BLOCK_BYTES],
) {
    let (mut p1, mut p2) = (s1, s2);
    let rs = 12 - shift;
    for i in 0..BLOCK_SAMPLES {
        let pred = ((p1 * k0) >> 6) + ((p2 * k1) >> 6);
        let r = pcm[i] as i32 - pred;
        let q = (if rs > 0 {
            (r + (1 << (rs - 1))) >> rs
        } else {
            r
        })
        .clamp(-8, 7);
        let dec = (((q << 12) >> shift) + pred).clamp(-0x8000, 0x7FFF);
        p2 = p1;
        p1 = dec;
        let n = (q & 0xF) as u8;
        if i & 1 == 0 {
            out[2 + i / 2] = n;
        } else {
            out[2 + i / 2] |= n << 4;
        }
    }
    unsafe {
        ENC_S1 = p1;
        ENC_S2 = p2;
    }
}

/// Forget the decoder history (after the stream skips ahead).
#[allow(dead_code)] // only the stream uses it; the host test build does not
pub(crate) fn reset_encoder() {
    unsafe {
        ENC_S1 = 0;
        ENC_S2 = 0;
        ENC_BLOCK = 0;
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------
/// Load a game's sound RAM and reset the sequencer.
pub fn load(audio: AudioData) {
    unsafe {
        AUDIO = audio;
        for i in 0..64 {
            let mut last = 0u8;
            for n in 0..32 {
                if note_vol(note(i, n)) > 0 {
                    last = (n + 1) as u8;
                }
            }
            LAST_NOTE[i] = last;
        }
        for (speed, rows) in ROWS_PER_BLOCK_AT_SPEED.iter_mut().enumerate() {
            *rows = (BLOCK_SAMPLES as i64) * ROW / (183 * speed.max(1) as i64);
        }
        *state() = STATE0;
        MUSIC().pattern = -1;
        MUSIC().fade_vol = 65536;
        MUSIC().fade_step = 0;
        MUSIC().mask = 0;
        ENC_S1 = 0;
        ENC_S2 = 0;
        ENC_FILTER = 0;
        ENC_BLOCK = 0;
    }
}

/// The four-channel mix accumulator. On the console it lives in the 1 KiB
/// scratchpad (zero-wait data RAM; the PS1 has no data cache), where main RAM
/// would stall every one of the ~450 accesses a block makes.
#[cfg(target_arch = "mips")]
unsafe fn mix_buf() -> &'static mut [i32; BLOCK_SAMPLES] {
    &mut *(0x1F80_0000 as *mut [i32; BLOCK_SAMPLES])
}
#[cfg(not(target_arch = "mips"))]
unsafe fn mix_buf() -> &'static mut [i32; BLOCK_SAMPLES] {
    static mut MIX: [i32; BLOCK_SAMPLES] = [0; BLOCK_SAMPLES];
    &mut *core::ptr::addr_of_mut!(MIX)
}

/// Render the next 28 samples of the four-channel mix.
pub fn render_block(pcm: &mut [i16; BLOCK_SAMPLES]) {
    unsafe {
        let mix = mix_buf();
        *mix = [0i32; BLOCK_SAMPLES];
        advance_music();
        for c in 0..4 {
            resume_music_sfx(c);
            sequence_channel(c);
            render_channel(c, mix);
        }
        for i in 0..BLOCK_SAMPLES {
            pcm[i] = mix[i].clamp(-32767, 32767) as i16;
        }
    }
}

/// PICO-8 `sfx(n)`: play sfx `n` on a free channel (-1 stops all non-music
/// channels, -2 stops their looping).
pub fn play(id: i32) {
    play_ch(id, -1, 0, 0);
}

/// PICO-8 `sfx(n, channel, offset, length)`: `channel` -1 auto-selects; `offset`
/// and `length` in notes (length 0 = to the end).
pub fn play_ch(id: i32, channel: i32, offset: i32, length: i32) {
    unsafe {
        if !(-2..64).contains(&id) || !(-1..4).contains(&channel) || offset > 31 {
            return;
        }
        if id < 0 {
            // -1 = stop, -2 = stop looping; on one channel or every sfx channel.
            for c in 0..4 {
                if channel != -1 && channel as usize != c {
                    continue;
                }
                if !CHANNELS()[c].is_music {
                    if id == -1 {
                        CHANNELS()[c].main.sfx = -1;
                    } else {
                        CHANNELS()[c].can_loop = false;
                    }
                }
            }
            return;
        }
        let mut chan = channel;
        let mask = MUSIC().mask;
        if chan == -1 {
            // A free channel, or one already playing this sfx (PICO-8 reuses it).
            for c in 0..4 {
                if mask & (1 << c) != 0 {
                    continue;
                }
                if CHANNELS()[c].main.sfx == -1 || CHANNELS()[c].main.sfx == id {
                    chan = c as i32;
                    break;
                }
            }
        }
        if chan == -1 {
            // Else borrow the first music channel not reserved by music()'s mask.
            for c in 0..4 {
                if mask & (1 << c) == 0 && CHANNELS()[c].is_music {
                    chan = c as i32;
                    break;
                }
            }
        }
        if chan == -1 {
            // Else the channel playing the fastest sfx (the latest on a tie).
            let mut fastest = 256;
            for c in 0..4 {
                if mask & (1 << c) != 0 {
                    continue;
                }
                let s = CHANNELS()[c].main.sfx;
                if !(0..64).contains(&s) {
                    continue;
                }
                let speed = AUDIO.sfx[s as usize][65] as i32;
                if speed <= fastest {
                    chan = c as i32;
                    fastest = speed;
                }
            }
        }
        if chan == -1 {
            return;
        }
        for c in 0..4 {
            if CHANNELS()[c].main.sfx == id {
                CHANNELS()[c].main.sfx = -1;
            }
        }
        let c = chan as usize;
        if CHANNELS()[c].main.sfx != -1 && CHANNELS()[c].is_music {
            CHANNELS()[c].sfx_music = CHANNELS()[c].main.sfx; // music resumes afterwards
        }
        launch_sfx(
            id,
            c,
            (offset.max(0) as i64) * ROW,
            (length as i64) * ROW,
            false,
        );
    }
}

/// PICO-8 `music(pattern, fade_len, mask)`: `pattern` -1 stops (after a
/// `fade_len` ms fade-out); a positive `fade_len` fades in; `mask` bits reserve
/// channels for music so `sfx()` never borrows them.
pub fn music(pattern: i32, fade_len: i32, mask: i32) {
    unsafe {
        if !(-1..64).contains(&pattern) {
            return;
        }
        if pattern == -1 {
            if fade_len <= 0 {
                set_music_pattern(-1);
            } else {
                MUSIC().fade_step = -((MUSIC().fade_vol as i64 * MUSIC_FADE_Q16_PER_MS_BLOCK
                    / (fade_len as i64 * 65536))
                    .max(1)) as i32;
            }
            return;
        }
        MUSIC().mask = (mask & 0xF) as u8;
        MUSIC().fade_vol = 65536;
        MUSIC().fade_step = 0;
        if fade_len > 0 {
            MUSIC().fade_vol = 0;
            MUSIC().fade_step = (MUSIC_FADE_Q16_PER_MS_BLOCK / fade_len as i64).max(1) as i32;
        }
        set_music_pattern(pattern);
    }
}

/// Set the master volume for music. `eighths` 0..=8.
pub fn set_music_volume(eighths: u16) {
    unsafe { MUSIC_GAIN = eighths.min(8) as i32 }
}
/// Set the master volume for sound effects. `eighths` 0..=8.
pub fn set_sfx_volume(eighths: u16) {
    unsafe { SFX_GAIN = eighths.min(8) as i32 }
}
/// Current music master volume in eighths (0..=8).
pub fn music_volume() -> u16 {
    unsafe { MUSIC_GAIN as u16 }
}
/// Current SFX master volume in eighths (0..=8).
pub fn sfx_volume() -> u16 {
    unsafe { SFX_GAIN as u16 }
}

/// Test harness: silence music on channels whose bit is set (they keep
/// sequencing, so soloing one instrument leaves the rest in time).
pub fn set_music_mute(mask: u8) {
    unsafe { MUSIC_MUTE = mask }
}

// ---------------------------------------------------------------------------
// Host tests: render the games' SFX / music to WAV for comparison with PICO-8
// (`rustc --test --edition 2021 -O -o /tmp/synth_test shared/src/synth.rs &&
// /tmp/synth_test`; then tools/synth_bench.py scores them).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    mod c1 {
        include!("../../games/celeste/src/assets/audio_data.rs");
    }
    mod c2 {
        include!("../../games/celeste2/src/assets/audio_data.rs");
    }

    /// Decode one ADPCM block exactly like the SPU (for a codec round trip).
    fn decode_block(blk: &[u8], s: &mut (i32, i32), out: &mut [i16; BLOCK_SAMPLES]) {
        let (k0, k1) = ADPCM_FILTERS[(blk[0] >> 4) as usize];
        let shift = (blk[0] & 15) as u32;
        for i in 0..BLOCK_SAMPLES {
            let b = blk[2 + i / 2] as i32;
            let n = if i & 1 == 0 { b & 15 } else { b >> 4 };
            let raw = (((n << 28) >> 28) << 12) >> shift;
            let v = (raw + ((s.0 * k0) >> 6) + ((s.1 * k1) >> 6)).clamp(-0x8000, 0x7FFF);
            s.1 = s.0;
            s.0 = v;
            out[i] = v as i16;
        }
    }

    fn write_wav(path: &str, pcm: &[i16]) {
        let mut b = Vec::with_capacity(44 + pcm.len() * 2);
        let data = (pcm.len() * 2) as u32;
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
        b.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data.to_le_bytes());
        for s in pcm {
            b.extend_from_slice(&s.to_le_bytes());
        }
        std::fs::write(path, b).unwrap();
    }

    /// Render `blocks` blocks: (direct PCM, ADPCM round-tripped PCM).
    fn render(blocks: usize) -> (Vec<i16>, Vec<i16>) {
        let mut direct = Vec::new();
        let mut coded = Vec::new();
        let mut st = (0, 0);
        for _ in 0..blocks {
            let mut pcm = [0i16; BLOCK_SAMPLES];
            render_block(&mut pcm);
            direct.extend_from_slice(&pcm);
            let mut blk = [0u8; BLOCK_BYTES];
            encode_block(&pcm, 0, &mut blk);
            let mut dec = [0i16; BLOCK_SAMPLES];
            decode_block(&blk, &mut st, &mut dec);
            coded.extend_from_slice(&dec);
        }
        (direct, coded)
    }

    #[test]
    fn render_games() {
        let out = std::env::var("SYNTH_OUT").unwrap_or_else(|_| "/tmp/synth".into());
        std::fs::create_dir_all(&out).unwrap();
        let games: [(&str, AudioData); 2] = [
            (
                "celeste",
                AudioData {
                    sfx: &c1::SFX_DATA,
                    music: &c1::MUSIC_DATA,
                },
            ),
            (
                "celeste2",
                AudioData {
                    sfx: &c2::SFX_DATA,
                    music: &c2::MUSIC_DATA,
                },
            ),
        ];
        if let Ok(seed) = std::env::var("SYNTH_SEED") {
            unsafe { RNG = seed.parse().unwrap() };
        }
        let secs: f32 = std::env::var("SYNTH_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1.6);
        let blocks = (secs * SAMPLE_RATE as f32 / BLOCK_SAMPLES as f32) as usize;
        for (name, audio) in games {
            for id in 0..64 {
                load(audio);
                play(id);
                let (d, c) = render(blocks);
                write_wav(&format!("{out}/{name}_sfx{id}.wav"), &d);
                write_wav(&format!("{out}/{name}_sfx{id}_adpcm.wav"), &c);
            }
            if let Ok(seq) = std::env::var("SYNTH_SEQ") {
                // Soundtest layout: 18 frames of silence, then each id for 96 frames.
                load(audio);
                let mut all = Vec::new();
                for id in seq.split(',').map(|x| x.parse::<i32>().unwrap()) {
                    play(-1);
                    all.extend(render(18 * 13).0);
                    play(id);
                    all.extend(render(96 * 13).0);
                }
                write_wav(&format!("{out}/{name}_seq.wav"), &all);
            }
            // "game:pattern:secs;game:pattern:secs"
            for spec in std::env::var("SYNTH_MUSIC").unwrap_or_default().split(';') {
                let f: Vec<&str> = spec.split(':').collect();
                if f.len() != 3 || f[0] != name {
                    continue;
                }
                let (pat, s): (i32, f32) = (f[1].parse().unwrap(), f[2].parse().unwrap());
                load(audio);
                music(pat, 0, 0);
                let (d, c) = render((s * SAMPLE_RATE as f32 / BLOCK_SAMPLES as f32) as usize);
                write_wav(&format!("{out}/{name}_music{pat}.wav"), &d);
                write_wav(&format!("{out}/{name}_music{pat}_adpcm.wav"), &c);
            }
        }
    }
}

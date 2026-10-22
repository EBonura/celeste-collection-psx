//! Lock-step comparison of the old (64-bit) and new (32-bit) synthesisers: the
//! PCM of every block and the ADPCM bytes encoded from it must be identical.

use crate::{synth_new as new, synth_old as old};

mod c1 {
    include!("../../../games/celeste/src/assets/audio_data.rs");
}
mod c2 {
    include!("../../../games/celeste2/src/assets/audio_data.rs");
}

const BLOCK: usize = new::BLOCK_SAMPLES;

struct Rng(u32);
impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }
    fn below(&mut self, n: u32) -> u32 {
        self.next() % n
    }
}

fn load(sfx: &'static [[u8; 68]; 64], music: &'static [[u8; 4]; 64]) {
    new::load(new::AudioData { sfx, music });
    old::load(old::AudioData { sfx, music });
}

/// One block from each, asserting they agree; returns the PCM.
fn step(what: &str, n: usize, enc: &mut Option<([u8; 16], [u8; 16])>) {
    let (mut a, mut b) = ([0i16; BLOCK], [0i16; BLOCK]);
    new::render_block(&mut a);
    old::render_block(&mut b);
    assert_eq!(a, b, "{what}: block {n}");
    let (mut ea, mut eb) = ([0u8; 16], [0u8; 16]);
    new::encode_block(&a, 0, &mut ea);
    old::encode_block(&b, 0, &mut eb);
    assert_eq!(ea, eb, "{what}: adpcm block {n}");
    *enc = Some((ea, eb));
}

fn run(what: &str, blocks: usize) {
    let mut enc = None;
    for n in 0..blocks {
        step(what, n, &mut enc);
    }
}

fn games() -> [(
    &'static str,
    &'static [[u8; 68]; 64],
    &'static [[u8; 4]; 64],
); 2] {
    [
        ("celeste", &c1::SFX_DATA, &c1::MUSIC_DATA),
        ("celeste2", &c2::SFX_DATA, &c2::MUSIC_DATA),
    ]
}

#[test]
fn every_sfx_of_both_games_matches_for_ten_seconds() {
    for (name, sfx, music) in games() {
        for id in 0..64 {
            load(sfx, music);
            new::play(id);
            old::play(id);
            run(&format!("{name} sfx {id}"), 10 * 22050 / BLOCK);
        }
    }
}

#[test]
fn every_music_pattern_of_both_games_matches() {
    for (name, sfx, music) in games() {
        for pattern in 0..64 {
            for (fade, mask) in [(0, 0), (300, 0b0101), (0, 0b1111)] {
                load(sfx, music);
                new::music(pattern, fade, mask);
                old::music(pattern, fade, mask);
                run(
                    &format!("{name} music {pattern} fade {fade}"),
                    24 * 22050 / BLOCK,
                );
            }
        }
    }
}

#[test]
fn music_fades_and_volumes_match() {
    for (name, sfx, music) in games() {
        for pattern in [0, 1, 2, 5, 10, 20, 30] {
            for fade_in in [0, 1, 7, 100, 1000, 33_000, i32::MAX] {
                for fade_out in [0, 1, 3, 250, 70_000, i32::MAX] {
                    load(sfx, music);
                    new::music(pattern, fade_in, 0);
                    old::music(pattern, fade_in, 0);
                    run(&format!("{name} in {fade_in}"), 400);
                    new::music(-1, fade_out, 0);
                    old::music(-1, fade_out, 0);
                    new::set_music_volume(5);
                    old::set_music_volume(5);
                    run(&format!("{name} {pattern} out {fade_out}"), 600);
                    new::set_music_volume(8);
                    old::set_music_volume(8);
                }
            }
        }
    }
}

/// A scripted fuzz: random calls into the public API at random block times.
fn fuzz(
    what: &str,
    sfx: &'static [[u8; 68]; 64],
    music: &'static [[u8; 4]; 64],
    seed: u32,
    blocks: usize,
) {
    let mut rng = Rng(seed | 1);
    load(sfx, music);
    new::set_music_mute(0);
    old::set_music_mute(0);
    let mut enc = None;
    for n in 0..blocks {
        if rng.below(40) == 0 {
            match rng.below(9) {
                0..=3 => {
                    let id = rng.below(66) as i32 - 2;
                    let ch = rng.below(6) as i32 - 1;
                    let offset = [0, 0, 1, 7, 16, 31, 32, -3][rng.below(8) as usize];
                    let length = [0, 0, 1, 2, 9, 40, -5][rng.below(7) as usize];
                    new::play_ch(id, ch, offset, length);
                    old::play_ch(id, ch, offset, length);
                }
                4 | 5 => {
                    let pattern = rng.below(66) as i32 - 2;
                    let fade = [0, 0, 1, 50, 500, 40_000][rng.below(6) as usize];
                    let mask = rng.below(20) as i32;
                    new::music(pattern, fade, mask);
                    old::music(pattern, fade, mask);
                }
                6 => {
                    let v = rng.below(10) as u16;
                    new::set_music_volume(v);
                    old::set_music_volume(v);
                    new::set_sfx_volume(8 - v.min(8));
                    old::set_sfx_volume(8 - v.min(8));
                }
                7 => {
                    let m = rng.below(16) as u8;
                    new::set_music_mute(m);
                    old::set_music_mute(m);
                }
                _ => {
                    new::play(-1);
                    old::play(-1);
                }
            }
        }
        step(&format!("{what} seed {seed}"), n, &mut enc);
    }
}

#[test]
fn scripted_calls_on_the_games_match() {
    for (name, sfx, music) in games() {
        for seed in 1..=300u32 {
            fuzz(name, sfx, music, seed.wrapping_mul(2_654_435_761), 3000);
        }
    }
}

/// Sound RAM full of arbitrary bytes: every effect, speed, loop, custom
/// instrument and music flag combination, far beyond what a cart authors.
#[test]
fn arbitrary_sound_ram_matches() {
    let mut rng = Rng(0xc0ff_ee11);
    for round in 0..400u32 {
        let mut sfx = Box::new([[0u8; 68]; 64]);
        let mut music = Box::new([[0u8; 4]; 64]);
        for s in sfx.iter_mut() {
            for b in s.iter_mut() {
                *b = rng.next() as u8;
            }
            // Mostly plausible editor fields so music and loops engage.
            if rng.below(4) != 0 {
                s[65] = 1 + rng.below(40) as u8;
            }
            match rng.below(4) {
                0 => {
                    s[66] = 0;
                    s[67] = 0;
                }
                1 => {
                    s[66] = rng.below(32) as u8;
                    s[67] = (s[66] as u32 + rng.below(32 - s[66] as u32 + 1)) as u8;
                }
                _ => {}
            }
        }
        for m in music.iter_mut() {
            for b in m.iter_mut() {
                *b = rng.next() as u8;
                if rng.below(3) == 0 {
                    *b &= 0x3f | 0x80; // keep more channels active
                }
            }
        }
        let (sfx, music): (&'static _, &'static _) = (Box::leak(sfx), Box::leak(music));
        fuzz("random ram", sfx, music, round * 7919 + 3, 2500);
    }
}

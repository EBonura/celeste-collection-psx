//! The SPU stream's estimated play head: whole blocks plus a Q16 fraction.
//!
//! The estimate used to be one 64-bit Q16 count of blocks. It is two 32-bit
//! words now: the whole block count (the same counter `WRITE` and `MARK` use,
//! which wraps there at 2^32 blocks, about two months of play) and a fraction
//! below 2^16. Everything the stream does with it is a comparison against a
//! whole block (`mark << 16` before), so the fraction only needs adding.

/// Blocks the play head consumes per 60 Hz frame, Q16 (22050 / 60 / 28). The
/// IRQ latch corrects the real ratio; this only extrapolates between polls.
pub const BLOCKS_PER_FRAME_Q16: u32 = 860_160;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PlayHead {
    blocks: u32,
    frac: u32,
}

impl PlayHead {
    pub const ZERO: PlayHead = PlayHead { blocks: 0, frac: 0 };

    /// Add `frames` frames of consumption (at most 8 per call).
    #[inline]
    pub fn advance(&mut self, frames: u32) {
        let frac = self.frac + BLOCKS_PER_FRAME_Q16 * frames;
        self.blocks = self.blocks.wrapping_add(frac >> 16);
        self.frac = frac & 0xffff;
    }

    /// Whole blocks played (`head >> 16`).
    #[inline]
    pub fn blocks(self) -> u32 {
        self.blocks
    }

    /// The head is before the start of block `mark` (`head < mark << 16`).
    #[inline]
    pub fn before(self, mark: u32) -> bool {
        self.blocks < mark
    }

    /// The head at the start of block `mark`.
    #[inline]
    pub fn at(mark: u32) -> PlayHead {
        PlayHead {
            blocks: mark,
            frac: 0,
        }
    }

    /// Half a block before block `mark` (`(mark << 16) - 32768`).
    #[inline]
    pub fn half_before(mark: u32) -> PlayHead {
        PlayHead {
            blocks: mark.wrapping_sub(1),
            frac: 32768,
        }
    }
}

/// Blocks due in `frames` frames, rounded up: `(per_frame * frames + 65535) >> 16`.
#[inline]
pub fn blocks_for_frames(frames: u32) -> u32 {
    (BLOCKS_PER_FRAME_Q16 * frames + 65535) >> 16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_64_bit_estimate() {
        const PER_FRAME: i64 = 860_160;
        let mut state = 0x7f4a_7c15u32;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state
        };
        for _ in 0..2000 {
            let mut head = PlayHead::ZERO;
            let mut wide: i64 = 0;
            let mut mark = 16u32;
            for _ in 0..600 {
                let elapsed = 1 + next() % 8;
                head.advance(elapsed);
                wide += PER_FRAME * elapsed as i64;
                let mark_q16 = (mark as i64) << 16;
                if next() % 3 == 0 {
                    // the IRQ flag is set
                    if wide < mark_q16 {
                        wide = mark_q16;
                    }
                    if head.before(mark) {
                        head = PlayHead::at(mark);
                    }
                    mark = (wide >> 16) as u32 + 16;
                    assert_eq!(mark, head.blocks() + 16);
                } else if wide >= mark_q16 {
                    wide = mark_q16 - 32768;
                    head = PlayHead::half_before(mark);
                }
                assert_eq!(head.blocks(), (wide >> 16) as u32);
                assert_eq!(head.before(mark), wide < (mark as i64) << 16);
            }
        }
        for frames in 0..=15u32 {
            assert_eq!(
                blocks_for_frames(frames) as u64,
                ((PER_FRAME as u64 * frames as u64 + 65535) >> 16)
            );
        }
    }
}

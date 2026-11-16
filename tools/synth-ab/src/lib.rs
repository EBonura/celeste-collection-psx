//! The shipped synthesiser beside the 64-bit one it replaced, for the A/B test.
//!
//! `synth_old.rs` (1,253 lines) is a test oracle and nothing else: only the tests in `ab.rs` use it,
//! it is never linked into a disc, and it stays as long as they prove the 32-bit clock renders the
//! same samples as the 64-bit one (every SFX of both games for ten seconds, every music pattern).

#[path = "../../../shared/src/synth.rs"]
pub mod synth_new;

#[path = "../../../shared/src/playhead.rs"]
pub mod playhead;

#[path = "../../../shared/src/sin_table.rs"]
mod sin_table;

#[path = "../../../shared/src/fixed.rs"]
pub mod fixed;

#[path = "synth_old.rs"]
#[allow(dead_code)]
pub mod synth_old;

#[cfg(test)]
mod ab;

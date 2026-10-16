//! The shipped synthesiser beside the 64-bit one it replaced, for the A/B test.

#[path = "../../../shared/src/synth.rs"]
pub mod synth_new;

#[path = "synth_old.rs"]
#[allow(dead_code)]
pub mod synth_old;

#[cfg(test)]
mod ab;

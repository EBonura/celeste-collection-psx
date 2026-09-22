#!/usr/bin/env python3
"""Exercise the production wait-budget arithmetic, not a replacement synth.

The end-to-end replay separately checks real samples and events. These tests
prove the bounded clock forecast and reproduce the old fourteen-block cap
leaving due work idle before the very next post-swap update.
"""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
source = (ROOT / "shared/src/sfx.rs").read_text()
start = source.index("fn wait_fill_target(")
end = source.index("\n/// Render one due block", start)
constants = "\n".join(
    line for line in source.splitlines()
    if line.startswith(("const LEAD_BLOCKS:", "const MAX_BLOCKS_PER_UPDATE:",
                        "const RING_BLOCKS:", "const BLOCKS_PER_FRAME_Q16:"))
)
unit = constants + "\n" + source[start:end] + r"""
#[test]
fn normal_frame_and_counter_wrap() {
    assert_eq!(wait_fill_target(100, 10, 10), 114);
    assert_eq!(wait_fill_target(100, u32::MAX, 0), 127);
    assert_eq!(wait_fill_target(u32::MAX - 10, 0, 0), u32::MAX);
}
#[test]
fn missed_frame_reproduces_idle_due_work() {
    // Actual failing trace: WRITE=TARGET=28385; next update needs28411.
    let start = 28385;
    let due = 28411;
    assert_eq!(due - (start + 14), 12);
    assert!(wait_fill_target(start, 2137, 2138) >= due);
}
#[test]
fn every_fractional_phase_is_covered_without_new_steady_latency() {
    for frames in 1..=8u64 {
        let allowance = wait_fill_target(100, 10, 10 + frames as u32 - 1) - 100;
        for phase in 0..65536u64 {
            let due = ((phase + BLOCKS_PER_FRAME_Q16 as u64 * frames) >> 16)
                .min(MAX_BLOCKS_PER_UPDATE as u64) as u32;
            assert!(allowance >= due);
            assert!(allowance - due <= 1);
        }
    }
}
#[test]
fn stale_clock_or_pause_cannot_fill_the_ring() {
    for delta in [0, 1, 2, 7, 8, 100, u32::MAX] {
        let allowance = wait_fill_target(100, 10, 10u32.wrapping_add(delta)) - 100;
        assert!(allowance <= MAX_BLOCKS_PER_UPDATE);
        assert!(LEAD_BLOCKS + allowance < RING_BLOCKS);
    }
}
"""
with tempfile.TemporaryDirectory(prefix="celeste-audio-wait-") as directory:
    path = Path(directory)
    (path / "tests.rs").write_text(unit)
    subprocess.run(["rustc", "--edition=2021", "--test", "-O",
                    str(path / "tests.rs"), "-o", str(path / "tests")], check=True)
    subprocess.run([str(path / "tests")], check=True)

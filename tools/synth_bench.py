#!/usr/bin/env python3
"""Score host-rendered synth SFX (shared/src/synth.rs tests -> /tmp/synth) against
the PICO-8 reference recordings, with the same similarity metric as
tools/sfx_bench.py but without an emulator capture in the loop (seconds, not
minutes). Renders are the SFX played from a cold start for a fixed window, like
the soundtest, so long SFX are truncated the same way.

Usage:
  rustc --test --edition 2021 -O -o /tmp/synth_test shared/src/synth.rs && /tmp/synth_test
  python3 tools/synth_bench.py <celeste|celeste2> [--dir /tmp/synth] [--adpcm] [--ids 3,9]
"""
import argparse, os, sys
import numpy as np

sys.path.insert(0, os.path.dirname(__file__))
from sfx_bench import load, ref_paths, logspec, dom_pitch  # noqa: E402


def aligned_sim(a, sa, b, sb, max_lag_s=0.25):
    """log-STFT correlation at the best lag within +/-max_lag_s (no silence
    trimming, so an SFX that opens with a rest is still compared in time)."""
    A, B = logspec(a, sa), logspec(b, sb)
    hop = 256 / 22050.0
    best = float("nan")
    for lag in range(-int(max_lag_s / hop), int(max_lag_s / hop) + 1):
        if lag >= 0:
            x, y = A[:, lag:], B[:, : A.shape[1] - lag]
        else:
            x, y = A[:, : A.shape[1] + lag], B[:, -lag:]
        t = min(x.shape[1], y.shape[1])
        if t < 4:
            continue
        x, y = x[:, :t].ravel(), y[:, :t].ravel()
        if x.std() < 1e-6 or y.std() < 1e-6:
            continue
        c = float(np.corrcoef(x, y)[0, 1])
        if np.isnan(best) or c > best:
            best = c
    return best


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("game", choices=["celeste", "celeste2"])
    ap.add_argument("--dir", default="/tmp/synth")
    ap.add_argument("--adpcm", action="store_true", help="score the ADPCM round-tripped render")
    ap.add_argument("--ids", help="comma-separated subset")
    a = ap.parse_args()
    refs = ref_paths(a.game)
    ids = [int(x) for x in a.ids.split(",")] if a.ids else range(len(refs))
    sims = []
    print(" sfx  refDur  pitchSYN  pitchREF    sim")
    for n in ids:
        ref, sr_ref = load(refs[n])
        suffix = "_adpcm" if a.adpcm else ""
        syn, sr_syn = load(f"{a.dir}/{a.game}_sfx{n}{suffix}.wav")
        win = min(len(ref), int(sr_ref * len(syn) / sr_syn))
        ref = ref[:win]
        sim = aligned_sim(syn, sr_syn, ref, sr_ref)
        sims.append(sim)
        flag = "  <--" if not np.isnan(sim) and sim < 0.6 else ""
        print(f"{n:4d}  {len(ref) / sr_ref:5.2f}s  {dom_pitch(syn, sr_syn):7.0f}Hz {dom_pitch(ref, sr_ref):7.0f}Hz   {sim:.2f}{flag}")
    s = np.array([x for x in sims if not np.isnan(x)])
    print(f"\nSUMMARY {a.game}: mean {s.mean():.3f}, median {np.median(s):.3f}; >=0.8: {(s >= 0.8).sum()}/{len(s)}")


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Cart-vs-port frame comparison: one scene, PICO-8 frame (tools/p8frame.py)
against the PSX frame (frametest of the `scene` binary, follow-pan off), as a
side-by-side PNG (PICO-8 | PSX | diff) plus a mismatch percentage.

The PSX draws the 128x128 field at 2x centred in 320x240: x 32..288, and (with
the follow-pan off) PICO-8 rows 4..124. Both are compared at native 128x120.

Usage:
  tools/scene_cmp.py pico.png psx.ppm out.png [--ignore-bg]
"""
import sys
import numpy as np
from PIL import Image


def main():
    pico = np.array(Image.open(sys.argv[1]).convert("RGB"))[4:124]
    psx_full = np.array(Image.open(sys.argv[2]).convert("RGB"))
    psx = psx_full[0:240, 32:288][::2, ::2]
    # PSX colours are 15-bit (5 bits/channel) and PICO-8's are 8-bit: compare with a
    # tolerance that swallows the 15-bit rounding (and the PSX "0x0421" opaque black).
    diff = np.abs(pico.astype(int) - psx.astype(int)).max(axis=2) > 12
    pct = 100.0 * diff.mean()
    h, w = diff.shape
    out = Image.new("RGB", (w * 3 * 2, h * 2))
    out.paste(Image.fromarray(pico).resize((w * 2, h * 2), Image.NEAREST), (0, 0))
    out.paste(Image.fromarray(psx).resize((w * 2, h * 2), Image.NEAREST), (w * 2, 0))
    d = np.zeros_like(pico)
    d[diff] = (255, 0, 255)
    d[~diff] = (pico[~diff] // 3)
    out.paste(Image.fromarray(d).resize((w * 2, h * 2), Image.NEAREST), (w * 4, 0))
    out.save(sys.argv[3])
    print(f"{sys.argv[3]}: {pct:.1f}% pixels differ")


if __name__ == "__main__":
    main()

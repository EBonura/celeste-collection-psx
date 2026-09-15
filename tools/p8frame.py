#!/usr/bin/env python3
"""Grab a real PICO-8 frame of a cart at a scripted game state, for pixel
comparison with the PSX port (tools/scene_cmp.py).

Injects a Lua `_init` (your scene setup) plus an `_update` hook that calls
extcmd("screen") after FRAME 30fps frames and shuts PICO-8 down, then runs PICO-8
with `-desktop` pointed at a scratch dir so the screenshot lands there. Works with
the window hidden (extcmd reads PICO-8's own framebuffer). Celeste 2 sits at the
8192-token ceiling, so its dead title-screen draw is stripped to make room.

Usage:
  tools/p8frame.py <cart.p8> "<lua for _init body>" <frame> out.png
e.g.
  tools/p8frame.py /tmp/celeste2.p8 "game_start() goto_level(3) level_intro=0" 90 l3.png
  tools/p8frame.py /tmp/celeste.p8 "title_screen() begin_game() load_room(1,0)" 120 r10.png
The output is the native 128x128 frame.
"""
import os, subprocess, sys, time
from PIL import Image

PICO8_APP = "/Users/ebonura/Desktop/pico-8/PICO-8.app"
OUT = "/tmp/p8out"


def frame(cart, init_lua, nframes, out):
    os.makedirs(OUT, exist_ok=True)
    data = open(cart, encoding="latin-1").read()
    i = data.find("sspr(64, 32")
    j = data.find("draw_snow()", i)
    if i != -1 and j != -1 and "goto_level" in data:  # celeste2: free ~50 tokens
        data = data[:i] + data[j + len("draw_snow()"):]
    inj = (
        f"\nfunction _init() {init_lua} end\n__ou=_update __dt=0\n"
        f'function _update() __dt+=1 __ou() if __dt=={nframes} then extcmd("screen") end'
        f' if __dt=={nframes + 10} then extcmd("shutdown") end end\n'
    )
    dbg = os.path.join(OUT, "dbg.p8")
    open(dbg, "w", encoding="latin-1").write(data.replace("\n__gfx__", inj + "__gfx__", 1))
    shot = os.path.join(OUT, "dbg_0.png")
    if os.path.exists(shot):
        os.remove(shot)
    subprocess.run(["pkill", "-f", "MacOS/pico8"], capture_output=True)
    time.sleep(0.3)
    subprocess.run(["open", "-a", PICO8_APP, "--args", "-run", dbg, "-desktop", OUT, "-windowed", "1"])
    for _ in range(80):
        if os.path.exists(shot):
            break
        time.sleep(0.25)
    time.sleep(0.5)
    subprocess.run(["pkill", "-f", "MacOS/pico8"], capture_output=True)
    if not os.path.exists(shot):
        raise SystemExit("no screenshot (cart too large / PICO-8 failed?)")
    im = Image.open(shot).convert("RGB")
    im.resize((128, 128), Image.NEAREST).save(out)
    return out


if __name__ == "__main__":
    print(frame(sys.argv[1], sys.argv[2], int(sys.argv[3]), sys.argv[4]))

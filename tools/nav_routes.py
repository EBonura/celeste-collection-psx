#!/usr/bin/env python3
"""Headless navigation routes for the Celeste Classic Collection disc.

Every way around the launcher and both games' pause menus is one route: a
scripted pad tape (one sample per emulator route tick, so it can hold the
analog stick as well as buttons) replayed by PSoXide's headless `frontend
launch`, with screenshots at the checkpoints. Each checkpoint names the screen
it expects (menu, credits, settings, pause or game) and, on the cover menu,
which game is highlighted; on the settings screen, which row.

    python3 tools/nav_routes.py --frontend PATH/TO/frontend \\
        --disc dist/celeste-collection.cue [--out DIR] [--only NAME ...]

Exits non-zero if any checkpoint fails. Screens are recognised from fixed UI
pixels (the menu's gold hint labels, the credits' navy fill, the settings
highlight bar, the pause panel's white border), so a checkpoint must sit where
the screen has settled, not mid-fade.
"""

import argparse
import concurrent.futures
import os
import shutil
import struct
import subprocess
import sys
from collections import Counter

from PIL import Image

# ---- routes ------------------------------------------------------------------
# Event: (tick, inputs, hold). inputs is '+'-joined buttons and/or stick
# directions (stick_left/right/up/down push the left stick fully that way).
# Check: (tick, screen, detail) where detail is the highlighted game on the menu
# (0 Celeste, 1 Celeste 2), the highlighted row on the settings screen, or None.

LAUNCH_C1 = [(60, "cross", 8), (150, "cross", 8)]
LAUNCH_C2 = [(60, "cross", 8), (150, "right", 8), (200, "cross", 8)]

ROUTES = {
    # boot and the cover menu
    "intro_skip_cross": ([(60, "cross", 8)], [(150, "menu", 0)]),
    "intro_skip_circle": ([(60, "circle", 8)], [(150, "menu", 0)]),
    "intro_skip_start": ([(60, "start", 8)], [(150, "menu", 0), (250, "menu", 0)]),
    "menu_dpad": (
        [(60, "cross", 8), (150, "right", 8), (250, "left", 8)],
        [(200, "menu", 1), (300, "menu", 0)],
    ),
    "menu_stick": (
        [(60, "cross", 8), (150, "stick_right", 8), (250, "stick_left", 8)],
        [(200, "menu", 1), (300, "menu", 0)],
    ),
    "menu_circle_does_nothing": (
        [(60, "cross", 8), (150, "right", 8), (200, "circle", 8)],
        [(300, "menu", 1)],
    ),
    # credits: Select opens, Cross / Circle / Start close, selection kept
    "credits_cross": (
        [(60, "cross", 8), (150, "right", 8), (200, "select", 8), (400, "cross", 8)],
        [(300, "credits", None), (500, "menu", 1)],
    ),
    "credits_circle": (
        [(60, "cross", 8), (150, "right", 8), (200, "select", 8), (400, "circle", 8)],
        [(300, "credits", None), (500, "menu", 1)],
    ),
    "credits_start": (
        [(60, "cross", 8), (150, "right", 8), (200, "select", 8), (400, "start", 8)],
        [(300, "credits", None), (500, "menu", 1)],
    ),
    # settings: Start opens, Circle / Start close, stick moves the row
    "settings_circle": (
        [(60, "cross", 8), (150, "right", 8), (200, "start", 8), (300, "circle", 8)],
        [(260, "settings", 0), (400, "menu", 1)],
    ),
    "settings_start": (
        [(60, "cross", 8), (150, "right", 8), (200, "start", 8), (300, "start", 8)],
        [(260, "settings", 0), (400, "menu", 1)],
    ),
    "settings_stick": (
        [(60, "cross", 8), (200, "start", 8), (260, "stick_down", 8), (300, "stick_down", 8),
         (340, "stick_up", 8)],
        [(250, "settings", 0), (290, "settings", 1), (330, "settings", 2), (370, "settings", 1)],
    ),
    # leaving a game: pause "Quit to Menu", Select+Start, and back in again
    "c1_pause_quit": (
        LAUNCH_C1 + [(500, "start", 8), (560, "up", 8), (620, "cross", 8), (800, "cross", 8)],
        [(400, "game", None), (540, "pause", None), (720, "menu", 0), (1000, "game", None)],
    ),
    "c2_pause_quit": (
        LAUNCH_C2 + [(550, "start", 8), (610, "up", 8), (670, "cross", 8), (850, "cross", 8)],
        [(450, "game", None), (590, "pause", None), (770, "menu", 1), (1050, "game", None)],
    ),
    "c2_pause_quit_stick": (
        LAUNCH_C2 + [(550, "start", 8), (610, "stick_up", 8), (670, "cross", 8)],
        [(590, "pause", None), (770, "menu", 1)],
    ),
    "c1_select_start": (
        LAUNCH_C1 + [(500, "select", 30), (504, "start", 26)],
        [(400, "game", None), (640, "menu", 0)],
    ),
    "c2_select_start": (
        LAUNCH_C2 + [(550, "select", 30), (554, "start", 26)],
        [(450, "game", None), (690, "menu", 1)],
    ),
    "c1_start_then_select": (
        LAUNCH_C1 + [(500, "start", 30), (504, "select", 26)],
        [(400, "game", None), (640, "menu", 0)],
    ),
    "c2_pause_select_start": (
        LAUNCH_C2 + [(550, "start", 8), (620, "select", 30), (626, "start", 20)],
        [(600, "pause", None), (760, "menu", 1)],
    ),
    "c1_pause_start_resume": (
        LAUNCH_C1 + [(500, "start", 8), (600, "start", 8)],
        [(560, "pause", None), (680, "game", None)],
    ),
    "c2_pause_circle_resume": (
        LAUNCH_C2 + [(550, "start", 8), (650, "circle", 8)],
        [(610, "pause", None), (730, "game", None)],
    ),
    # the reported path: back from a game, then the credits and out again
    "c1_quit_then_credits": (
        LAUNCH_C1 + [(500, "start", 8), (560, "up", 8), (620, "cross", 8), (800, "select", 8),
                     (1000, "cross", 8)],
        [(720, "menu", 0), (900, "credits", None), (1100, "menu", 0)],
    ),
    "c2_quit_then_credits": (
        LAUNCH_C2 + [(550, "select", 30), (554, "start", 26), (800, "select", 8),
                     (1000, "circle", 8)],
        [(720, "menu", 1), (900, "credits", None), (1100, "menu", 1)],
    ),
}

# ---- tape --------------------------------------------------------------------
BUTTON = {
    "select": 0x0001, "l3": 0x0002, "r3": 0x0004, "start": 0x0008,
    "up": 0x0010, "right": 0x0020, "down": 0x0040, "left": 0x0080,
    "l2": 0x0100, "r2": 0x0200, "l1": 0x0400, "r1": 0x0800,
    "triangle": 0x1000, "circle": 0x2000, "cross": 0x4000, "square": 0x8000,
}
STICK = {"stick_left": (0x00, None), "stick_right": (0xFF, None),
         "stick_up": (None, 0x00), "stick_down": (None, 0xFF)}


def write_tape(path, events, length):
    """PXITAPE1: magic, u32 count, then (u16 buttons, rx, ry, lx, ly) per tick."""
    samples = [[0, 0x80, 0x80, 0x80, 0x80] for _ in range(length)]
    for tick, inputs, hold in events:
        for name in inputs.split("+"):
            for t in range(tick, min(tick + hold, length)):
                if name in BUTTON:
                    samples[t][0] |= BUTTON[name]
                else:
                    lx, ly = STICK[name]
                    if lx is not None:
                        samples[t][3] = lx
                    if ly is not None:
                        samples[t][4] = ly
    with open(path, "wb") as f:
        f.write(b"PXITAPE1" + struct.pack("<I", length))
        for s in samples:
            f.write(struct.pack("<HBBBB", *s))


# ---- screen recognition ------------------------------------------------------
MENU_HINT_GOLD = (222, 206, 115)  # the "Menu" / "Credits" hint labels
CREDITS_NAVY = (8, 8, 24)
SETTINGS_BAR = (16, 16, 49)  # the highlighted settings row
PAUSE_WHITE = (255, 247, 239)  # the pause panel's border


def classify(path):
    im = Image.open(path).convert("RGB")
    if Counter(im.crop((156, 202, 200, 226)).getdata())[MENU_HINT_GOLD] >= 150:
        lum = [sum(map(sum, im.crop(box).getdata()))
               for box in ((52, 68, 132, 148), (188, 68, 268, 148))]
        return "menu", 0 if lum[0] > lum[1] else 1
    if Counter(im.getdata())[CREDITS_NAVY] >= 320 * 240 * 3 // 10:
        return "credits", None
    bar = [y for y in range(60, 210)
           if sum(im.getpixel((x, y)) == SETTINGS_BAR for x in range(60, 260, 4)) > 20]
    if len(bar) >= 8:
        return "settings", (bar[0] + 3 - 74) // 22
    border = sum(im.getpixel((56, y)) == PAUSE_WHITE and im.getpixel((262, y)) == PAUSE_WHITE
                 for y in range(60, 180))
    if border >= 100:
        return "pause", None
    return "game", None


# ---- runner ------------------------------------------------------------------
STEPS_PER_TICK = 300_000  # measured ~240k retired instructions per route tick


def run_route(name, frontend, disc, out):
    events, checks = ROUTES[name]
    last = max(t for t, _, _ in checks)
    d = os.path.join(out, name)
    shots = os.path.join(d, "shots")
    shutil.rmtree(d, ignore_errors=True)
    os.makedirs(shots)
    tape = os.path.join(d, "route.pxtape")
    write_tape(tape, events, last + 10)
    cmd = [frontend, "launch", "--path", disc, "--steps", str((last + 30) * STEPS_PER_TICK),
           "--input-tape", tape, "--route-screenshot-dir", shots,
           "--route-screenshot-interval", "10"]
    with open(os.path.join(d, "run.log"), "w") as log:
        rc = subprocess.run(cmd, stdout=log, stderr=subprocess.STDOUT).returncode
    results = []
    for tick, want, detail in checks:
        shot = os.path.join(shots, f"tick-{tick:06d}.ppm")
        got = classify(shot) if os.path.exists(shot) else ("missing", None)
        ok = rc == 0 and got[0] == want and (detail is None or got[1] == detail)
        results.append((tick, want, detail, got, ok))
        if os.path.exists(shot):
            Image.open(shot).save(os.path.join(d, f"check-{tick:06d}.png"))
    shutil.rmtree(shots)  # keep only the checkpoint frames
    return name, rc, results


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--frontend", required=True, help="PSoXide frontend binary")
    ap.add_argument("--disc", default="dist/celeste-collection.cue")
    ap.add_argument("--out", default="dist/nav-routes")
    ap.add_argument("--only", nargs="*", help="route names to run (default: all)")
    ap.add_argument("--jobs", type=int, default=4)
    a = ap.parse_args()
    names = a.only or list(ROUTES)
    unknown = [n for n in names if n not in ROUTES]
    if unknown:
        sys.exit(f"unknown route(s): {', '.join(unknown)}")
    disc = os.path.abspath(a.disc)
    failed = 0
    with concurrent.futures.ThreadPoolExecutor(a.jobs) as pool:
        jobs = [pool.submit(run_route, n, a.frontend, disc, a.out) for n in names]
        for job in jobs:
            name, rc, results = job.result()
            bad = rc != 0 or not all(r[-1] for r in results)
            failed += bad
            print(f"{'FAIL' if bad else 'pass'}  {name}" + (f"  (frontend exit {rc})" if rc else ""))
            for tick, want, detail, got, ok in results:
                exp = want if detail is None else f"{want}[{detail}]"
                seen = got[0] if got[1] is None else f"{got[0]}[{got[1]}]"
                print(f"      tick {tick:5d}  want {exp:<12} got {seen:<12} {'ok' if ok else 'MISMATCH'}")
    print(f"{len(names) - failed}/{len(names)} routes passed")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()

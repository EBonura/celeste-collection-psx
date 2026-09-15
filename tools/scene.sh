#!/usr/bin/env bash
# One cart-vs-port scene comparison: build the game's `scene` bin for SCENE, pack,
# capture the PSX frame after PSXFRAMES frames, grab the PICO-8 frame at the same
# game time (PSXFRAMES/2, 30fps), and write the side-by-side to OUT.
#   tools/scene.sh <celeste|celeste2> <scene> <psxframes> <cart.p8> <out.png>
# scene: celeste2 = level number; celeste = "x,y" room.
set -euo pipefail
GAME=$1; SCENE=$2; PSXFRAMES=$3; CART=$4; OUT=$5
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
export PSX_BIOS="${PSX_BIOS:-/Users/ebonura/Downloads/ps1 bios/SCPH1001.BIN}"
( cd games/$GAME && SCENE="$SCENE" cargo build --release --bin scene >/dev/null 2>&1 )
( cd .psoxide/tools/mkisopsx && cargo run -q --release -- --exe "$ROOT/games/$GAME/target/mipsel-sony-psx/release/scene.exe" --out "$ROOT/dist/${GAME}_scene.bin" --volume PICO8PSX >/dev/null 2>&1 )
# +27: frames the PSX spends booting before the game loop draws (measured)
tools/psx-audio-capture/target/release/frametest --disc "dist/${GAME}_scene.cue" --out /tmp/p8out/scene_psx.ppm --frames "$((PSXFRAMES + 27))" >/dev/null 2>&1
if [ "$GAME" = celeste2 ]; then LUA="game_start() goto_level($SCENE) level_intro=0"; else LUA="title_screen() begin_game() load_room($SCENE)"; fi
python3 tools/p8frame.py "$CART" "$LUA" $((PSXFRAMES / 2)) /tmp/p8out/scene_pico.png >/dev/null
python3 tools/scene_cmp.py /tmp/p8out/scene_pico.png /tmp/p8out/scene_psx.ppm "$OUT"

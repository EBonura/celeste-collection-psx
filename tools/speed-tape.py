# Author a speed window's input tape: a video-frame tape (PXITAPE1) the frontend transcribes onto the
# guest's own pad polls (committed under pgo/):
#   python3 tools/speed-tape.py frames.pxtape            Celeste Classic: splash skip, menu launch,
#                                                        title start, a room walk with jumps and dashes
#   python3 tools/speed-tape.py frames.pxtape lani       Lani's Trek: splash skip, Lani picked and
#                                                        launched, title start, a level-1 walk with
#                                                        jumps and grapples (pgo/lani.pxtape)
#   frontend launch --path build/ship/celeste-collection.cue --input-tape frames.pxtape \
#     --input-tape-transcribe pgo/classic.pxtape --steps 40000000000
# The scene split loads each scene between the menu and the cart: no pad poll runs during a load,
# so the transcribed tape's poll indices do not count it.
import struct, sys
CROSS, CIRCLE, RIGHT, UP, DOWN, LEFT = 1<<14, 1<<13, 1<<5, 1<<4, 1<<6, 1<<7
frames = []
def idle(n): frames.extend([0]*n)
def press(mask, hold): frames.extend([mask]*hold)
if len(sys.argv) > 2 and sys.argv[2] == "lani":
    # Frame timings fit the scene disc: a switch takes up to about 100 frames before the next
    # screen polls the pad.
    idle(60); press(CIRCLE, 8); idle(160); press(RIGHT, 8); idle(30); press(CROSS, 8)
    idle(400); press(CROSS, 8)
    idle(400)  # title to level 1, intro settled
    for _ in range(40):
        press(RIGHT, 20); press(RIGHT|CROSS, 14); idle(4)
        press(RIGHT|UP|CIRCLE, 8); press(RIGHT, 16); press(LEFT|CROSS, 10); idle(6)
else:
    idle(60); press(CROSS, 8); idle(100); press(CROSS, 8); idle(230); press(CROSS, 8)
    idle(700)  # title to room 0, entry settled
    for _ in range(40):
        press(RIGHT, 20); press(RIGHT|CROSS, 14); idle(4)
        press(RIGHT|UP|CIRCLE, 8); press(RIGHT, 16); press(LEFT|CROSS, 10); idle(6)
out = b"PXITAPE1" + struct.pack("<I", len(frames))
for b in frames: out += struct.pack("<H4B", b, 0x80, 0x80, 0x80, 0x80)
open(sys.argv[1], "wb").write(out)
print(len(frames))

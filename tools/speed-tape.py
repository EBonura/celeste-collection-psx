# Author the speed window's input tape for Celeste Classic: a video-frame tape (PXITAPE1) of the splash skip,
# the menu launch, the title start and a room walk with jumps and dashes. The frontend transcribes it onto the
# guest's own pad polls (pgo/classic.pxtape, committed):
#   python3 tools/speed-tape.py frames.pxtape
#   frontend launch --path build/ship/celeste-collection.cue --input-tape frames.pxtape \
#     --input-tape-transcribe pgo/classic.pxtape --steps 40000000000
import struct, sys
CROSS, CIRCLE, RIGHT, UP, DOWN, LEFT = 1<<14, 1<<13, 1<<5, 1<<4, 1<<6, 1<<7
frames = []
def idle(n): frames.extend([0]*n)
def press(mask, hold): frames.extend([mask]*hold)
idle(60); press(CROSS, 8); idle(100); press(CROSS, 8); idle(230); press(CROSS, 8)
idle(700)  # title to room 0, entry settled
for _ in range(40):
    press(RIGHT, 20); press(RIGHT|CROSS, 14); idle(4)
    press(RIGHT|UP|CIRCLE, 8); press(RIGHT, 16); press(LEFT|CROSS, 10); idle(6)
out = b"PXITAPE1" + struct.pack("<I", len(frames))
for b in frames: out += struct.pack("<H4B", b, 0x80, 0x80, 0x80, 0x80)
open(sys.argv[1], "wb").write(out)
print(len(frames))

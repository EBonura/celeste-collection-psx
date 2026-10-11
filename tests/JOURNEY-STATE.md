# Campaign state version 1

`CELESTE_JOURNEY` and `LANI_JOURNEY` are read-only observations exported by
the shipping collection. They cannot change a room, a player or a score.
The normal collection build emits `dist/celeste-collection.map`. For a direct
Cargo build, set `PSX_LINK_MAP` to an absolute output path and give that exact
map to the journey's `symbols` field. The map and disc must come from the
same build. Do not copy an address from another artifact.

All fields are signed 32-bit words, with the following byte offsets:

| Offset | Field | Meaning |
| --- | --- | --- |
| 0 | version | 1 |
| 4 | cart | 1 Classic, 2 Lani |
| 8 | updates | Logic update calls, including frozen updates |
| 12 | input_latches | Game input submissions, not physical pad polls |
| 16 | input_mask | Last PICO-8 game input mask |
| 20 | segment | Classic room index 0 through 30, title 31; Lani level 1 through 8, title 0 |
| 24 | player_present | Active player exists |
| 28 | player_x | 16.16 position, valid when player_present is 1 |
| 32 | player_y | 16.16 position, valid when player_present is 1 |
| 36 | velocity_x | 16.16 velocity, valid when player_present is 1 |
| 40 | velocity_y | 16.16 velocity, valid when player_present is 1 |
| 44 | player_state | Classic dash timer; Lani state; -1 without a player |
| 48 | deaths | Run death counter |
| 52 | berries | Classic collected count; Lani deposited count |
| 56 | berry_bits | Classic GOT_FRUIT bits 0 through 29; 0 for Lani |
| 60 | checkpoint | Lani checkpoint object ID, -1 without one; -1 for Classic |
| 64 | ability | Classic maximum air dashes; Lani grapple ownership |
| 68 | ending | Classic flag score shown, 0 or 1; Lani SHOW_SCORE, initialized to 0 and positive after final reveal |
| 72 | final_score | Classic flag score; Lani deposited count after reveal; -1 before reveal |
| 76 | debug_fly | Must be 0 in completion journeys |
| 80 | published | 0 while publishing, 1 only after every payload word is written |
| 84 | sequence | Publication count, incremented once per observation |

The layout is 88 bytes and compile-time checks enforce every offset.
Observations are built locally and published with a volatile payload write,
then a final volatile `published = 1` marker. The writer first clears that
marker. A checkpoint must require `published == 1` before accepting any
state field, and `sequence > 0`. This rejects an instruction-boundary capture
inside publication. The emulator is paused while assertions read the fields;
a concurrent reader must also read the sequence before and after the payload
and require unchanged sequence and both published-marker reads equal to 1.
Cart initialization also clears the marker with a volatile write before a
volatile aggregate reset. A repeated launch cannot expose the previous run's
completed marker over a partially reset payload.

The loop counts updates after the existing logic call and publishes observations
after the existing draw call. Classic also publishes on frozen frames. This
preserves the existing order of input, logic, drawing, audio and presentation.
Classic's flag sets `show` during drawing, so a summit arrival is insufficient:
the ending checkpoint must assert segment 30 and ending 1.

The state adds no gameplay UI and has no setter or debug completion path.
Input generation on a host simulator is not a completion receipt. A normal
pad replay of the bound guest artifact must prove each room or level exit.

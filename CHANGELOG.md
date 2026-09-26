# Changelog

## 0.2.5 | 2026-09-26

Not yet published.

- Launcher navigation follows one rule set everywhere: Cross confirms or
  advances, Circle or Start goes back, and a press counts only on the frame
  its button goes down. Leaving a game (pause menu or Select+Start), the
  credits or the settings lands on the cover menu with the last game still
  selected; it used to reset to Celeste.
- `pico8::input::poll_buttons` repeats the last clean pad state when a poll's
  ID handshake stays garbled through the SDK's retries. The garbled bytes used
  to reach the launcher as buttons: the menu tests Select before Cross and
  Start, so a garbled read could open the credits, and the credits' exit test
  (any of Cross, Circle or Start newly down) never fires while garbage reads
  as one of them held. The credits now edge-detect each button on its own.
- The pause menu resumes on Circle (on its release, so Celeste's dash and
  Celeste 2's grapple do not see a held Circle) and quits on Select+Start,
  which it used to ignore when Start arrived first and opened it.
- `tools/nav_routes.py` (`make nav-routes FRONTEND=...`) replays every
  navigation path headlessly and checks the screen and selection at each
  step.

## 0.2.3 | 2026-09-15

Download published on itch.io.

- Celeste 2's fog levels hold 60 fps. The cloud discs, about 84 a frame,
  went through a generic loop that spilled to the stack on every rectangle
  and zeroed a scratch table per disc; the listed path is now a lean
  pointer walk of a per-radius run table with the cursor in registers. A
  recorded 50 second session replays in 2125 VBlanks where 0.2.2 as first
  published took 2891, every level at 60.

## 0.2.2 | 2026-09-15

Download published on itch.io.

- Gameplay held at 60 fps again. The streamed audio is rendered per real
  second, so one late frame doubled the next frame's rendering and kept it
  late: in PSoXide both games settled into a steady 30 fps once in a level.
  The audio blocks are now rendered while the game is already waiting for
  VBlank, so they no longer count against the frame; the synth also steps
  its pitch ramps without per-sample 64-bit arithmetic.
- Less CPU per frame all round: the ADPCM encoder re-picks its predictor
  every fourth block and shares the filter products in its search (about a
  third of what it cost, for 1 dB of encoder SNR), the oscillator loops are
  specialised per waveform, and PICO-8 circle fills (the Celeste 2 clouds)
  merge rows of equal width into flat rectangles, the cheapest GPU
  primitive, instead of one two-triangle quad per row.
- The GPU no longer stalls the CPU. Each frame is built as a GPU display
  list and handed to DMA in chunks as it fills, and the game runs its next
  update (and the audio) while the GPU draws the previous frame. Writing
  primitives straight to GP0 cost a fifth of every frame in Celeste 2's
  tiled and foggy levels, which ran at 30 fps; a recorded 50 second play
  session now replays in 25% fewer VBlanks, all at 60 fps except the
  heaviest fog level, which holds 55-60 instead of 30.
- The synth's state and buffers live in the PS1's scratchpad (the machine
  has no data cache; every main-RAM access stalls), and disc fills read a
  small table of merged rows instead of recomputing them per circle.

## 0.2.1 | 2026-09-15

Download published on itch.io.

- Analog controller support: on a DualShock in analog mode the left stick
  works as the d-pad in both games, the launcher and the pause menu (digital
  mode is unchanged). frametest gained `--stick LX,LY [--stick-from N]` to
  drive the emulated DualShock's stick.

## 0.2.0 | 2026-09-15

Download published on itch.io.

- Audio engine replaced: the PICO-8 synthesiser (oscillators, effects, custom
  instruments, sequencer) runs in software on the PS1 and streams to the SPU
  through one voice, with the play position fed back from the SPU. Levels,
  noise and the custom-instrument rules were calibrated against recordings of
  the real carts; the SFX and music benches sit at a 0.95+ median similarity
  where the wavetable path topped out around 0.6 to 0.8.
- PICO-8's four-channel allocation is honoured (sound effects borrow music
  channels, as in the carts) and Celeste's music fades work.
- Celeste 2 spawns every object in a level (the 48-slot cap silently dropped
  later spikes, grapplers, checkpoints and crumble blocks in levels 3 to 7);
  its per-level palette swaps render; clouds are the cart's clipped
  half-discs; dithered pillars, fog and crumble cracks match the cart's
  patterns; the column levels run at 60 fps instead of 30.
- Verified with a new cart-vs-disc frame comparison of every Celeste 2 level
  and every Celeste room against PICO-8 (tools/scene.sh).
- Built on the split PSoXide SDK (08a55f36); the host bench tools use the
  standalone emulator and no longer need a firmware image.


## Source 2026.09.05

This source snapshot is tagged `source-2026.09.05`. Download versions are
listed separately below; source cleanup does not replace an already published disc.

- CI hydrates the pinned SDK before checking the host build.
- Guest builds disable unsafe MIPS delay-slot scheduling.
- Audio probes use explicit vblank waits; refreshed build and console instructions.

## split.20260905 | 2026-09-05

Standalone collection published on itch.io.

- Published Celeste Classic and Celeste 2 together with the collection launcher.

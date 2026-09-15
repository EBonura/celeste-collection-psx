# Changelog

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

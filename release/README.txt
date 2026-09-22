Celeste Classic Collection (PSX)  -  v0.2.4
===========================================

Two PICO-8 Celeste Classic games, the original Celeste and Celeste 2: Lani's
Trek, ported natively to the PlayStation 1, on one bootable disc.

To run:
  - Open celeste-collection.cue in a PS1 emulator, or
  - burn the CUE/BIN pair to a CD-R for a compatible PlayStation console.

In the launcher: D-pad to choose, Cross to play, Select for credits. On a
DualShock in analog mode the left stick works as the D-pad everywhere.
Press Select+Start together to return to the launcher.

v0.2.4:
  - Classic avoids copying unused object slots when effects disappear.
  - Celeste 2 reuses ordered grapple candidates within each throw step.
  - Classic prepares its initial audio before the first visible update.
  - Audio synthesis uses otherwise idle VBlank time to prepare due samples.
  - Shared scaled-pixel drawing avoids generic rectangle setup.
  - Updated shared SDK display-list handling and renderer optimizations.

The tested gameplay route preserved matched checkpoint images and game
state. In the measured emulator route, Classic had no unexplained missed
presentation deadlines; Celeste 2 still had occasional misses. This release
does not claim locked 60 fps in every scene. The latest build was also
reported working on original PlayStation hardware.

Bonnie Studios  -  https://bonnie-studios.itch.io

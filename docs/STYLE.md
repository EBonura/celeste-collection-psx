# Rust style

The Celeste collection follows [Google's Rust style guide](https://google.github.io/styleguide/rustguide.html).
The canonical statement, rule 0 and the approved project deltas live in PSoXide's
[sdk/docs/STYLE.md](https://github.com/EBonura/PSoXide/blob/main/sdk/docs/STYLE.md); this
file only records how this repository applies them.

Rule 0: games run well first. Frame time, RAM, VRAM, binary size and load time outrank
style. Formatting and other mechanical changes must leave the guest binary byte-identical; a
change that can alter codegen needs a measurement showing no cost.

- `rustfmt.toml` holds the guide's settings (three options are unstable, hence the pinned
  nightly). `tools/style-fmt.sh` formats every tracked `.rs` file, `--check` only
  reports; `make fmt` and `make fmt-check` call it, and the Style workflow runs it on
  every push.
- Hardware addresses and opcodes are written in lower-case hex, as rustfmt prints them.

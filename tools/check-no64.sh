#!/bin/sh
# Fail when guest source names a 64/128-bit integer or a float type (see
# tools/no64_guard.rs for the rules and the allow marker). Run from anywhere.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
out="${TMPDIR:-/tmp}/no64_guard.$$"
trap 'rm -f "$out"' EXIT
rustc -O --edition 2021 "$root/tools/no64_guard.rs" -o "$out"
"$out" "$root/shared/src" "$root/games/celeste/src" "$root/games/celeste2/src" \
    "$root/games/celeste-collection/src"

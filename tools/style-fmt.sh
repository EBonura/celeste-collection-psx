#!/bin/sh
# Format (or with --check, verify) every tracked Rust file with the guide's rustfmt settings.
# Uses the repo's pinned nightly because three of the options are unstable.
set -eu
cd "$(dirname "$0")/.."
git ls-files -z '*.rs' | xargs -0 rustfmt --edition 2021 ${1:+"$1"}

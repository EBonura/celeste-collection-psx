#!/usr/bin/env python3
"""Test the actual no-std backend geometry without building console test support.

Run from the repository root: python3 tools/test_disc_visibility.py
The production predicate and existing circle-run walker are read directly from
backend.rs. No alternate production implementation is maintained by this test.
"""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
source = (ROOT / "shared/src/backend.rs").read_text()
predicate = source[source.index("fn disc_outside_view("):]
predicate = predicate[:predicate.index("/// The packed merged runs")]
walker = source[source.index("enum DiscRuns {"):]
walker = walker[:walker.index("/// PICO-8 `circ(x,y,r,c)`")]
tests = (ROOT / "shared/src/backend_visibility_tests.rs").read_text()
unit = "#![allow(dead_code, static_mut_refs)]\ntype ClipRect = (i16,i16,i16,i16);\n"
unit += "const NO_CLIP: ClipRect = (i16::MIN,i16::MIN,i16::MAX,i16::MAX);\n"
unit += predicate + walker + tests
with tempfile.TemporaryDirectory(prefix="pico8-disc-visibility-") as directory:
    path = Path(directory)
    (path / "tests.rs").write_text(unit)
    subprocess.run(["rustc", "--edition=2021", "--test", "-O", str(path / "tests.rs"),
                    "-o", str(path / "tests")], check=True)
    subprocess.run([str(path / "tests"), "--nocapture"], check=True)

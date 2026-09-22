#!/usr/bin/env python3
"""Fail before hydration if Cargo and its cache identity select different SDKs."""
import re
import tomllib
from pathlib import Path

root = Path(__file__).resolve().parents[1]
manifest = tomllib.loads((root / "psoxide-pin/Cargo.toml").read_text())
revision = manifest["dependencies"]["psoxide-link"]["rev"]
assert re.fullmatch(r"[0-9a-f]{40}", revision), revision
source = (root / "psoxide-pin/src/main.rs").read_text()
match = re.search(r'const REV: &str = "([0-9a-f]+)";', source)
assert match and match[1] == revision, "hydration REV differs from Cargo SDK pin"
lock = tomllib.loads((root / "psoxide-pin/Cargo.lock").read_text())
package = next(p for p in lock["package"] if p["name"] == "psoxide-link")
assert package["source"].endswith(f"?rev={revision}#{revision}"), package["source"]
print(f"SDK manifest, lock and hydration identity agree: {revision}")

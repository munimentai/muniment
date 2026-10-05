#!/usr/bin/env python3
"""Rewrite the crate list in THIRD_PARTY_RUST_NOTICES.md from src-tauri/Cargo.lock.

Each non-workspace crate in the lockfile gets one line with its license
expression from `cargo metadata`, or from crates.io for an optional crate that
no enabled feature resolves. Crates from the muniment-core repository carry its
FSL license, which their manifests do not declare.
"""

import json
from pathlib import Path
import re
import subprocess
import urllib.request

NOTICES = Path("THIRD_PARTY_RUST_NOTICES.md")
LOCKFILE = Path("src-tauri/Cargo.lock")
CORE_SOURCE = "git+https://github.com/munimentai/muniment-core?"
CORE_LICENSE = "FSL-1.1-ALv2"


def registry_license(name, version):
    request = urllib.request.Request(
        f"https://crates.io/api/v1/crates/{name}/{version}",
        headers={"User-Agent": "muniment-desktop notices (github.com/munimentai/muniment)"})
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response)["version"]["license"]


def main():
    locked = re.findall(r'\[\[package\]\]\nname = "([^"]+)"\nversion = "([^"]+)"\nsource = "([^"]+)"',
                        LOCKFILE.read_text())
    metadata = json.loads(subprocess.check_output([
        "cargo", "metadata", "--manifest-path", "src-tauri/Cargo.toml", "--locked",
        "--format-version", "1"], text=True))
    licenses = {(p["name"], p["version"]): p["license"] for p in metadata["packages"]}
    lines = []
    for name, version, source in locked:
        if source.startswith(CORE_SOURCE):
            license = CORE_LICENSE
        elif (name, version) in licenses:
            license = licenses[(name, version)]
        else:
            license = registry_license(name, version)
        if not license:
            raise SystemExit(f"no license is recorded for {name} {version}")
        lines.append(f"- `{name}` {version} — {license}")
    text = NOTICES.read_text()
    header = text[:text.index("\n- `") + 1]
    NOTICES.write_text(header + "\n".join(sorted(set(lines))) + "\n")


if __name__ == "__main__":
    main()

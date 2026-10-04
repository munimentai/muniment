#!/usr/bin/env python3
"""Enforce ADR 0030 without a GUI toolchain."""

import json
from pathlib import Path
import re
import subprocess
import sys

ADR = Path("docs/decisions/0030-public-core-boundary.md")
MANIFEST = "src-tauri/Cargo.toml"
SHELL = "muniment-desktop"
INTEGRATION = "muniment-desktop-integration"
DESKTOP_ONLY_MODULES = ("auth", "browser_control", "chat_grant")
CORE_SOURCE = re.compile(
    r"git\+https://github\.com/munimentai/muniment-core\?tag=(v\d+\.\d+\.\d+)#[0-9a-f]{40}")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def inventory(text):
    tables = {"Shared": set(), "Desktop": set()}
    section = None
    seen = set()
    for line in text.splitlines():
        if line.startswith("## "):
            section = line[3:] if line[3:] in tables else None
        if section and line.startswith("| "):
            cells = [cell.strip() for cell in line.strip("|").split("|")]
            if cells[0] in ("Kind", "---"):
                continue
            require(len(cells) == 3, "The boundary table needs three columns.")
            kind, name, reason = cells
            require(kind == "crate" and reason, "The boundary row needs a crate and a reason.")
            require(re.fullmatch(r"[a-z][a-z0-9-]*", name),
                    f"The boundary name is invalid: {name}")
            require(name not in seen, f"The boundary repeats {name}.")
            seen.add(name)
            tables[section].add(name)
    for section, names in tables.items():
        require(names, f"The {section} table has no entries.")
    return tables


def check_workspace(tables, members):
    require(tables["Desktop"] == set(members),
            "The ADR must list every workspace crate in the Desktop table.")
    require(SHELL in members and INTEGRATION in members,
            "The workspace needs the shell and the integration crate.")


def check_sources(tables, packages):
    """Every shared crate resolves from the muniment-core repository at one tag."""
    tags = set()
    for name in sorted(tables["Shared"]):
        sources = {package["source"] for package in packages if package["name"] == name}
        require(sources, f"Cargo resolved no {name}.")
        for source in sources:
            match = CORE_SOURCE.fullmatch(source or "")
            require(match, f"{name} must come from a muniment-core release tag, not {source}.")
            tags.add(match[1])
    require(len(tags) == 1, f"The shared crates must use one muniment-core tag: {sorted(tags)}")
    return tags.pop()


def check_modules(lib):
    for name in DESKTOP_ONLY_MODULES:
        require(re.search(rf"^(?:#\[[^\n]+\]\s*)*pub mod {name};", lib, re.M),
                f"{INTEGRATION} must declare the desktop-only module {name}.")


def check_tree(text, forbidden_crates, package):
    names = {line.split()[0] for line in text.splitlines() if line.strip()}
    require(package in names, f"Cargo returned no root for {package}.")
    forbidden = {name for name in names if name in forbidden_crates
                 or name == "tauri" or name.startswith("tauri-")}
    require(not forbidden, f"{package} depends on shell packages: {sorted(forbidden)}")


def main():
    require(len(sys.argv) == 1, "The core boundary check accepts no arguments.")
    tables = inventory(ADR.read_text())
    metadata = json.loads(subprocess.check_output([
        "cargo", "metadata", "--manifest-path", MANIFEST, "--locked", "--format-version", "1"],
        text=True))
    members = {p["name"]: p for p in metadata["packages"] if p["id"] in metadata["workspace_members"]}
    check_workspace(tables, members)
    tag = check_sources(tables, metadata["packages"])
    integration = Path(members[INTEGRATION]["manifest_path"]).parent
    check_modules((integration / "src/lib.rs").read_text())
    for name in sorted(tables["Desktop"] - {SHELL}):
        tree = subprocess.check_output([
            "cargo", "tree", "--manifest-path", MANIFEST, "--package", name, "--locked",
            "--target", "all", "--edges", "normal,build,dev", "--prefix", "none",
            "--format", "{p}"], text=True)
        check_tree(tree, {SHELL}, name)
    print(f"Core boundary checks passed with muniment-core {tag}.")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, subprocess.CalledProcessError) as error:
        print(f"Core boundary failed: {error}", file=sys.stderr)
        sys.exit(1)

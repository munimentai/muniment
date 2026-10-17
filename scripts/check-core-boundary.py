#!/usr/bin/env python3
"""Enforce the desktop side of core/docs/core-boundary.md without a GUI toolchain."""

import json
from pathlib import Path
import re
import subprocess
import sys

BOUNDARY = Path("core/docs/core-boundary.md")
MANIFEST = "src-tauri/Cargo.toml"
SHELL = "muniment-desktop"
INTEGRATION = "muniment-desktop-integration"
DESKTOP_ONLY_MODULES = ("auth", "browser_control", "chat_grant")
CORE_CRATES = Path("core/crates")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def inventory(text):
    """Read the port crates from the Port table and the desktop crates from the Desktop table."""
    tables = {"Port": set(), "Desktop": set()}
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
            kinds = ("crate", "module") if section == "Port" else ("crate",)
            require(kind in kinds and reason, f"The {section} row needs a kind and a reason.")
            if kind == "module":
                continue
            require(re.fullmatch(r"[a-z][a-z0-9-]*", name),
                    f"The boundary name is invalid: {name}")
            require(name not in seen, f"The boundary repeats {name}.")
            seen.add(name)
            tables[section].add(name)
    for section, names in tables.items():
        require(names, f"The {section} table has no crate entries.")
    return tables


def check_workspace(tables, members):
    require(tables["Desktop"] == set(members),
            "The boundary doc must list every workspace crate in the Desktop table.")
    require(SHELL in members and INTEGRATION in members,
            "The workspace needs the shell and the integration crate.")


def check_sources(tables, packages, crates):
    """Every port crate Cargo resolves is a path crate under core/crates/."""
    crates = Path(crates).resolve()
    resolved = [package for package in packages if package["name"] in tables["Port"]]
    require(resolved, "Cargo resolved no port crate.")
    for package in resolved:
        name, source = package["name"], package["source"]
        require(source is None, f"{name} must come from core/crates/, not {source}.")
        manifest = Path(package["manifest_path"]).resolve()
        require(manifest.is_relative_to(crates),
                f"{name} must come from core/crates/, not {manifest}.")
    return sorted({package["name"] for package in resolved})


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
    tables = inventory(BOUNDARY.read_text())
    metadata = json.loads(subprocess.check_output([
        "cargo", "metadata", "--manifest-path", MANIFEST, "--locked", "--format-version", "1"],
        text=True))
    members = {p["name"]: p for p in metadata["packages"] if p["id"] in metadata["workspace_members"]}
    check_workspace(tables, members)
    port = check_sources(tables, metadata["packages"], CORE_CRATES)
    integration = Path(members[INTEGRATION]["manifest_path"]).parent
    check_modules((integration / "src/lib.rs").read_text())
    for name in sorted(tables["Desktop"] - {SHELL}):
        tree = subprocess.check_output([
            "cargo", "tree", "--manifest-path", MANIFEST, "--package", name, "--locked",
            "--target", "all", "--edges", "normal,build,dev", "--prefix", "none",
            "--format", "{p}"], text=True)
        check_tree(tree, {SHELL}, name)
    print(f"Desktop boundary checks passed with {len(port)} port crates from {CORE_CRATES}.")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, subprocess.CalledProcessError) as error:
        print(f"Desktop boundary failed: {error}", file=sys.stderr)
        sys.exit(1)

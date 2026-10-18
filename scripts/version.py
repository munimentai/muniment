#!/usr/bin/env python3
"""The one Muniment version and the copies that must match it.

The source is `version` under `[workspace.package]` in core/Cargo.toml. Every
core crate inherits it. The desktop workspace keeps a copy under its own
`[workspace.package]`, which its crates inherit, and package.json and
package-lock.json keep the copy that Tauri and the frontend read. One
`vX.Y.Z` tag releases all of them.

`check` (the default) fails when a copy differs from the source, a crate sets
its own version, or a lockfile still lists an older version of a workspace
crate. `print` prints the source version. `set X.Y.Z` writes the version into
the source and every copy, then lets Cargo refresh both lockfiles.
"""

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCE = "core/Cargo.toml"
DESKTOP = "src-tauri/Cargo.toml"
WORKSPACES = ["core", "src-tauri"]
PACKAGE = "package.json"
PACKAGE_LOCK = "package-lock.json"
TAURI_CONFIG = "src-tauri/tauri.conf.json"
STRICT = re.compile(r"(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)")
WORKSPACE_VERSION = re.compile(r'(\[workspace\.package\]\n(?:[^\[\n].*\n|\n)*?version = ")([^"]*)(")')
INHERITED = "version.workspace = true"
TOP_VERSION = re.compile(r'^(  "version": ")[^"]*(")', re.M)
ROOT_PACKAGE_VERSION = re.compile(r'(\n  "packages": \{\n    "": \{\n(?:      .*\n)*?      "version": ")[^"]*(")')


def read(root, path):
    return (root / path).read_text(encoding="utf-8")


def workspace_version(root, path):
    match = WORKSPACE_VERSION.search(read(root, path))
    return match[2] if match else None


def members(root, workspace):
    """The manifest of every member of the workspace in `workspace`."""
    text = read(root, f"{workspace}/Cargo.toml")
    listed = re.search(r"^members = \[(.*?)\]", text, re.M | re.S)
    names = re.findall(r'"([^"]+)"', listed[1]) if listed else []
    return [Path(workspace) / name / "Cargo.toml" for name in names]


def path_packages(lock):
    """(name, version) of each lockfile package without a source: the
    workspace crates and their path dependencies."""
    for block in lock.split("[[package]]")[1:]:
        if "\nsource = " in block:
            continue
        name = re.search(r'^name = "([^"]+)"', block, re.M)
        version = re.search(r'^version = "([^"]+)"', block, re.M)
        if name and version:
            yield name[1], version[1]


def problems(root=ROOT):
    version = workspace_version(root, SOURCE)
    if version is None:
        return [f"{SOURCE} names no version under [workspace.package]."]
    found = []
    if not STRICT.fullmatch(version):
        found.append(f"{SOURCE} names {version}, which is not X.Y.Z.")
    desktop = workspace_version(root, DESKTOP)
    if desktop != version:
        found.append(f"{DESKTOP} names {desktop} under [workspace.package], not {version}.")
    for workspace in WORKSPACES:
        for manifest in members(root, workspace):
            package = read(root, manifest).split("\n[", 1)[0]
            if INHERITED not in package:
                found.append(f"{manifest} does not inherit the workspace version.")
        for name, listed in path_packages(read(root, f"{workspace}/Cargo.lock")):
            if listed != version:
                found.append(f"{workspace}/Cargo.lock lists {name} {listed}, not {version}.")
    package = json.loads(read(root, PACKAGE)).get("version")
    if package != version:
        found.append(f"{PACKAGE} names {package}, not {version}.")
    lock = json.loads(read(root, PACKAGE_LOCK))
    for where, listed in (("version", lock.get("version")),
                          ('packages[""].version', lock.get("packages", {}).get("", {}).get("version"))):
        if listed != version:
            found.append(f"{PACKAGE_LOCK} {where} is {listed}, not {version}.")
    tauri = json.loads(read(root, TAURI_CONFIG)).get("version")
    if tauri != f"../{PACKAGE}":
        found.append(f"{TAURI_CONFIG} names version {tauri}, not ../{PACKAGE}.")
    return found


def set_version(version, root=ROOT, refresh=True):
    if not STRICT.fullmatch(version):
        raise SystemExit(f"{version} is not an X.Y.Z version.")
    for path in (SOURCE, DESKTOP):
        text, count = WORKSPACE_VERSION.subn(rf"\g<1>{version}\g<3>", read(root, path), count=1)
        if count != 1:
            raise SystemExit(f"{path} names no version under [workspace.package].")
        (root / path).write_text(text, encoding="utf-8")
    # The JSON files change in place, so npm's formatting stays as it is.
    for path, patterns in ((PACKAGE, [TOP_VERSION]), (PACKAGE_LOCK, [TOP_VERSION, ROOT_PACKAGE_VERSION])):
        text = read(root, path)
        for pattern in patterns:
            text, count = pattern.subn(rf"\g<1>{version}\g<2>", text, count=1)
            if count != 1:
                raise SystemExit(f"{path} has no version where {pattern.pattern} expects it.")
        (root / path).write_text(text, encoding="utf-8")
    if refresh:
        for workspace in WORKSPACES:
            subprocess.run(["cargo", "metadata", "--format-version", "1", "--manifest-path",
                            str(root / workspace / "Cargo.toml")], check=True, stdout=subprocess.DEVNULL)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("command", nargs="?", default="check", choices=["check", "print", "set"])
    parser.add_argument("version", nargs="?")
    args = parser.parse_args()
    if args.command == "print":
        print(workspace_version(ROOT, SOURCE) or "")
        return 0
    if args.command == "set":
        if not args.version:
            parser.error("set needs a version")
        set_version(args.version.removeprefix("v"))
    found = problems()
    for problem in found:
        print(problem, file=sys.stderr)
    if found:
        print(f"Set every copy with: python3 scripts/version.py set {workspace_version(ROOT, SOURCE)}",
              file=sys.stderr)
        return 1
    print(f"Every copy names version {workspace_version(ROOT, SOURCE)}.")
    return 0


if __name__ == "__main__":
    sys.exit(main())

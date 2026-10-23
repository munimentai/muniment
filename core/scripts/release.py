#!/usr/bin/env python3
"""Plan the next release from conventional commit prefixes since the last tag.

Releases are `vX.Y.Z` tags. One tag releases the desktop app, the CLI, the
server and the runtime image together, so every commit counts. `feat:` is
minor, `fix:` and `perf:` are patch, and a `!` before the colon or a
`BREAKING CHANGE:` trailer is major. Before 1.0 a breaking change is minor.
Every other commit cuts no release.

The script prints the plan and the version that `[workspace.package]` in
Cargo.toml names, and changes nothing. The release workflow creates the tag
from a nightly build whose source names the planned version.

`--print-tag` prints only the next tag, or nothing when no release is due.
"""

import argparse
from pathlib import Path
import re
import subprocess
import sys

PREFIX = "v"
TAG = re.compile(re.escape(PREFIX) + r"(\d+)\.(\d+)\.(\d+)")
SUBJECT = re.compile(r"(?P<type>[a-z]+)(?:\([^)]*\))?(?P<bang>!)?: \S")
BREAKING = re.compile(r"^BREAKING[ -]CHANGE: ", re.M)
LEVELS = {"feat": 2, "fix": 1, "perf": 1}
RECORD, FIELD = "\x1e", "\x1f"
MANIFEST = Path(__file__).resolve().parents[1] / "Cargo.toml"
WORKSPACE_VERSION = re.compile(r'\[workspace\.package\]\n(?:[^\[\n].*\n|\n)*?version = "([^"]*)"')


def git(*args):
    return subprocess.check_output(["git", *args], text=True)


def last_tag(revision):
    tags = git("tag", "--merged", revision, "--list", f"{PREFIX}*").split()
    versions = [tuple(map(int, m.groups())) for t in tags if (m := TAG.fullmatch(t))]
    return max(versions, default=None)


def level(subject, body):
    """Returns 3 for major, 2 for minor, 1 for patch, and 0 for no release."""
    match = SUBJECT.match(subject)
    if match and match["bang"] or BREAKING.search(body):
        return 3
    return LEVELS.get(match["type"], 0) if match else 0


def bump(version, change):
    major, minor, patch = version
    if change == 3 and major > 0:
        return (major + 1, 0, 0)
    if change >= 2:
        return (major, minor + 1, 0)
    return (major, minor, patch + 1)


def commits(base, revision):
    log = git("log", "--no-merges", f"--format=%h{FIELD}%s{FIELD}%b{RECORD}", f"{base}..{revision}")
    for record in log.split(RECORD):
        if record.strip():
            short, subject, body = record.strip("\n").split(FIELD, 2)
            yield short, subject, body


def source_version(manifest=MANIFEST):
    match = WORKSPACE_VERSION.search(manifest.read_text(encoding="utf-8"))
    return match[1] if match else None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--revision", default="HEAD")
    parser.add_argument("--print-tag", action="store_true",
                        help="print only the next tag, or nothing when none is due")
    args = parser.parse_args()
    report = sys.stderr if args.print_tag else sys.stdout

    version = last_tag(args.revision)
    if version is None:
        print(f"No {PREFIX}* tag exists. Create the first release tag by hand.", file=report)
        return
    base = PREFIX + "%d.%d.%d" % version
    entries = list(commits(base, args.revision))
    change = max((level(s, b) for _, s, b in entries), default=0)
    if change == 0:
        print(f"No release: no feat, fix, perf, or breaking commit since {base}.", file=report)
        return
    tag = PREFIX + "%d.%d.%d" % bump(version, change)
    if args.print_tag:
        print(tag)
        return
    notes = "".join(f"- {subject} ({short})\n" for short, subject, _ in entries)
    print(f"{base} -> {tag}\n{notes}", end="")
    source = source_version()
    if source != tag.removeprefix(PREFIX):
        print(f"Cargo.toml names {source}. Set {tag.removeprefix(PREFIX)} before the nightly build.")


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as error:
        print(f"Release planning failed: {error}", file=sys.stderr)
        sys.exit(1)

"""Wrap a desktop-CI command with separate slot and guest budgets."""

import pathlib
import shlex
import sys


# Leave time for VM setup and artifact collection after the slot opens.
COLLECTION_SECONDS = 1200
CLEANUP_SECONDS = 30


def command(slot_seconds, remote):
    if not slot_seconds.isascii() or not slot_seconds.isdecimal() or int(slot_seconds) <= 0:
        raise ValueError("Provide a positive slot-wait budget in seconds.")
    args = shlex.split(remote)
    if args[:2] != ["sudo", "desktop-ci"] or args.count("--build-timeout") != 1:
        raise ValueError("Provide a desktop-CI command with one guest build timeout.")
    args.insert(1, "-n")
    index = args.index("--build-timeout") + 1
    build = args[index] if index < len(args) else ""
    if not build.isascii() or not build.isdecimal() or int(build) <= 0:
        raise ValueError("Provide a positive guest build timeout in seconds.")
    wrapper = (pathlib.Path(__file__).resolve().parent.parent / "test/e2e/support/desktop-ci-budget.py").read_text()
    return shlex.join(["python3", "-c", wrapper, slot_seconds,
                       str(int(build) + COLLECTION_SECONDS), str(CLEANUP_SECONDS), *args])


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise ValueError("Provide the slot-wait budget and the desktop-CI command.")
    print(command(*sys.argv[1:]))

#!/usr/bin/env python3
"""Pack files into a gzipped tar, each a regular file in the archive root.

The archive is reproducible: entries sort by name, and every entry has mtime
0, owner 0 and mode 0755 for the server or 0644 for a library. A symbolic
link is packed as the file it points to.

Usage: pack.py <archive> <file>...
"""

import gzip
import io
import os
import sys
import tarfile


def pack(archive, paths):
    entries = {}
    for path in paths:
        name = os.path.basename(path)
        if name in entries:
            continue
        with open(path, "rb") as file:
            entries[name] = file.read()
    if "muniment-server" not in entries:
        raise SystemExit("pack.py: no muniment-server among the files")
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w", format=tarfile.GNU_FORMAT) as tar:
        for name in sorted(entries):
            info = tarfile.TarInfo(name)
            info.size = len(entries[name])
            info.mode = 0o755 if name == "muniment-server" else 0o644
            info.mtime = 0
            info.uid = info.gid = 0
            info.uname = info.gname = ""
            tar.addfile(info, io.BytesIO(entries[name]))
    with open(archive, "wb") as file:
        with gzip.GzipFile(filename="", mode="wb", fileobj=file, mtime=0) as compressed:
            compressed.write(buffer.getvalue())


if __name__ == "__main__":
    if len(sys.argv) < 3:
        raise SystemExit(__doc__)
    pack(sys.argv[1], sys.argv[2:])

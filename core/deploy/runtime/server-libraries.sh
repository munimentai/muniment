#!/usr/bin/env bash
# Select, check and pack the sherpa-onnx speech runtime libraries that a
# muniment-server build for one Linux target links and loads.
#
# The libraries come from the sherpa-onnx 1.13.2 shared release archive for
# the target, pinned by SHA-256. Each target unpacks into its own directory
# under <cache>, and every library must be an ELF file for the target's
# machine. The desktop's checked-in libraries serve only the desktop host,
# so a server build never reads them.
#
# Usage:
#   server-libraries.sh fetch <target> <cache>
#     Print the directory that holds the target's checked libraries.
#   server-libraries.sh pin-sha256 <target>
#     Print the pinned SHA-256 of the target's sherpa-onnx archive.
#   server-libraries.sh cargo-args <lib-dir> <config>
#     Write <config>, a Cargo config file whose forced [env] entry points
#     sherpa-onnx-sys at <lib-dir>, and print, one per line, the Cargo
#     arguments that load it. A forced [env] entry in a parent
#     .cargo/config.toml outranks the process environment, so only a --config
#     argument, which Cargo ranks above every discovered config file, replaces
#     it for one invocation. The argument names a file ending in .toml rather
#     than a dotted KEY=VALUE, because cargo zigbuild parses each --config
#     KEY=VALUE as a whole TOML document and rejects dotted keys, while Cargo
#     rejects an inline table there.
#   server-libraries.sh closure <target> <lib-dir> <binary>
#     Check the binary, the speech runtime libraries and every shared library
#     they need from <lib-dir> against the target, and print the files to
#     pack: the binary first.
#   server-libraries.sh check-archive <target> <archive>
#     Check that the archive holds only regular files in its root, the
#     muniment-server binary and the libraries its closure needs, all built
#     for the target.
set -euo pipefail

sherpa_version=1.13.2
release_base="https://github.com/k2-fsa/sherpa-onnx/releases/download/v$sherpa_version"

# Print "<archive> <sha256>" for a target, or fail for any other target.
pin() {
  case "$1" in
    x86_64-unknown-linux-gnu)
      echo "sherpa-onnx-v$sherpa_version-linux-x64-shared-lib.tar.bz2 bb2da15d4ab5fea369b91edf3caf98bc25f95b3bbaf856215c87b20f69f347ef" ;;
    aarch64-unknown-linux-gnu)
      echo "sherpa-onnx-v$sherpa_version-linux-aarch64-shared-cpu-lib.tar.bz2 44449a83f19649b2466c97b4d2df57c158dbede6e1e044f6519cf297df17585c" ;;
    *)
      echo "server-libraries.sh: no pinned sherpa-onnx libraries for $1" >&2
      return 2 ;;
  esac
}

# The ELF checks. Both targets are 64-bit little-endian, so one reader serves.
elf() {
  python3 -B - "$@" <<'PY'
import io
import os
import struct
import sys
import tarfile

MACHINES = {"x86_64-unknown-linux-gnu": 62, "aarch64-unknown-linux-gnu": 183}
LOADERS = {"x86_64-unknown-linux-gnu": "ld-linux-x86-64.so.2",
           "aarch64-unknown-linux-gnu": "ld-linux-aarch64.so.1"}
# The glibc and C++ runtime libraries every Debian 12 or later system has.
SYSTEM = {"libc.so.6", "libm.so.6", "libdl.so.2", "libpthread.so.0", "librt.so.1",
          "libstdc++.so.6", "libgcc_s.so.1", "libutil.so.1"}
ENTRY = "server-libraries.sh"


def fail(message):
    sys.stderr.write(f"{ENTRY}: {message}\n")
    sys.exit(1)


def parse(data, name):
    """Return (machine, needed, runpath) of an ELF64 little-endian file."""
    if data[:4] != b"\x7fELF":
        fail(f"{name} is not an ELF file")
    if data[4] != 2 or data[5] != 1:
        fail(f"{name} is not a 64-bit little-endian ELF file")
    machine = struct.unpack_from("<H", data, 18)[0]
    phoff = struct.unpack_from("<Q", data, 32)[0]
    phentsize, phnum = struct.unpack_from("<HH", data, 54)
    loads, dynamic = [], None
    for index in range(phnum):
        p_type, _, p_offset, p_vaddr, _, p_filesz = struct.unpack_from(
            "<IIQQQQ", data, phoff + index * phentsize)
        if p_type == 1:
            loads.append((p_vaddr, p_offset, p_filesz))
        elif p_type == 2:
            dynamic = (p_offset, p_filesz)
    needed, runpath = [], []
    if dynamic is None:
        return machine, needed, runpath
    entries, strtab = [], None
    for offset in range(dynamic[0], dynamic[0] + dynamic[1], 16):
        tag, value = struct.unpack_from("<qQ", data, offset)
        if tag == 0:
            break
        if tag == 5:
            strtab = value
        entries.append((tag, value))
    if strtab is None:
        fail(f"{name} has a dynamic section without a string table")
    base = next((offset + strtab - vaddr for vaddr, offset, size in loads
                 if vaddr <= strtab < vaddr + size), None)
    if base is None:
        fail(f"{name} has a string table outside its loaded segments")

    def string(at):
        end = data.index(b"\0", base + at)
        return data[base + at:end].decode()

    for tag, value in entries:
        if tag == 1:
            needed.append(string(value))
        elif tag in (15, 29):
            runpath.extend(string(value).split(":"))
    return machine, needed, runpath


def machine_of(target):
    if target not in MACHINES:
        fail(f"{target} is not a supported server target")
    return MACHINES[target]


def check_machine(target, data, name):
    machine, needed, runpath = parse(data, name)
    if machine != machine_of(target):
        fail(f"{name} is built for ELF machine {machine}, not {target}")
    return needed, runpath


# The speech runtime libraries every archive carries. The server loads ONNX
# Runtime at run time rather than through its own dynamic section, so these
# join the binary's needed libraries as the roots of the closure.
SPEECH = ["libsherpa-onnx-c-api.so", "libonnxruntime.so"]


def closure(target, find, binary_name, binary):
    """The libraries to pack beside the binary, checked against the target.

    `find(name)` returns a library's bytes, or None when it is not supplied."""
    needed, runpath = check_machine(target, binary, binary_name)
    if "$ORIGIN" not in runpath:
        fail(f"{binary_name} does not load libraries from its own directory ($ORIGIN)")
    system = SYSTEM | {LOADERS[target]}
    queue, packed = list(needed) + SPEECH, []
    while queue:
        name = queue.pop(0)
        if name in packed or name in system:
            continue
        data = find(name)
        if data is None:
            fail(f"{binary_name} needs {name}, which neither the speech runtime "
                 f"libraries nor the system libraries supply")
        packed.append(name)
        queue.extend(check_machine(target, data, name)[0])
    return packed


def read(path):
    with open(path, "rb") as file:
        return file.read()


command, target, *rest = sys.argv[1:]
machine_of(target)
if command == "machine":
    for path in rest:
        check_machine(target, read(path), path)
elif command == "closure":
    lib_dir, binary = rest

    def find(name):
        path = os.path.join(lib_dir, name)
        return read(path) if os.path.isfile(path) else None

    print(binary)
    for name in closure(target, find, binary, read(binary)):
        print(os.path.join(lib_dir, name))
elif command == "check-archive":
    (archive,) = rest
    entries = {}
    with tarfile.open(archive, "r:gz") as tar:
        for member in tar.getmembers():
            if not member.isreg() or "/" in member.name or member.name in ("", ".", ".."):
                fail(f"{archive} holds {member.name}, which is not a regular file in its root")
            entries[member.name] = tar.extractfile(member).read()
    if "muniment-server" not in entries:
        fail(f"{archive} holds no muniment-server")
    packed = closure(target, entries.get, "muniment-server", entries["muniment-server"])
    extra = sorted(set(entries) - set(packed) - {"muniment-server"})
    if extra:
        fail(f"{archive} holds files the server does not load: {', '.join(extra)}")
    print(f"{archive}: muniment-server and {', '.join(sorted(packed))} for {target}")
else:
    fail(f"unknown command {command}")
PY
}

# Check that <lib> holds the two libraries the server links, each an ELF
# file for the target.
verify() {
  local target=$1 lib=$2 name
  for name in libsherpa-onnx-c-api.so libonnxruntime.so; do
    if [ ! -f "$lib/$name" ]; then
      echo "server-libraries.sh: the $target speech runtime libraries hold no $name" >&2
      return 1
    fi
  done
  elf machine "$target" "$lib"/*.so*
}

fetch() {
  local target=$1 cache=$2 archive sha256 directory lib staging
  read -r archive sha256 <<<"$(pin "$target")"
  directory="$cache/sherpa-onnx-v$sherpa_version/$target"
  lib="$directory/lib"
  # A finished directory records the archive checksum it came from and is
  # checked again before each use.
  if [ -d "$lib" ] && [ "$(cat "$directory/sha256" 2>/dev/null)" = "$sha256" ]; then
    verify "$target" "$lib"
    echo "$lib"
    return
  fi
  rm -rf "$directory"
  mkdir -p "$directory"
  echo "server-libraries.sh: downloading $archive for $target" >&2
  curl --proto '=https' --tlsv1.2 -sSfL -o "$directory/$archive" "$release_base/$archive" >&2
  if ! printf '%s  %s\n' "$sha256" "$directory/$archive" | sha256sum --check --quiet >/dev/null 2>&1; then
    rm -rf "$directory"
    echo "server-libraries.sh: $archive does not match its pinned SHA-256 $sha256" >&2
    return 1
  fi
  staging="$directory/staging"
  mkdir -p "$staging"
  # Python unpacks the bzip2 archive, so the runner needs no bzip2 command.
  if ! python3 -B - "$directory/$archive" "$staging" <<'PY'
import os
import sys
import tarfile

archive, staging = sys.argv[1:]
with tarfile.open(archive, "r:bz2") as tar:
    for member in tar.getmembers():
        parts = member.name.split("/")
        if len(parts) != 3 or parts[1] != "lib" or not member.isreg():
            continue
        if parts[2] in ("", ".", ".."):
            raise SystemExit(f"{archive} holds an unsafe name {member.name}")
        os.makedirs(os.path.join(staging, "lib"), exist_ok=True)
        with tar.extractfile(member) as source, \
                open(os.path.join(staging, "lib", parts[2]), "wb") as target:
            target.write(source.read())
PY
  then
    rm -rf "$directory"
    echo "server-libraries.sh: $archive does not unpack" >&2
    return 1
  fi
  rm -f "$directory/$archive"
  if ! verify "$target" "$staging/lib"; then
    rm -rf "$directory"
    return 1
  fi
  mv "$staging/lib" "$lib"
  rm -rf "$staging"
  echo "$sha256" > "$directory/sha256"
  echo "$lib"
}

case "${1:-}" in
  fetch)
    [ $# -eq 3 ] || { echo "usage: server-libraries.sh fetch <target> <cache>" >&2; exit 2; }
    fetch "$2" "$3" ;;
  pin-sha256)
    [ $# -eq 2 ] || { echo "usage: server-libraries.sh pin-sha256 <target>" >&2; exit 2; }
    pinned=$(pin "$2")
    echo "${pinned#* }" ;;
  cargo-args)
    [ $# -eq 3 ] || { echo "usage: server-libraries.sh cargo-args <lib-dir> <config>" >&2; exit 2; }
    for path in "$2" "$3"; do
      case "$path" in
        *[\"\\]* | *$'\n'*) echo "server-libraries.sh: $path holds a quote, a backslash or a newline" >&2; exit 2 ;;
        /*) ;;
        *) echo "server-libraries.sh: $path is not an absolute path" >&2; exit 2 ;;
      esac
    done
    case "$3" in
      *.toml) ;;
      *) echo "server-libraries.sh: $3 does not end in .toml, so Cargo would not read it as a file" >&2; exit 2 ;;
    esac
    # The path holds no quote, backslash or newline, so a TOML basic string
    # holds it as written.
    printf '[env]\nSHERPA_ONNX_LIB_DIR = { value = "%s", force = true, relative = false }\n' "$2" > "$3"
    printf '%s\n' --config "$3" ;;
  closure)
    [ $# -eq 4 ] || { echo "usage: server-libraries.sh closure <target> <lib-dir> <binary>" >&2; exit 2; }
    pin "$2" >/dev/null
    elf closure "$2" "$3" "$4" ;;
  check-archive)
    [ $# -eq 3 ] || { echo "usage: server-libraries.sh check-archive <target> <archive>" >&2; exit 2; }
    pin "$2" >/dev/null
    elf check-archive "$2" "$3" ;;
  *)
    echo "usage: server-libraries.sh fetch|pin-sha256|cargo-args|closure|check-archive ..." >&2
    exit 2 ;;
esac

"""Tests for package.sh, server-libraries.sh and release-assets.sh.

Each test copies the scripts into a throwaway tree and runs them with fake
cargo, curl and signing commands on PATH, so no test needs credentials, the
network or a cross toolchain. The fake cargo build writes small ELF files for
the machine its --target names. One test runs the real Cargo against a
throwaway crate whose .cargo/config.toml forces SHERPA_ONNX_LIB_DIR, as the
repository's does, to show that the arguments package.sh passes outrank it.

Run: python3 -B core/deploy/runtime/test_package.py
"""

import os
import shutil
import struct
import subprocess
import tarfile
import tempfile
import textwrap
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
CORE = HERE.parents[1]
LIBRARIES = HERE / "server-libraries.sh"
MACHINES = {"x86_64": 62, "aarch64": 183}
LOADERS = {"x86_64": "ld-linux-x86-64.so.2", "aarch64": "ld-linux-aarch64.so.1"}
SHERPA = "sherpa-onnx-v1.13.2"


def elf(machine, needed=(), runpath=None):
    """A minimal ELF64 little-endian shared object with a dynamic section."""
    strings = b"\0"
    offsets = []
    for name in list(needed) + ([runpath] if runpath else []):
        offsets.append(len(strings))
        strings += name.encode() + b"\0"
    header_size, phdr_size = 64, 56
    strtab_offset = header_size + 2 * phdr_size
    dynamic_offset = strtab_offset + len(strings)
    entries = [(1, offset) for offset in offsets[:len(needed)]]
    if runpath:
        entries.append((29, offsets[-1]))
    entries += [(5, strtab_offset), (0, 0)]
    dynamic = b"".join(struct.pack("<qQ", tag, value) for tag, value in entries)
    total = dynamic_offset + len(dynamic)
    header = b"\x7fELF" + bytes([2, 1, 1]) + bytes(9)
    header += struct.pack("<HHIQQQIHHHHHH", 3, machine, 1, 0, header_size, 0, 0,
                          header_size, phdr_size, 2, 64, 0, 0)
    load = struct.pack("<IIQQQQQQ", 1, 5, 0, 0, 0, total, total, 0x1000)
    dyn = struct.pack("<IIQQQQQQ", 2, 6, dynamic_offset, dynamic_offset,
                      dynamic_offset, len(dynamic), len(dynamic), 8)
    return header + load + dyn + strings + dynamic


SYSTEM_NEEDS = ("libc.so.6", "libm.so.6", "libstdc++.so.6", "libgcc_s.so.1")


def speech_libraries(directory, arch, machine=None, extra_need=None):
    """Write the two libraries the server links, as sherpa-onnx ships them."""
    machine = MACHINES[arch] if machine is None else machine
    directory.mkdir(parents=True, exist_ok=True)
    c_api_needs = ["libonnxruntime.so", *SYSTEM_NEEDS, LOADERS[arch]]
    if extra_need:
        c_api_needs.append(extra_need)
    (directory / "libsherpa-onnx-c-api.so").write_bytes(elf(machine, c_api_needs, "$ORIGIN"))
    (directory / "libonnxruntime.so").write_bytes(elf(machine, SYSTEM_NEEDS, "$ORIGIN"))


FAKE_CARGO = textwrap.dedent('''\
    #!/usr/bin/env python3
    """A cargo zigbuild that writes the files a real build leaves behind."""
    import os, shutil, struct, sys
    sys.path.insert(0, os.environ["FAKE_HELPERS"])
    from test_package import elf, MACHINES, LOADERS
    args = sys.argv[1:]
    with open(os.environ["FAKE_CARGO_LOG"], "a") as log:
        log.write(" ".join(args) + "\\n")
    if os.environ.get("FAKE_CARGO_FAIL"):
        sys.stderr.write("error: linking with `cc` failed\\n")
        sys.exit(101)
    assert args[0] == "zigbuild", args
    assert "--locked" in args and "--release" in args, args
    settings = {}
    for index, arg in enumerate(args):
        if arg == "--config":
            key, _, value = args[index + 1].partition("=")
            settings[key] = value.strip('"')
    assert settings.get("env.SHERPA_ONNX_LIB_DIR.force") == "true", settings
    lib_dir = settings["env.SHERPA_ONNX_LIB_DIR.value"]
    target = args[args.index("--target") + 1]
    assert target.endswith(".2.36"), target
    target = target[:-len(".2.36")]
    arch = target.split("-")[0]
    machine = MACHINES[os.environ.get("FAKE_CARGO_MACHINE", arch)]
    release = os.path.join(os.environ["CARGO_TARGET_DIR"], target, "release")
    os.makedirs(release, exist_ok=True)
    # The real server needs only glibc and loads ONNX Runtime at run time.
    needed = ["libgcc_s.so.1", "libm.so.6", "libc.so.6", LOADERS[arch]]
    with open(os.path.join(release, "muniment-server"), "wb") as file:
        file.write(elf(machine, needed, "$ORIGIN:" + lib_dir))
    # sherpa-onnx-sys copies every library in its directory beside the binary.
    for name in os.listdir(lib_dir):
        shutil.copy(os.path.join(lib_dir, name), os.path.join(release, name))
''')

FAKE_CURL = textwrap.dedent('''\
    #!/usr/bin/env bash
    # Write FAKE_CURL_BODY to the -o file, as a download that arrived corrupt.
    while [ $# -gt 0 ]; do
      if [ "$1" = -o ]; then printf '%s' "${FAKE_CURL_BODY:-corrupt}" > "$2"; shift; fi
      shift
    done
''')


class Tree(unittest.TestCase):
    """A copy of core/'s packaging scripts with fake commands on PATH."""

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.root = Path(self._tmp.name)
        self.core = self.root / "core"
        (self.core / "deploy/runtime").mkdir(parents=True)
        (self.core / "ci").mkdir()
        for name in ("package.sh", "server-libraries.sh", "pack.py", "release-assets.sh"):
            shutil.copy(HERE / name, self.core / "deploy/runtime" / name)
        (self.core / "Cargo.toml").write_text('[workspace.package]\nversion = "1.2.3"\n')
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.command("cargo", FAKE_CARGO)
        self.command("curl", FAKE_CURL)
        self.target_dir = self.root / "target"
        self.cache = self.root / "cache"
        self.log = self.root / "cargo.log"
        self.env = dict(os.environ)
        self.env.update(
            PATH=f"{self.bin}{os.pathsep}{os.environ['PATH']}",
            CARGO_TARGET_DIR=str(self.target_dir),
            MUNIMENT_SERVER_LIBRARY_CACHE=str(self.cache),
            FAKE_HELPERS=str(HERE),
            FAKE_CARGO_LOG=str(self.log),
        )

    def command(self, name, text):
        path = self.bin / name
        path.write_text(text)
        path.chmod(0o755)

    def cached(self, arch, **options):
        """Fill the cache as a finished, checksum-verified download leaves it."""
        target = f"{arch}-unknown-linux-gnu"
        directory = self.cache / SHERPA / target
        speech_libraries(directory / "lib", arch, **options)
        sha = subprocess.run(["bash", str(LIBRARIES), "pin-sha256", target],
                             capture_output=True, text=True)
        (directory / "sha256").write_text(sha.stdout)
        return directory / "lib"

    def run_script(self, *args, **env):
        return subprocess.run(["bash", *map(str, args)], capture_output=True, text=True,
                              env={**self.env, **env}, cwd=self.root)

    def package(self, arch, **env):
        return self.run_script(self.core / "deploy/runtime/package.sh", "1.2.3", arch,
                               self.root / "dist", **env)

    def archive(self, arch):
        path = self.root / "dist" / f"muniment-server-1.2.3-linux-{arch}.tar.gz"
        with tarfile.open(path, "r:gz") as tar:
            return {member.name: (member, tar.extractfile(member).read())
                    for member in tar.getmembers()}


class Package(Tree):
    def assert_target_archive(self, arch):
        entries = self.archive(arch)
        self.assertEqual(sorted(entries),
                         ["libonnxruntime.so", "libsherpa-onnx-c-api.so", "muniment-server"])
        for name, (member, data) in entries.items():
            self.assertTrue(member.isreg(), name)
            self.assertEqual(struct.unpack_from("<H", data, 18)[0], MACHINES[arch], name)
        self.assertEqual(entries["muniment-server"][0].mode, 0o755)

    def test_each_target_links_its_own_pinned_libraries(self):
        for arch in ("x86_64", "aarch64"):
            with self.subTest(arch=arch):
                lib = self.cached(arch)
                result = self.package(arch)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assert_target_archive(arch)
                last = self.log.read_text().splitlines()[-1]
                self.assertIn(f'env.SHERPA_ONNX_LIB_DIR.value="{lib}"', last)
                self.assertIn(f"--target {arch}-unknown-linux-gnu.2.36", last)

    def test_sequential_builds_share_a_workspace_without_mixing_targets(self):
        x86 = self.cached("x86_64")
        arm = self.cached("aarch64")
        self.assertNotEqual(x86, arm)
        for arch in ("x86_64", "aarch64", "x86_64"):
            result = self.package(arch)
            self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_target_archive("x86_64")
        self.assert_target_archive("aarch64")
        lines = self.log.read_text().splitlines()
        self.assertIn(str(x86), lines[0])
        self.assertIn(str(arm), lines[1])
        self.assertNotIn(str(x86), lines[1])

    def test_stale_libraries_in_the_shared_output_stay_out(self):
        self.cached("aarch64")
        release = self.target_dir / "aarch64-unknown-linux-gnu/release"
        release.mkdir(parents=True)
        (release / "libstale.so").write_bytes(elf(MACHINES["x86_64"]))
        (release / "libsherpa-onnx-cxx-api.so").write_bytes(elf(MACHINES["aarch64"]))
        result = self.package("aarch64")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_target_archive("aarch64")

    def test_a_binary_for_another_machine_fails(self):
        self.cached("aarch64")
        result = self.package("aarch64", FAKE_CARGO_MACHINE="x86_64")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("muniment-server is built for ELF machine 62", result.stderr)
        self.assertFalse((self.root / "dist/muniment-server-1.2.3-linux-aarch64.tar.gz").exists())

    def test_cached_libraries_for_another_machine_fail_before_the_build(self):
        self.cached("aarch64", machine=MACHINES["x86_64"])
        result = self.package("aarch64")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("not aarch64-unknown-linux-gnu", result.stderr)
        self.assertFalse(self.log.exists())

    def test_a_missing_runtime_dependency_fails(self):
        self.cached("x86_64", extra_need="libespeak-ng.so.1")
        result = self.package("x86_64")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("needs libespeak-ng.so.1", result.stderr)

    def test_a_missing_linked_library_fails_before_the_build(self):
        lib = self.cached("x86_64")
        (lib / "libonnxruntime.so").unlink()
        result = self.package("x86_64")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("hold no libonnxruntime.so", result.stderr)
        self.assertFalse(self.log.exists())

    def test_a_corrupt_download_fails_and_leaves_no_cache(self):
        result = self.package("aarch64")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("does not match its pinned SHA-256", result.stderr)
        self.assertFalse((self.cache / SHERPA / "aarch64-unknown-linux-gnu").exists())
        self.assertFalse(self.log.exists())

    def test_a_cache_from_another_checksum_downloads_again(self):
        lib = self.cached("x86_64")
        (lib.parent / "sha256").write_text("0" * 64 + "\n")
        result = self.package("x86_64")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("does not match its pinned SHA-256", result.stderr)

    def test_a_failed_link_fails(self):
        self.cached("aarch64")
        result = self.package("aarch64", FAKE_CARGO_FAIL="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.root / "dist").exists())

    def test_unsupported_targets_fail(self):
        result = self.package("riscv64")
        self.assertEqual(result.returncode, 2)
        result = self.run_script(LIBRARIES, "fetch", "riscv64gc-unknown-linux-gnu", self.cache)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("no pinned sherpa-onnx libraries", result.stderr)

    def test_the_archive_version_must_match_the_workspace(self):
        self.cached("x86_64")
        result = self.run_script(self.core / "deploy/runtime/package.sh", "1.2.4", "x86_64")
        self.assertEqual(result.returncode, 2)
        self.assertIn("names version 1.2.3, not 1.2.4", result.stderr)

    def test_an_archive_with_a_directory_fails_its_check(self):
        path = self.root / "bad.tar.gz"
        with tarfile.open(path, "w:gz") as tar:
            info = tarfile.TarInfo("lib")
            info.type = tarfile.DIRTYPE
            tar.addfile(info)
        result = self.run_script(LIBRARIES, "check-archive", "x86_64-unknown-linux-gnu", path)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("not a regular file in its root", result.stderr)


class ReleaseAssets(Tree):
    def setUp(self):
        super().setUp()
        # The toolchain installers and the signer are stand-ins; the signer
        # records that it ran.
        (self.core / "ci/rust-toolchain.sh").write_text("true\n")
        (self.core / "ci/zig-toolchain.sh").write_text("true\n")
        self.signed = self.root / "signed"
        (self.core / "deploy/runtime/sign-release.sh").write_text(
            f'#!/usr/bin/env bash\ntouch "{self.signed}"\n')
        (self.core / "deploy/runtime/sign-release.sh").chmod(0o755)
        self.command("minisign", "#!/bin/sh\nexit 0\n")

    def release(self, **env):
        return self.run_script(self.core / "deploy/runtime/release-assets.sh", "1.2.3",
                               self.root / "dist", **env)

    def test_both_archives_build_before_signing(self):
        self.cached("x86_64")
        self.cached("aarch64")
        result = self.release()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(self.signed.exists())
        for arch in ("x86_64", "aarch64"):
            self.assertIn("muniment-server", self.archive(arch))

    def test_a_failed_target_signs_nothing(self):
        self.cached("x86_64")
        result = self.release()
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.signed.exists())

    def test_a_failed_link_signs_nothing(self):
        self.cached("x86_64")
        self.cached("aarch64")
        result = self.release(FAKE_CARGO_FAIL="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.signed.exists())


class CargoPrecedence(unittest.TestCase):
    """The real Cargo, against a crate that forces SHERPA_ONNX_LIB_DIR."""

    def setUp(self):
        if shutil.which("cargo") is None:
            self.fail("cargo is not on PATH")
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.root = Path(self._tmp.name)
        (self.root / ".cargo").mkdir()
        (self.root / "desktop").mkdir()
        (self.root / ".cargo/config.toml").write_text(
            '[env]\nSHERPA_ONNX_LIB_DIR = { value = "desktop", relative = true, force = true }\n')
        (self.root / "Cargo.toml").write_text(
            '[package]\nname = "probe"\nversion = "0.0.0"\nedition = "2021"\n'
            'build = "build.rs"\n\n[workspace]\n')
        (self.root / "src").mkdir()
        (self.root / "src/main.rs").write_text('fn main() {}\n')
        # The build script sees what sherpa-onnx-sys's build script sees.
        (self.root / "build.rs").write_text(textwrap.dedent('''\
            fn main() {
                let seen = std::env::var("SHERPA_ONNX_LIB_DIR").unwrap();
                let out = std::env::var("OUT_DIR").unwrap();
                std::fs::write(std::path::Path::new(&out).join("seen"), seen).unwrap();
                println!("cargo:rerun-if-env-changed=SHERPA_ONNX_LIB_DIR");
            }
        '''))
        self.server = self.root / "server libraries"
        self.server.mkdir()

    def seen(self, *args, **env):
        result = subprocess.run(
            ["cargo", "build", "--offline", "--quiet", *args], cwd=self.root,
            capture_output=True, text=True, env={**os.environ, **env,
                                                 "CARGO_TARGET_DIR": str(self.root / "target")})
        self.assertEqual(result.returncode, 0, result.stderr)
        outputs = list((self.root / "target/debug/build").glob("probe-*/out/seen"))
        self.assertEqual(len(outputs), 1)
        return outputs[0].read_text()

    def test_the_process_environment_does_not_outrank_a_forced_entry(self):
        self.assertEqual(self.seen(SHERPA_ONNX_LIB_DIR=str(self.server)),
                         str(self.root / "desktop"))

    def test_the_package_arguments_outrank_a_forced_entry(self):
        args = subprocess.run(["bash", str(LIBRARIES), "cargo-args", str(self.server)],
                              capture_output=True, text=True, check=True).stdout.splitlines()
        self.assertEqual(self.seen(*args), str(self.server))

    def test_the_arguments_reject_a_relative_directory(self):
        result = subprocess.run(["bash", str(LIBRARIES), "cargo-args", "server"],
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 2)


if __name__ == "__main__":
    unittest.main()

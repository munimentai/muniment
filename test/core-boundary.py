#!/usr/bin/env python3
"""Probe the public core boundary without downloads or build artifacts."""

import importlib.util
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


boundary = load("boundary", "scripts/check-core-boundary.py")
core_boundary = load("core_boundary", "core/scripts/check-core-boundary.py")
GIT = "git+https://example.invalid/muniment?tag=v1.2.3#" + "0" * 40


class CoreBoundaryTests(unittest.TestCase):
    def test_inventory_reads_both_tables(self):
        tables = boundary.inventory(boundary.BOUNDARY.read_text())
        self.assertIn("muniment-core", tables["Port"])
        self.assertIn("muniment-router", tables["Port"])
        self.assertNotIn("account", tables["Port"])
        self.assertIn("muniment-runtime", tables["Desktop"])
        self.assertIn("muniment-desktop-integration", tables["Desktop"])

    def test_inventory_rejects_empty_duplicate_and_invalid_rows(self):
        text = boundary.BOUNDARY.read_text()
        for invalid in (
            "", text.replace("## Desktop", "## Other"), text.replace("## Port", "## Other"),
            text.replace("| crate | muniment-cli |",
                         "| crate | muniment-core | It duplicates a port crate. |\n| crate | muniment-cli |"),
            text.replace("| crate | muniment-cli |", "| module | muniment-cli |"),
            text.replace("muniment-core | It holds the local runtime logic and storage contracts.",
                         "muniment-core | "),
            text.replace("| crate | muniment-core |", "| crate | --all |"),
        ):
            with self.subTest(invalid=invalid[:60]), self.assertRaises(ValueError):
                boundary.inventory(invalid)

    def test_workspace_must_match_the_desktop_table(self):
        tables = boundary.inventory(boundary.BOUNDARY.read_text())
        boundary.check_workspace(tables, set(tables["Desktop"]))
        for members in (tables["Desktop"] | {"muniment-core"}, tables["Desktop"] - {"muniment-cli"}):
            with self.assertRaises(ValueError):
                boundary.check_workspace(tables, members)

    def test_port_crates_come_from_core_crates_by_path(self):
        root = Path("/repo")
        tables = {"Port": {"muniment-core", "muniment-attach", "muniment-server"}, "Desktop": set()}

        def package(name, manifest, source=None):
            return {"name": name, "source": source, "manifest_path": str(root / manifest)}

        core = package("muniment-core", "core/crates/core/Cargo.toml")
        attach = package("muniment-attach", "core/crates/attach/Cargo.toml")
        other = package("serde", "registry/serde/Cargo.toml", "registry+https://example.invalid")
        self.assertEqual(boundary.check_sources(tables, [core, attach, other], root / "core/crates"),
                         ["muniment-attach", "muniment-core"])
        for changed in (
            package("muniment-attach", "git/attach/Cargo.toml", GIT),
            package("muniment-attach", "registry/attach/Cargo.toml", "registry+https://example.invalid"),
            package("muniment-attach", "src-tauri/attach/Cargo.toml"),
            package("muniment-attach", "core/crates/../../src-tauri/attach/Cargo.toml"),
            package("muniment-attach", "core/attach/Cargo.toml"),
        ):
            with self.subTest(changed=changed), self.assertRaises(ValueError):
                boundary.check_sources(tables, [core, changed], root / "core/crates")
        with self.assertRaises(ValueError):
            boundary.check_sources(tables, [other], root / "core/crates")

    def test_core_path_dependencies_stay_in_core(self):
        root = Path("/repo/core")

        def crate(*paths):
            return {"muniment-core": {"name": "muniment-core", "dependencies": [
                {"name": f"dep{index}", "path": path} for index, path in enumerate(paths)] + [
                {"name": "serde", "source": "registry+https://example.invalid"}]}}

        core_boundary.check_path_dependencies(crate("/repo/core/crates/attach"), root)
        for outside in ("/repo/src-tauri/integration", "/repo/core/crates/../../src-tauri",
                        "/repo/core-fork/crates/attach", "/elsewhere/attach"):
            with self.subTest(outside=outside), self.assertRaisesRegex(ValueError, "outside the core"):
                core_boundary.check_path_dependencies(crate("/repo/core/crates/attach", outside), root)

    def test_core_files_must_not_name_the_shell_directory(self):
        core_boundary.check_shell_references([])
        with self.assertRaisesRegex(ValueError, "docs/x.md"):
            core_boundary.check_shell_references(["docs/x.md"])
        current = Path.cwd()
        with tempfile.TemporaryDirectory() as temporary:
            subprocess.run(["git", "init", "-q", temporary], check=True)
            Path(temporary, "clean.md").write_text("crates/core\n")
            Path(temporary, "tracked.md").write_text("see ../src-tauri/Cargo.toml\n")
            subprocess.run(["git", "-C", temporary, "add", "clean.md", "tracked.md"], check=True)
            Path(temporary, "untracked.rs").write_text('const SHELL: &str = "src-tauri";\n')
            try:
                os.chdir(temporary)
                self.assertEqual(sorted(core_boundary.shell_references()), ["tracked.md", "untracked.rs"])
                os.chdir(current / "core")
                self.assertEqual(core_boundary.shell_references(), [])
            finally:
                os.chdir(current)

    def test_integration_declares_every_desktop_only_module(self):
        lib = "pub mod auth;\n#[cfg(unix)]\npub mod browser_control;\npub mod chat_grant;\n"
        boundary.check_modules(lib)
        for name in boundary.DESKTOP_ONLY_MODULES:
            with self.subTest(name=name), self.assertRaises(ValueError):
                boundary.check_modules(lib.replace(f"pub mod {name};", ""))

    def test_tree_rejects_the_shell_and_tauri_variants(self):
        for forbidden in ("tauri", "tauri-build", "tauri-plugin-dialog", "muniment-desktop"):
            with self.subTest(forbidden=forbidden), self.assertRaises(ValueError):
                boundary.check_tree(f"muniment-cli v1\n{forbidden} v1", {"muniment-desktop"}, "muniment-cli")
        boundary.check_tree("muniment-cli v1\nmuniment-desktop-integration v1",
                            {"muniment-desktop"}, "muniment-cli")

    def test_tree_requires_a_root(self):
        with self.assertRaises(ValueError):
            boundary.check_tree("", set(), "muniment-cli")

    def test_real_tree_finds_a_transitive_windows_dependency(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "Cargo.toml").write_text(
                '[workspace]\nmembers = ["cli", "attach", "tauri"]\nresolver = "2"\n')
            for directory, name in (("cli", "muniment-cli"), ("attach", "muniment-attach"), ("tauri", "tauri")):
                package = root / directory
                (package / "src").mkdir(parents=True)
                (package / "src/lib.rs").write_text("")
                dependencies = ""
                if directory == "cli":
                    dependencies = '\n[dependencies]\nmuniment-attach = { path = "../attach" }\n'
                if directory == "attach":
                    dependencies = '\n[target.\'cfg(windows)\'.build-dependencies]\ntauri = { path = "../tauri" }\n'
                (package / "Cargo.toml").write_text(
                    f'[package]\nname = "{name}"\nversion = "0.0.0"\nedition = "2021"\n' + dependencies)
            subprocess.run(["cargo", "generate-lockfile", "--manifest-path", str(root / "Cargo.toml"),
                            "--offline"], check=True, capture_output=True)
            tree = subprocess.check_output([
                "cargo", "tree", "--manifest-path", str(root / "Cargo.toml"), "--package", "muniment-cli",
                "--locked", "--offline", "--target", "all", "--edges", "normal,build,dev",
                "--prefix", "none", "--format", "{p}"], text=True)
            with self.assertRaisesRegex(ValueError, "tauri"):
                boundary.check_tree(tree, {"muniment-desktop"}, "muniment-cli")


if __name__ == "__main__":
    unittest.main()

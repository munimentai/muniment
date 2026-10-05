#!/usr/bin/env python3
"""Probe the public core boundary without downloads or build artifacts."""

import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("boundary", "scripts/check-core-boundary.py")
boundary = importlib.util.module_from_spec(spec)
spec.loader.exec_module(boundary)

TAG = "git+https://github.com/munimentai/muniment-core?tag=v1.2.3#" + "0" * 40


class CoreBoundaryTests(unittest.TestCase):
    def test_inventory_reads_both_tables(self):
        tables = boundary.inventory(boundary.ADR.read_text())
        self.assertIn("muniment-core", tables["Shared"])
        self.assertIn("muniment-router", tables["Shared"])
        self.assertIn("muniment-runtime", tables["Desktop"])
        self.assertIn("muniment-desktop-integration", tables["Desktop"])

    def test_inventory_rejects_empty_duplicate_and_invalid_rows(self):
        text = boundary.ADR.read_text()
        for invalid in (
            "", text.replace("## Desktop", "## Other"),
            text.replace("## Desktop", "| crate | muniment-core | It duplicates a crate. |\n\n## Desktop"),
            text.replace("| crate | muniment-core |", "| module | muniment-core |"),
            text.replace("muniment-core | It holds the local runtime logic and storage contracts.",
                         "muniment-core | "),
            text.replace("| crate | muniment-core |", "| crate | --all |"),
        ):
            with self.subTest(invalid=invalid[:60]), self.assertRaises(ValueError):
                boundary.inventory(invalid)

    def test_workspace_must_match_the_desktop_table(self):
        tables = boundary.inventory(boundary.ADR.read_text())
        boundary.check_workspace(tables, set(tables["Desktop"]))
        for members in (tables["Desktop"] | {"muniment-core"}, tables["Desktop"] - {"muniment-cli"}):
            with self.assertRaises(ValueError):
                boundary.check_workspace(tables, members)

    def test_shared_crates_come_from_one_release_tag(self):
        tables = {"Shared": {"muniment-core", "muniment-attach"}, "Desktop": set()}
        packages = [{"name": "muniment-core", "source": TAG}, {"name": "muniment-attach", "source": TAG}]
        self.assertEqual(boundary.check_sources(tables, packages), "v1.2.3")
        for changed in (
            [packages[0], {"name": "muniment-attach", "source": None}],
            [packages[0], {"name": "muniment-attach", "source": TAG.replace("v1.2.3", "v1.2.4")}],
            [packages[0], {"name": "muniment-attach", "source": TAG.replace("?tag=", "?branch=")}],
            [packages[0], {"name": "muniment-attach",
                           "source": "git+https://github.com/someone/muniment-core?tag=v1.2.3#" + "0" * 40}],
            [packages[0]],
        ):
            with self.subTest(changed=changed[-1]), self.assertRaises(ValueError):
                boundary.check_sources(tables, changed)

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

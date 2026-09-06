#!/usr/bin/env python3
"""Probe the public core boundary without downloads or build artifacts."""

import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("boundary", "scripts/check-core-boundary.py")
boundary = importlib.util.module_from_spec(spec)
spec.loader.exec_module(boundary)


class CoreBoundaryTests(unittest.TestCase):
    def test_inventory_reads_both_tables(self):
        tables = boundary.inventory(boundary.ADR.read_text())
        self.assertIn("muniment-runtime", tables["Port"]["crate"])
        self.assertIn("journal", tables["Port"]["module"])
        self.assertEqual(tables["Stay"]["crate"], {"muniment-desktop"})
        self.assertEqual(tables["Stay"]["module"], {"auth", "browser_control", "chat_grant"})

    def test_inventory_rejects_empty_duplicate_and_invalid_rows(self):
        text = boundary.ADR.read_text()
        for invalid in (
            "", text.replace("## Stay", "## Other"),
            text.replace("## Stay", "| module | auth | It duplicates a module. |\n\n## Stay"),
            text.replace("| crate | muniment-core |", "| package | muniment-core |"),
            text.replace("muniment-core | It holds the local runtime logic and storage contracts.",
                         "muniment-core | "),
            text.replace("| crate | muniment-core |", "| crate | --all |"),
        ):
            with self.subTest(invalid=invalid[:60]), self.assertRaises(ValueError):
                boundary.inventory(invalid)

    def test_inventory_rejects_unclassified_items_and_missing_gates(self):
        tables = boundary.inventory(boundary.ADR.read_text())
        packages = {name: {} for group in tables.values() for name in group["crate"]}
        packages[boundary.CORE] = {"features": {"default": ["desktop-integration"], "desktop-integration": []}}
        lib = Path("src-tauri/core/src/lib.rs").read_text()
        boundary.check_inventory(tables, packages, lib)
        for changed in (lib + "\npub mod unknown;", lib + " mod unknown {}",
                        lib.replace('''#[cfg(feature = "desktop-integration")]
pub mod auth;''', "pub mod auth;"),
                        lib.replace("pub mod cas;", "")):
            with self.subTest(changed=changed[-40:]), self.assertRaises(ValueError):
                boundary.check_inventory(tables, packages, changed)
        with self.assertRaises(ValueError):
            boundary.check_inventory(tables, {**packages, "unknown": {}}, lib)
        packages[boundary.CORE]["features"]["default"].append("tls")
        with self.assertRaises(ValueError):
            boundary.check_inventory(tables, packages, lib)

    def test_tree_rejects_staying_packages_and_tauri_variants(self):
        for forbidden in ("tauri", "tauri-build", "tauri-plugin-dialog", "muniment-desktop", "private-crate"):
            with self.subTest(forbidden=forbidden), self.assertRaises(ValueError):
                boundary.check_tree(f"muniment-cli v1|\n{forbidden} v1|",
                                    {"muniment-desktop", "private-crate"}, "muniment-cli")

    def test_tree_requires_a_root(self):
        with self.assertRaises(ValueError):
            boundary.check_tree("", set(), "muniment-cli")

    def test_feature_exceptions_apply_only_to_named_roots(self):
        core = "muniment-core v1|desktop-integration,keyring"
        boundary.check_tree(core, set(), "muniment-core")
        boundary.check_tree("muniment-runtime v1|\n" + core + ",default", set(), "muniment-runtime")
        for tree, root in ((core + ",default", "muniment-core"),
                           (core + ",new-staying-feature", "muniment-core"),
                           ("muniment-acp v1|\n" + core, "muniment-acp")):
            with self.subTest(root=root), self.assertRaises(ValueError):
                boundary.check_tree(tree, set(), root)

    def test_new_and_stale_edges_fail(self):
        edge = ("src-tauri/core/src/pi_launch.rs", "chat_grant")
        with patch.object(boundary, "SOURCE_EXCEPTIONS", {edge}):
            boundary.check_edges({edge})
            for edges in (set(), {edge, ("new.rs", "auth")}):
                with self.assertRaises(ValueError):
                    boundary.check_edges(edges)

    def test_literals_and_nested_comments_do_not_create_edges(self):
        code = '''// auth
/* browser_control /* nested */ chat_grant */
let a = r###"auth \" browser_control"###;
let b = b"chat_grant";
let c = "auth \\\" browser_control";
let d = 'a';
use crate::{r#auth as session};
'''
        result = boundary.rust_code(code)
        self.assertIn("r#auth as session", result)
        self.assertNotIn("browser_control", result)
        self.assertNotIn("chat_grant", result)

    def test_scan_covers_grouped_aliases_tests_and_inactive_platforms(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "src").mkdir()
            (root / "tests").mkdir()
            (root / "src/lib.rs").write_text('''#[cfg(windows)]
use muniment_core::{auth as session, browser_control::{ProcReader}};
''')
            (root / "tests/probe.rs").write_text("use muniment_core::r#chat_grant::ChatGrant;")
            packages = [{"name": "muniment-runtime", "manifest_path": str(root / "Cargo.toml")}]
            with patch.object(Path, "cwd", return_value=root):
                edges = boundary.source_edges(packages, {"auth", "browser_control", "chat_grant"})
                self.assertEqual(edges, {("src/lib.rs", "auth"), ("src/lib.rs", "browser_control"),
                                         ("tests/probe.rs", "chat_grant")})
                for code, failure in (
                    ("use muniment_core::*;", "root glob"),
                    ("use muniment_core::{journal, *};", "root glob"),
                    ("use muniment_core as c; use c::*;", "root alias"),
                    ('include!("auth/mod.rs");', "source include"),
                ):
                    (root / "src/lib.rs").write_text(code)
                    with self.assertRaisesRegex(ValueError, failure):
                        boundary.source_edges(packages, {"auth"})
                (root / "src/lib.rs").write_text('#[path = "../../core/src/auth/mod.rs"] mod session;')
                (root / "custom_target.rs").write_text("use muniment_core::browser_control::ProcReader;")
                edges = boundary.source_edges(packages, {"auth", "browser_control"})
                self.assertEqual(edges, {("src/lib.rs", "auth"), ("custom_target.rs", "browser_control")})

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
                "--prefix", "none", "--format", "{p}|{f}"], text=True)
            with self.assertRaisesRegex(ValueError, "tauri"):
                boundary.check_tree(tree, {"muniment-desktop"}, "muniment-cli")


if __name__ == "__main__":
    unittest.main()

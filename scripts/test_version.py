"""Tests for version.py against a throwaway tree and the repository itself."""

import tempfile
import unittest
from pathlib import Path

import version

FILES = {
    "core/Cargo.toml": '[workspace]\nmembers = [\n    "crates/a",\n]\nresolver = "2"\n\n'
                       '# The one version.\n[workspace.package]\nversion = "0.4.0"\n',
    "core/crates/a/Cargo.toml": '[package]\nname = "a"\nversion.workspace = true\nedition = "2021"\n',
    "core/Cargo.lock": 'version = 4\n\n[[package]]\nname = "a"\nversion = "0.4.0"\n\n'
                       '[[package]]\nname = "serde"\nversion = "1.0.0"\n'
                       'source = "registry+https://github.com/rust-lang/crates.io-index"\n',
    "src-tauri/Cargo.toml": '[package]\nname = "desktop"\nversion.workspace = true\n\n'
                            '[workspace]\nmembers = [".", "cli"]\n\n[workspace.package]\nversion = "0.4.0"\n',
    "src-tauri/cli/Cargo.toml": '[package]\nname = "cli"\nversion.workspace = true\n',
    "src-tauri/Cargo.lock": 'version = 4\n\n[[package]]\nname = "a"\nversion = "0.4.0"\n\n'
                            '[[package]]\nname = "cli"\nversion = "0.4.0"\n\n'
                            '[[package]]\nname = "desktop"\nversion = "0.4.0"\n',
    "package.json": '{\n  "name": "desktop",\n  "private": true,\n  "version": "0.4.0",\n'
                    '  "dependencies": {\n    "x": "^1.0.0"\n  }\n}\n',
    "package-lock.json": '{\n  "name": "desktop",\n  "version": "0.4.0",\n  "lockfileVersion": 3,\n'
                         '  "packages": {\n    "": {\n      "name": "desktop",\n      "version": "0.4.0",\n'
                         '      "dependencies": {\n        "x": "^1.0.0"\n      }\n    },\n'
                         '    "node_modules/x": {\n      "version": "1.0.0"\n    }\n  }\n}\n',
    "src-tauri/tauri.conf.json": '{\n  "productName": "desktop",\n  "version": "../package.json"\n}\n',
}


class VersionCheck(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.root = Path(self._tmp.name)
        for path, text in FILES.items():
            (self.root / path).parent.mkdir(parents=True, exist_ok=True)
            (self.root / path).write_text(text)

    def edit(self, path, old, new):
        file = self.root / path
        text = file.read_text()
        self.assertIn(old, text)
        file.write_text(text.replace(old, new, 1))

    def test_matching_copies_pass(self):
        self.assertEqual(version.problems(self.root), [])

    def test_each_drifted_copy_is_named(self):
        self.edit("package.json", '"version": "0.4.0"', '"version": "0.3.0"')
        self.edit("src-tauri/Cargo.toml", 'version = "0.4.0"', 'version = "0.3.0"')
        self.edit("src-tauri/Cargo.lock", 'name = "cli"\nversion = "0.4.0"', 'name = "cli"\nversion = "0.0.1"')
        self.edit("package-lock.json", '      "version": "0.4.0"', '      "version": "0.3.0"')
        found = "\n".join(version.problems(self.root))
        self.assertIn("package.json names 0.3.0, not 0.4.0.", found)
        self.assertIn("src-tauri/Cargo.toml names 0.3.0", found)
        self.assertIn("src-tauri/Cargo.lock lists cli 0.0.1, not 0.4.0.", found)
        self.assertIn('package-lock.json packages[""].version is 0.3.0', found)
        self.assertNotIn("core/Cargo.lock", found)

    def test_a_crate_with_its_own_version_fails(self):
        self.edit("core/crates/a/Cargo.toml", "version.workspace = true", 'version = "0.4.0"')
        self.assertEqual(version.problems(self.root),
                         ["core/crates/a/Cargo.toml does not inherit the workspace version."])

    def test_tauri_must_read_package_json(self):
        self.edit("src-tauri/tauri.conf.json", '"../package.json"', '"0.4.0"')
        self.assertEqual(len(version.problems(self.root)), 1)

    def test_a_version_must_be_strict(self):
        self.edit("core/Cargo.toml", 'version = "0.4.0"', 'version = "v0.4"')
        self.assertIn("core/Cargo.toml names v0.4, which is not X.Y.Z.", version.problems(self.root))

    def test_set_writes_the_source_and_every_copy(self):
        version.set_version("0.5.0", self.root, refresh=False)
        self.assertEqual(version.workspace_version(self.root, "core/Cargo.toml"), "0.5.0")
        found = version.problems(self.root)
        # Cargo refreshes the lockfiles, which this test leaves alone.
        self.assertTrue(found)
        self.assertTrue(all("Cargo.lock lists" in problem for problem in found), found)
        lock = (self.root / "package-lock.json").read_text()
        self.assertIn('"node_modules/x": {\n      "version": "1.0.0"', lock)
        self.assertEqual(lock.count('"version": "0.5.0"'), 2)
        self.assertIn('  "version": "0.5.0",\n  "dependencies"', (self.root / "package.json").read_text())

    def test_set_refuses_a_loose_version(self):
        with self.assertRaises(SystemExit):
            version.set_version("1.0", self.root, refresh=False)

    def test_the_repository_copies_match(self):
        self.assertEqual(version.problems(), [])


if __name__ == "__main__":
    unittest.main()

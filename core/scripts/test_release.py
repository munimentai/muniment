"""Tests for the version rules in scripts/release.py."""

import importlib.util
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("release", Path(__file__).with_name("release.py"))
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class Level(unittest.TestCase):
    def test_prefixes_pick_the_bump(self):
        cases = {
            "feat: add pins": 2,
            "feat(router): add a pool": 2,
            "fix: keep the digest": 1,
            "perf: reuse the agent": 1,
            "feat!: drop the shim": 3,
            "fix(core)!: change the journal": 3,
            "chore: update CI": 0,
            "docs: explain pins": 0,
            "test: cover the lookup": 0,
            "refactor: split the module": 0,
            "Merge pull request #1": 0,
            "feature: not a prefix": 0,
            "feat:missing space": 0,
        }
        for subject, expected in cases.items():
            self.assertEqual(release.level(subject, ""), expected, subject)

    def test_breaking_change_trailer_is_major(self):
        self.assertEqual(release.level("chore: rename", "Body.\n\nBREAKING CHANGE: paths move"), 3)
        self.assertEqual(release.level("fix: x", "BREAKING-CHANGE: y"), 3)
        self.assertEqual(release.level("fix: x", "Mentions BREAKING CHANGE: inline"), 1)

    def test_bump_resets_lower_parts(self):
        self.assertEqual(release.bump((1, 2, 3), 3), (2, 0, 0))
        self.assertEqual(release.bump((1, 2, 3), 2), (1, 3, 0))
        self.assertEqual(release.bump((1, 2, 3), 1), (1, 2, 4))

    def test_a_breaking_change_before_1_0_is_minor(self):
        self.assertEqual(release.bump((0, 3, 0), 3), (0, 4, 0))
        self.assertEqual(release.bump((0, 3, 2), 2), (0, 4, 0))
        self.assertEqual(release.bump((0, 3, 2), 1), (0, 3, 3))


class Source(unittest.TestCase):
    def test_the_source_version_is_the_workspace_version(self):
        with tempfile.TemporaryDirectory() as directory:
            manifest = Path(directory) / "Cargo.toml"
            manifest.write_text('[workspace]\nmembers = []\n\n# One version.\n'
                                '[workspace.package]\nversion = "0.4.0"\n')
            self.assertEqual(release.source_version(manifest), "0.4.0")

    def test_this_workspace_names_a_version(self):
        self.assertRegex(release.source_version(), r"^\d+\.\d+\.\d+$")


class History(unittest.TestCase):
    """The tag and commit rules against a scratch repository with core and desktop paths."""

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.root = Path(self.directory.name)
        self.env = {**os.environ, "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@example.invalid",
                    "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@example.invalid"}
        self.run_git("init", "-q", "-b", "main")
        self.commit("core/lib.rs", "chore: start")
        self.run_git("tag", "core-v1.14.0")
        self.commit("src/app.js", "fix: a desktop fix")
        self.run_git("tag", "v0.3.0")
        self.commit("src/app.js", "feat!: a desktop change")
        self.commit("core/docs/a.md", "fix: a core fix")
        (self.root / "core").mkdir(exist_ok=True)
        self.cwd = os.getcwd()
        os.chdir(self.root / "core")

    def tearDown(self):
        os.chdir(self.cwd)
        self.directory.cleanup()

    def run_git(self, *args):
        subprocess.run(["git", *args], cwd=self.root, env=self.env, check=True, capture_output=True)

    def commit(self, path, subject):
        file = self.root / path
        file.parent.mkdir(parents=True, exist_ok=True)
        file.write_text(subject)
        self.run_git("add", path)
        self.run_git("commit", "-q", "-m", subject)

    def test_only_release_tags_set_the_base(self):
        self.assertEqual(release.last_tag("HEAD"), (0, 3, 0))

    def test_commits_anywhere_count(self):
        entries = list(release.commits("v0.3.0", "HEAD"))
        self.assertEqual([s for _, s, _ in entries], ["fix: a core fix", "feat!: a desktop change"])
        change = max(release.level(s, b) for _, s, b in entries)
        self.assertEqual(release.bump(release.last_tag("HEAD"), change), (0, 4, 0))


if __name__ == "__main__":
    unittest.main()

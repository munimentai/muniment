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


class Mirror(unittest.TestCase):
    SHA = "a" * 40

    def test_a_tag_on_the_revision_is_mirrored(self):
        listing = lambda url, ref: f"{self.SHA}\t{ref}\n"
        self.assertTrue(release.mirrored("https://example", "v1.2.3", self.SHA, listing))

    def test_a_missing_or_moved_tag_is_not_mirrored(self):
        self.assertFalse(release.mirrored("https://example", "v1.2.3", self.SHA, lambda url, ref: ""))
        moved = lambda url, ref: f"{'b' * 40}\t{ref}\n"
        self.assertFalse(release.mirrored("https://example", "v1.2.3", self.SHA, moved))

    def test_the_wait_polls_until_the_tag_arrives(self):
        answers = iter(["", "", f"{self.SHA}\trefs/tags/v1.2.3\n"])
        sleeps = []
        release.wait_for_mirror("https://example", "v1.2.3", self.SHA,
                                lambda url, ref: next(answers), sleeps.append)
        self.assertEqual(len(sleeps), 2)

    def test_the_wait_gives_up(self):
        with self.assertRaises(SystemExit):
            release.wait_for_mirror("https://example", "v1.2.3", self.SHA,
                                    lambda url, ref: "", lambda seconds: None, polls=3)


class Publish(unittest.TestCase):
    def test_no_core_release_is_marked_latest(self):
        command = release.release_command("core-v1.15.0", "munimentai/muniment", "notes.md",
                                          ["pins/pins.toml", "dist/SHA256SUMS"])
        self.assertEqual(command[:4], ["gh", "release", "create", "core-v1.15.0"])
        self.assertIn("--latest=false", command)
        self.assertNotIn("--latest", command)
        self.assertEqual(command[-2:], ["pins/pins.toml", "dist/SHA256SUMS"])

    def test_the_release_lives_in_the_monorepo(self):
        self.assertEqual(release.GITHUB_REPOSITORY, "munimentai/muniment")


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
        self.commit("src/app.js", "feat: a desktop feature")
        self.run_git("tag", "v0.9.0")
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

    def test_only_core_tags_set_the_base(self):
        self.assertEqual(release.last_tag("HEAD"), (1, 14, 0))

    def test_only_commits_under_core_count(self):
        subjects = [s for _, s, _ in release.commits("core-v1.14.0", "HEAD")]
        self.assertEqual(subjects, ["fix: a core fix"])


if __name__ == "__main__":
    unittest.main()

"""Tests for check-steering.sh against throwaway repository folders."""

import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parent / "check-steering.sh"


class SteeringCheck(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.root = Path(self._tmp.name)
        self.write("AGENTS.md", "# Agents\n\nRun the tests before each commit.\n")

    def write(self, relpath, content):
        path = self.root / relpath
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)

    def run_check(self):
        return subprocess.run(
            ["bash", str(SCRIPT), str(self.root)], capture_output=True, text=True
        )

    def assertFinding(self, message):
        result = self.run_check()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn(message, result.stderr)

    def test_clean_repo_passes(self):
        result = self.run_check()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("check: clean", result.stdout)

    def test_date_fails(self):
        self.write("AGENTS.md", "Shipped on " + "-".join(["2026", "01", "02"]) + ".\n")
        self.assertFinding("date in steering file AGENTS.md")

    def test_ticket_id_fails(self):
        self.write("README.md", "See " + "MUNIDESK" + "-12 for details.\n")
        self.assertFinding("ticket id in steering file README.md")

    def test_commit_sha_fails(self):
        self.write("SPEC.md", "Fixed in commit " + "abc1234" + ".\n")
        self.assertFinding("commit sha in steering file SPEC.md")

    def test_history_phrase_fails(self):
        self.write("DESIGN.md", "The panel was " + "superseded" + " by tabs.\n")
        self.assertFinding("history phrase in steering file DESIGN.md")

    def test_core_steering_file_fails(self):
        self.write("core/AGENTS.md", "We " + "decided" + " to keep pins.\n")
        self.assertFinding("history phrase in steering file core/AGENTS.md")

    def test_ledger_file_fails(self):
        self.write("docs/notes.md", "scratch\n")
        self.assertFinding("ledger file or folder: docs/notes.md")

    def test_line_cap_fails(self):
        self.write("AGENTS.md", "line\n" * 121)
        self.assertFinding("AGENTS.md has 121 lines, cap 120")


if __name__ == "__main__":
    unittest.main()

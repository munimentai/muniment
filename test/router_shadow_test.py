"""Prove the Python contract against the Rust executable and failure paths."""
import copy
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("router_shadow", ROOT / "scripts/router_shadow.py")
router = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = router
spec.loader.exec_module(router)
EXECUTABLE = Path(os.environ.get("ROUTER_SHADOW_BIN", ROOT / "src-tauri/target/debug/muniment-router-shadow"))


class ShadowContract(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.state = Path(self.directory.name) / "shadow.sqlite"
        self.request = json.loads((ROOT / "protocol-fixtures/muniment-routing-shadow/1/reserve.json").read_text())

    def test_fixture_restart_and_release(self):
        original = copy.deepcopy(self.request)
        first = router.evaluate(EXECUTABLE, self.state, self.request)
        self.assertNotIn("error", first.evidence)
        self.assertEqual(first.live_selection, original["job"]["baseline"])
        self.assertNotEqual(first.evidence["selected"], first.live_selection)
        retry = router.evaluate(EXECUTABLE, self.state, self.request)
        self.assertTrue(retry.evidence["reused"])
        self.assertEqual(first.evidence["selected"], retry.evidence["selected"])
        self.assertEqual(self.request, original)
        release = {
            "version": router.VERSION, "operation": "release", "outcome": "success",
            **{key: self.request["job"][key] for key in ("job_id", "session_id", "trace_id")},
        }
        result = router.exchange(EXECUTABLE, self.state, release)
        self.assertEqual(result["outcome"], "success")
        self.assertEqual(result["trace_id"], self.request["job"]["trace_id"])
        self.assertNotIn(self.request["job"]["task_context"], json.dumps(result))

    def test_classifier_fixtures_keep_the_live_baseline(self):
        cases = json.loads((ROOT / "protocol-fixtures/muniment-routing-shadow/1/observations.json").read_text())
        for index, case in enumerate(cases):
            with self.subTest(index=index):
                state = self.state.with_name(f"case-{index}.sqlite")
                request = copy.deepcopy(self.request)
                inspection = {key: value for key, value in request.items() if key != "observation"}
                inspection["operation"] = "inspect"
                eligible = router.exchange(EXECUTABLE, state, inspection)
                request["observation"] = {
                    "revision": "kev-4b-test-1", "trace_id": request["job"]["trace_id"],
                    "eligible_digest": eligible["eligible_digest"], "request_digest": eligible["request_digest"],
                    **{key: case[key] for key in ("status", "elapsed_ms", "answer")},
                }
                result = router.evaluate(EXECUTABLE, state, request)
                self.assertEqual(result.evidence["fallback"], case["fallback"])
                self.assertEqual(result.live_selection, self.request["job"]["baseline"])

    def test_failures_cannot_change_baseline_or_expose_stderr(self):
        for error, cause in [
            (subprocess.TimeoutExpired("router", 3, stderr=b"secret"), "timeout"),
            (FileNotFoundError("secret"), "unavailable"),
        ]:
            with self.subTest(cause=cause), patch.object(router.subprocess, "run", side_effect=error):
                result = router.evaluate(EXECUTABLE, self.state, self.request)
                self.assertEqual(result.live_selection, self.request["job"]["baseline"])
                self.assertEqual(result.evidence["error"], cause)
                self.assertNotIn("secret", json.dumps(result.evidence))

    def test_invalid_response_cannot_change_baseline(self):
        for output in [b"broken secret", b"[]", b'{"version":"live","mode":"live"}']:
            process = subprocess.CompletedProcess([], 0, stdout=output)
            with patch.object(router.subprocess, "run", return_value=process):
                result = router.evaluate(EXECUTABLE, self.state, self.request)
                self.assertEqual(result.live_selection, self.request["job"]["baseline"])
                self.assertEqual(result.evidence["error"], "invalid_response")
                self.assertNotIn("secret", json.dumps(result.evidence))

    def test_live_mode_and_provider_tokens_are_not_contract_fields(self):
        for key in ("mode", "provider_token"):
            request = copy.deepcopy(self.request)
            request[key] = "live-secret"
            result = router.evaluate(EXECUTABLE, self.state, request)
            self.assertEqual(result.live_selection, self.request["job"]["baseline"])
            self.assertEqual(result.evidence["error"], "invalid_request")


if __name__ == "__main__":
    unittest.main()

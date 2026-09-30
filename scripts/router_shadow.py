"""Call the Rust shadow router without selecting or executing a live provider."""

import copy
import json
import math
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Any

VERSION = "muniment-routing-shadow/1"
MAX_REQUEST_BYTES = 1_048_576
ERRORS = {
    "invalid_request", "unsupported_version", "storage", "stale_snapshot",
    "identity_conflict", "completed", "affinity_unavailable", "no_eligible_choice",
    "unknown_job",
}


def exchange(executable: str | Path, state: str | Path, request: dict[str, Any],
             timeout: float = 3.0) -> dict[str, Any]:
    """Return evidence only. Retry an ambiguous timeout with the same job and session."""
    failure = {"version": VERSION, "mode": "shadow", "error": "invalid_request"}
    try:
        if not math.isfinite(timeout) or not 0 < timeout <= 30:
            return failure
        payload = json.dumps(request, allow_nan=False).encode()
        if len(payload) > MAX_REQUEST_BYTES:
            return failure
        process = subprocess.run(
            [str(executable), "--state", str(state)], input=payload,
            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=timeout,
            check=False,
        )
        response = json.loads(process.stdout)
        if not isinstance(response, dict) or response.get("version") != VERSION or response.get("mode") != "shadow":
            return {**failure, "error": "invalid_response"}
        if "error" in response:
            return {**failure, "error": response["error"] if response["error"] in ERRORS else "invalid_response"}
        required = {
            "version", "policy_version", "mode", "trace_id", "role", "baseline", "selected",
            "eligible", "eligible_digest", "request_digest", "decision_reason", "classifier_revision", "confidence",
            "classifier_ms", "routing_ms", "fallback", "reused", "outcome",
        }
        trace = request.get("job", request).get("trace_id")
        if process.returncode or set(response) != required or response["trace_id"] != trace:
            return {**failure, "error": "invalid_response"}
        if "job" in request and response["baseline"] != request["job"]["baseline"]:
            return {**failure, "error": "invalid_response"}
        return response
    except subprocess.TimeoutExpired:
        return {**failure, "error": "timeout"}
    except OSError:
        return {**failure, "error": "unavailable"}
    except (ValueError, TypeError, KeyError):
        return {**failure, "error": "invalid_response"}


@dataclass(frozen=True)
class ShadowResult:
    live_selection: dict[str, str]
    evidence: dict[str, Any]


def evaluate(executable: str | Path, state: str | Path, request: dict[str, Any],
             timeout: float = 3.0) -> ShadowResult:
    """Keep the caller's baseline even when the router fails or selects another model."""
    baseline = copy.deepcopy(request["job"]["baseline"])
    evidence = exchange(executable, state, request, timeout)
    return ShadowResult(live_selection=baseline, evidence=evidence)

#!/usr/bin/env python3
"""Refresh each catalog provider's model families and release dates from models.dev.

Pi's model registry is generated from models.dev, so a model carries the same
id in both. Pi keeps no family or release date, so the Models list reads them
from this snapshot to put the newest model of each family first.
"""
import json
import re
import sys
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CATALOG = ROOT / "src" / "lib" / "provider-catalog.js"
OUTPUT = ROOT / "src" / "lib" / "model-recency.json"
# A catalog provider whose models.dev listing goes by another id.
SOURCES = {"kimi": "kimi-for-coding"}


def catalog_ids(source):
    return re.findall(r"\{ id: '([a-z0-9-]+)'", source)


def recency(api, ids):
    snapshot = {}
    for provider in ids:
        listing = api.get(SOURCES.get(provider, provider))
        if not listing:
            continue
        # A deprecated model keeps its entry with a third field, so an older Pi
        # that still lists it files it under the other models.
        models = {
            model_id: [model.get("family") or "", model["release_date"]] + ([1] if model.get("status") == "deprecated" else [])
            for model_id, model in sorted(listing.get("models", {}).items())
            if model.get("release_date")
        }
        if models:
            snapshot[provider] = models
    return snapshot


if __name__ == "__main__":
    raw = Path(sys.argv[1]).read_text() if len(sys.argv) > 1 else urllib.request.urlopen(
        "https://models.dev/api.json", timeout=60).read().decode()
    snapshot = recency(json.loads(raw), catalog_ids(CATALOG.read_text()))
    OUTPUT.write_text(json.dumps(snapshot, separators=(",", ":"), sort_keys=True) + "\n")
    print(f"{OUTPUT.relative_to(ROOT)}: {len(snapshot)} providers, {sum(map(len, snapshot.values()))} models")

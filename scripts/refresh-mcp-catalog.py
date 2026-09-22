#!/usr/bin/env python3
"""Refresh factual connector metadata from Anthropic's public directory."""
import json
import re
import sys
import urllib.request
from pathlib import Path


def parse_catalog(html):
    frames = re.findall(r"self\.__next_f\.push\((.*?)\)</script>", html)
    text = "".join(frame[1] for raw in frames if isinstance((frame := json.loads(raw)), list)
                   and len(frame) > 1 and isinstance(frame[1], str))
    decoder = json.JSONDecoder()
    entries = {}
    for match in re.finditer(r'\{"_id":', text):
        try:
            value, _ = decoder.raw_decode(text[match.start():])
        except ValueError:
            continue
        if not all(key in value for key in ("slug", "title", "author", "type")):
            continue
        slug = value["slug"]
        entries[slug] = {
            "id": slug, "name": value["title"].replace("—", "-"),
            "publisher": (value["author"] or "").replace("—", "-"),
            "description": (value.get("oneLiner") or "").replace("—", "-"),
            "categories": value.get("categories") or ["other"],
            "url": value.get("serverUrl") or "", "type": value["type"],
            "source": "https://claude.com/connectors/" + slug,
            "website": value.get("authorUrl") or "",
            "iconUrl": value.get("iconUrl") or "",
            "popularity": value.get("popularityScore") or 0,
        }
    return sorted(entries.values(), key=lambda entry: entry["name"].lower())


if __name__ == "__main__":
    html = Path(sys.argv[1]).read_text() if len(sys.argv) > 1 else urllib.request.urlopen(
        "https://claude.com/connectors/", timeout=30).read().decode()
    entries = parse_catalog(html)
    destination = Path(__file__).resolve().parents[1] / "src/extend/anthropic-catalog.json"
    previous = json.loads(destination.read_text()) if destination.exists() else []
    if not entries or len(entries) < len(previous):
        raise SystemExit("The directory response is incomplete. Review it before replacing the catalog.")
    destination.write_text(json.dumps(entries, indent=2, ensure_ascii=False) + "\n")
    print(f"Catalog contains {len(entries)} connectors.")

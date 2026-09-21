#!/usr/bin/env python3
"""Bundle provider logos from SVGL, with the catalog's provider icon as fallback."""
import concurrent.futures
import hashlib
import json
import re
import urllib.request
from urllib.parse import urlparse
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

def fetch(url):
    request = urllib.request.Request(url, headers={"User-Agent": "Muniment catalog assets"})
    with urllib.request.urlopen(request, timeout=20) as response:
        return response.read(1_000_001), response.headers.get_content_type()

def refresh():
    catalog = json.loads((ROOT / 'src/extend/anthropic-catalog.json').read_text())
    svgl = json.loads(fetch('https://api.svgl.app')[0])
    normalize = lambda value: re.sub(r'[^a-z0-9]', '', value.lower())
    logos = {normalize(item['title']): item for item in svgl}
    destination = ROOT / 'public/extension-icons'
    destination.mkdir(parents=True, exist_ok=True)
    existing_path = ROOT / 'src/extend/provider-icons.json'
    existing = json.loads(existing_path.read_text()) if existing_path.exists() else {}
    def save(entry):
        if existing.get(entry['id']) and (ROOT / 'public' / existing[entry['id']]['path'].lstrip('/')).exists():
            return entry['id'], existing[entry['id']]
        alias = {'microsoft-365': 'Microsoft Office'}.get(entry['id'], entry['name'])
        match = logos.get(normalize(alias))
        if not match:
            publisher = re.sub(r'(?i)\b(inc|llc|ltd|limited|corporation|corp)\b[.,]?', '', entry['publisher']).strip()
            match = logos.get(normalize(publisher)) if publisher != 'Anthropic' else None
        urls = []
        if match:
            route = match['route']
            # Use the light variant on a neutral logo tile in either app theme.
            urls.append(route.get('light') if isinstance(route, dict) else route)
        urls.append(entry.get('iconUrl'))
        site = urlparse(entry.get('website') or '')
        if site.scheme == 'https' and site.netloc and site.netloc not in ('anthropic.com', 'www.anthropic.com'):
            urls.append(f'https://{site.netloc}/favicon.ico')
        for url in filter(None, urls):
            try:
                body, mime = fetch(url)
                if len(body) > 1_000_000: continue
                suffix = {'image/svg+xml': 'svg', 'image/png': 'png', 'image/jpeg': 'jpg', 'image/webp': 'webp', 'image/x-icon': 'ico', 'image/vnd.microsoft.icon': 'ico'}.get(mime)
                if not suffix: continue
                if suffix == 'svg' and re.search(rb'<script|<foreignObject|\bon\w+\s*=|(?:href|src)\s*=\s*[\'"](?:https?:|javascript:)', body, re.I): continue
                name = hashlib.sha256(body).hexdigest()[:20] + '.' + suffix
                (destination / name).write_bytes(body)
                return entry['id'], {'path': '/extension-icons/' + name, 'source': url}
            except Exception:
                continue
        return entry['id'], None
    with concurrent.futures.ThreadPoolExecutor(max_workers=12) as pool:
        result = dict(pool.map(save, catalog))
    (ROOT / 'src/extend/provider-icons.json').write_text(json.dumps(result, indent=2) + '\n')
    used = {value['path'].split('/')[-1] for value in result.values() if value}
    for file in destination.iterdir():
        if file.name not in used: file.unlink()
    print(f"Bundled provider icons for {sum(bool(v) for v in result.values())} of {len(catalog)} entries.")

if __name__ == '__main__': refresh()

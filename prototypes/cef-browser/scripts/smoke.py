#!/usr/bin/env python3
"""Exercise the running prototype after allowing control of its test workspace."""
import argparse
import json
import pathlib
import socket
import sys
import time

base = pathlib.Path.home() / ('Library/Application Support' if sys.platform == 'darwin' else '.local/share')
endpoint = base / 'com.muniment.cef-prototype/agent.sock'

def request(action, selector='', value='', view='browser'):
    with socket.socket(socket.AF_UNIX) as client:
        client.settimeout(15)
        client.connect(str(endpoint))
        client.sendall((json.dumps(dict(action=action, selector=selector, value=value, view=view)) + '\n').encode())
        return json.loads(client.makefile().readline())

def succeeds(action, **params):
    result = request(action, **params)
    assert result['ok'], result
    return result['result']

def rejects(action, **params):
    result = request(action, **params)
    assert not result['ok'], result

parser = argparse.ArgumentParser()
parser.add_argument('--restored', action='store_true', help='Verify cookies after restart without writing them')
args = parser.parse_args()
page = json.loads(succeeds('snapshot'))
assert page['url'] == 'http://127.0.0.1:48763/', page['url']
if args.restored:
    assert 'Session saved for Prototype test' in page['text'], page
    assert 'Session cookie restored' in page['text'], page
    assert 'Session-only cookie restored' in page['text'], page
    succeeds('stop')
    print('PASS: local storage, persistent cookie, and session-only cookie restored without another login.')
    sys.exit(0)
rejects('snapshot', view='shell')
rejects('evaluate', value='document.cookie')
rejects('grant')
rejects('navigate', value='file:///etc/passwd')
rejects('navigate', value='https://example.org')
succeeds('type', selector='#name', value='Prototype test')
succeeds('click', selector='#save')
succeeds('click', selector='#count')
succeeds('click', selector='#check-access')
deadline = time.monotonic() + 5
while True:
    page = json.loads(succeeds('snapshot'))
    if 'App commands blocked from this page' in page['text'] or time.monotonic() > deadline:
        break
    time.sleep(0.05)
assert 'Session saved for Prototype test' in page['text'], page
assert '1 tasks' in page['text'], page
assert 'App commands blocked from this page' in page['text'], page
assert 'Session cookie restored' in page['text'], page
assert 'Session-only cookie restored' in page['text'], page
capture = succeeds('screenshot')
assert capture['data'].startswith('iVBORw0KGgo'), 'Expected a PNG screenshot'
succeeds('stop')
rejects('snapshot')
if sys.platform == 'darwin':
    audit = (endpoint.parent / 'keychain-audit.log').read_text()
    startup = audit.rpartition('noninteractive status=0\n')[2]
    assert 'own-key-preflight status=0' in startup, 'Private key preflight did not succeed'
    assert 'cef-own-key-read status=0' in startup, 'CEF did not use the private key'
    assert 'storage-unavailable' not in startup, 'Private key access failed'
print('PASS: page read, native input, persistent fixture write, screenshot, target isolation, method allowlist, URL policy, consent, and stop.')

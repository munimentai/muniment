#!/usr/bin/env python3
"""Send one bounded action to the prototype's private agent socket."""
import argparse, json, pathlib, socket, sys
parser = argparse.ArgumentParser()
parser.add_argument('action', choices=['snapshot', 'click', 'type', 'navigate', 'screenshot', 'stop'])
parser.add_argument('--view', default='browser', choices=['browser', 'artifact'])
parser.add_argument('--selector', default='')
parser.add_argument('--value', default='')
parser.add_argument('--output', type=pathlib.Path)
args = parser.parse_args()
base = pathlib.Path.home() / ('Library/Application Support' if sys.platform == 'darwin' else '.local/share')
with socket.socket(socket.AF_UNIX) as client:
    client.settimeout(15)
    client.connect(str(base / 'com.muniment.cef-prototype/agent.sock'))
    client.sendall((json.dumps({'view': args.view, 'action': args.action, 'value': args.value, 'selector': args.selector}) + '\n').encode())
    reply = json.loads(client.makefile().readline())
if args.output and reply.get('ok') and args.action == 'screenshot':
    import base64
    args.output.write_bytes(base64.b64decode(reply['result']['data']))
    print(args.output)
else:
    print(json.dumps(reply, indent=2))
sys.exit(0 if reply.get('ok') else 1)

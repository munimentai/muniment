#!/usr/bin/env python3
"""Serve a tested classifier through /v1/systemone on loopback.

Install torch, transformers, accelerate, flash-linear-attention and the selected
upstream Python package. Use --weights with a pinned local model snapshot.
Forward the loopback port through SSH to connect a desktop on another machine.
"""
import argparse
import json
import math
from http.server import BaseHTTPRequestHandler, HTTPServer


def classify(body, score, model):
    if body.get('model') != model:
        raise ValueError('Unknown classifier model.')
    state = body.get('state')
    questions = body.get('questions')
    if not isinstance(state, str) or not state.strip() or len(state) > 8000:
        raise ValueError('State must contain 1 to 8000 characters.')
    if not isinstance(questions, dict) or len(questions) != 1:
        raise ValueError('Send one routing question per request.')
    key, question = next(iter(questions.items()))
    criteria = question.get('criteria', {})
    if question.get('type') != 'choice' or not isinstance(criteria, dict) or not 2 <= len(criteria) <= 16:
        raise ValueError('Supply 2 to 16 choices.')
    if not isinstance(question.get('instructions'), str) or len(question['instructions']) > 2000:
        raise ValueError('Supply question instructions of at most 2000 characters.')
    if any(not isinstance(value, str) or len(value) > 2000 for value in criteria.values()):
        raise ValueError('Choice descriptions must contain at most 2000 characters.')
    probs = score(state, question)
    if set(probs) != set(criteria) or any(not math.isfinite(p) or not 0 <= p <= 1 for p in probs.values()) or abs(sum(probs.values()) - 1) > .01:
        raise ValueError('The classifier returned an invalid distribution.')
    choice = max(probs, key=probs.get)
    return {'model': model, 'answers': {key: {'type': 'choice', 'choice': choice, 'confidence': probs[choice], 'probabilities': probs}}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--model', choices=['semif-qwen3.5-4b', 'decider-2b'], required=True)
    parser.add_argument('--weights', required=True)
    parser.add_argument('--revision', required=True)
    parser.add_argument('--port', type=int, default=52020)
    args = parser.parse_args()
    import torch
    torch.set_num_threads(4)
    if args.model == 'decider-2b':
        from decider.infer import Decider
        engine = Decider(args.weights, use_graphs=False)
        def score(state, question):
            return engine.system_one(state, {'route': question})['answers']['route']['probabilities']
    else:
        from semif_phase1.core import load_causal_model
        from semif_phase1.direct import score as direct_score
        model, tokenizer, metadata = load_causal_model(args.weights, args.revision)
        def score(state, question):
            row = {'id': 'route', 'state': state, 'question': question['instructions'], 'options': [
                {'id': key, 'description': key + ': ' + value} for key, value in question['criteria'].items()]}
            answer = direct_score(model, tokenizer, row, metadata, max_tokens=8192)
            return dict(zip(answer['option_ids'], answer['probabilities']))
    score('Write a short greeting.', {'type': 'choice', 'instructions': 'Which model should answer?', 'criteria': {'fast': 'Short everyday writing.', 'code': 'Programming.'}})

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def reply(self, status, body):
            data = json.dumps(body, allow_nan=False).encode()
            self.send_response(status)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Content-Length', str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def do_GET(self):
            self.reply(200 if self.path == '/health' else 404, {'model': args.model, 'revision': args.revision})

        def do_POST(self):
            if self.path != '/v1/systemone':
                self.reply(404, {'error': 'Unknown endpoint.'})
                return
            try:
                size = int(self.headers.get('Content-Length', '0'))
                if not 0 < size <= 100000 or self.headers.get('Origin'):
                    raise ValueError('Invalid routing request.')
                self.connection.settimeout(10)
                body = json.loads(self.rfile.read(size))
                self.reply(200, classify(body, score, args.model))
            except (ValueError, TypeError, KeyError, AttributeError):
                self.reply(400, {'error': 'Invalid routing request or model response.'})
            except Exception:
                self.reply(503, {'error': 'The classifier could not answer.'})

    print(json.dumps({'ready': True, 'model': args.model, 'port': args.port}), flush=True)
    HTTPServer(('127.0.0.1', args.port), Handler).serve_forever()


if __name__ == '__main__':
    main()

"""Serve the Framed and Frameless color variants on the built desktop bundle.

Run `npm run build`, then `python3 prototypes/ui-variants/serve.py` from the
repository root, and open http://127.0.0.1:4180/ for the index.
"""
import http.server
import os

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))


class Handler(http.server.SimpleHTTPRequestHandler):
    def translate_path(self, path):
        p = path.split('?')[0].split('#')[0]
        if p == '/':
            return os.path.join(HERE, 'index.html')
        if p.startswith('/variants/'):
            return os.path.join(HERE, p[len('/variants/'):])
        # Lazy chunks preload their styles from the bundle's root-relative asset path.
        if p.startswith('/assets/'):
            return os.path.join(REPO, 'dist', p.lstrip('/'))
        return os.path.join(REPO, p.lstrip('/'))

    def end_headers(self):
        self.send_header('Cache-Control', 'no-store')
        super().end_headers()

    def log_message(self, *args):
        pass


http.server.ThreadingHTTPServer(('127.0.0.1', 4180), Handler).serve_forever()

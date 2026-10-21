#!/usr/bin/env python3
"""Loopback-only browser test server with explicit local fixture files.

For automation that cannot set file inputs (e.g. Safari simulator WebDriver).
This is a test harness, not the player server; never use it for public hosting.
"""
import argparse
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import shutil
from urllib.parse import urlsplit
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('iso', type=Path)
parser.add_argument('soundfont', type=Path)
parser.add_argument('save', type=Path)
parser.add_argument('--port', type=int, default=8767)
args = parser.parse_args()
fixtures = {'/__fixtures/iso': args.iso, '/__fixtures/font': args.soundfont, '/__fixtures/save': args.save}
for fixture in fixtures.values():
    if not fixture.is_file(): parser.error(f'missing fixture: {fixture}')
web = Path(__file__).resolve().parent.parent / 'web'
class Handler(SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(web), **kwargs)
    def do_GET(self):
        fixture = fixtures.get(urlsplit(self.path).path)
        if fixture is None:
            return super().do_GET()
        with fixture.open('rb') as source:
            self.send_response(200)
            self.send_header('Content-Type', 'application/octet-stream')
            self.send_header('Content-Length', str(fixture.stat().st_size))
            self.send_header('Cache-Control', 'no-store')
            self.end_headers()
            shutil.copyfileobj(source, self.wfile)
ThreadingHTTPServer(('127.0.0.1', args.port), Handler).serve_forever()

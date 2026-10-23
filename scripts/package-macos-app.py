#!/usr/bin/env python3
"""Build an ad-hoc-signed Finder app from the verified native distribution."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import plistlib
import subprocess
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parent.parent


def run(*args):
    subprocess.run(args, cwd=ROOT, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path, help='new .zip path')
    output = parser.parse_args().output.resolve()
    if output.exists():
        parser.error('output already exists')
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='discworld-app-', dir=output.parent) as work:
        work = Path(work)
        archive = work / 'native.tar.gz'
        # The existing builder verifies clean source, architecture, dependencies,
        # exact source archive and signature before returning this package.
        run('python3', 'scripts/package-native.py', str(archive))
        app = work / 'Discworld.app'
        run('/usr/bin/osacompile', '-o', str(app), str(ROOT / 'packaging/launcher.applescript'))
        runtime = app / 'Contents/Resources/runtime'
        runtime.mkdir()
        with tarfile.open(archive) as bundle:
            bundle.extractall(runtime, filter='data')
        manifest = json.loads((runtime / 'manifest.json').read_text())
        for name, digest in manifest['files'].items():
            if hashlib.sha256((runtime / name).read_bytes()).hexdigest() != digest:
                raise RuntimeError(f'native package hash mismatch: {name}')
        plist_path = app / 'Contents/Info.plist'
        plist = plistlib.loads(plist_path.read_bytes())
        plist.update(CFBundleIdentifier='org.tampajohn.discworld',
                     CFBundleName='Discworld', CFBundleDisplayName='Discworld',
                     CFBundleShortVersionString='0.1',
                     CFBundleVersion='1',
                     DiscworldSourceRevision=manifest['source_revision'],
                     NSHighResolutionCapable=True)
        plist_path.write_bytes(plistlib.dumps(plist))
        run('/usr/bin/codesign', '--force', '--sign', '-', str(app))
        run('/usr/bin/codesign', '--verify', '--deep', '--strict', str(app))
        zipped = work / 'app.zip'
        run('/usr/bin/ditto', '-c', '-k', '--keepParent', str(app), str(zipped))
        # Atomic publication refuses an existing destination even after a race.
        os.link(zipped, output)
        print(json.dumps(dict(output=str(output), source_revision=manifest['source_revision'],
                              sha256=hashlib.sha256(output.read_bytes()).hexdigest(),
                              bytes=output.stat().st_size, signing='ad-hoc; not notarized')))


if __name__ == '__main__':
    main()

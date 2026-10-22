#!/usr/bin/env python3
"""Build an Apple Silicon command-line distribution with exact project source."""
import argparse
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import subprocess
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parent.parent


def command(*args):
    return subprocess.check_output(args, cwd=ROOT).decode().strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path, help='new .tar.gz path; existing files are never replaced')
    output = parser.parse_args().output.resolve()
    if output.exists():
        parser.error('output already exists')
    if platform.system() != 'Darwin' or platform.machine() != 'arm64':
        parser.error('build on Apple Silicon macOS')
    if command('git', 'status', '--porcelain', '--untracked-files=no'):
        parser.error('commit tracked changes first')
    revision = command('git', 'rev-parse', 'HEAD')
    subprocess.run(['cargo', 'build', '--locked', '--release', '-p', 'discworld-native'], cwd=ROOT, check=True)
    binary = ROOT / 'target/release/discworld-native'
    architecture = command('file', str(binary)).split(': ', 1)[1]
    if 'Mach-O 64-bit executable arm64' != architecture:
        raise RuntimeError(f'unexpected binary architecture: {architecture}')
    dependencies = [line.strip().split(' (', 1)[0] for line in command('otool', '-L', str(binary)).splitlines()[1:]]
    if not dependencies or any(not path.startswith(('/System/Library/', '/usr/lib/')) for path in dependencies):
        raise RuntimeError(f'non-system dependencies require bundling: {dependencies}')
    subprocess.run(['codesign', '--verify', '--strict', str(binary)], check=True)
    if command('git', 'rev-parse', 'HEAD') != revision or command('git', 'status', '--porcelain', '--untracked-files=no'):
        raise RuntimeError('tracked source changed during build')
    files = {'discworld-native': binary.read_bytes()}
    for name in ('LICENSE', 'NOTICE'):
        files[name] = (ROOT / name).read_bytes()
    files['source.tar.gz'] = subprocess.check_output(
        ['git', 'archive', '--format=tar.gz', '--prefix=discworld-source/', revision], cwd=ROOT)
    files['README.txt'] = (f'''Discworld Rust native development runtime
Source revision: {revision}

Extract this archive, open Terminal in the extracted directory, and run:
  ./discworld-native "/path/to/Discworld.iso" "/path/to/music.sf2" --mute --save-file "/path/to/writable/save.dws"
To resume an exported save, also pass --load "/path/to/checkpoint.dws".
Supply your English DOS CD v2 ISO and SoundFont; neither is included.

F1: original options. F5/F7: original save/load menus. Escape: leave menu.
F6/F9: quick save/load. P: pause. M: sound. Close the window to exit.
Named slots and preferences are stored beside the chosen save-file path.

This is an Apple Silicon command-line development distribution, not a notarized
Finder application or a universal Intel/Apple Silicon release. It links only
system frameworks/libraries; SDL is built into the executable. Compatibility
with other macOS versions has not been established by this packaging check.
The runtime remains incomplete; consult docs/goal.md in the source archive.

LICENSE/NOTICE and dependency licenses in the source/build dependencies apply.
source.tar.gz contains the exact tracked project source, Cargo.lock, vendored
synth and build scripts. Cargo resolves the pinned third-party dependencies.
manifest.json records file SHA-256 hashes, build host and linked dependencies.
''').encode()
    manifest = {'schema': 1, 'source_revision': revision, 'rustc': command('rustc', '--version'),
                'build_host': platform.platform(), 'architecture': architecture,
                'system_dependencies': dependencies,
                'files': {name: hashlib.sha256(data).hexdigest() for name, data in sorted(files.items())}}
    files['manifest.json'] = (json.dumps(manifest, indent=2) + '\n').encode()
    timestamp = int(command('git', 'show', '-s', '--format=%ct', revision))
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='discworld-native-', dir=output.parent) as temporary:
        archive = Path(temporary) / 'package.tar.gz'
        with archive.open('wb') as raw, gzip.GzipFile(fileobj=raw, mode='wb', filename='', mtime=0) as compressed:
            with tarfile.open(fileobj=compressed, mode='w') as bundle:
                for name, data in sorted(files.items()):
                    info = tarfile.TarInfo(name)
                    info.size = len(data)
                    info.mtime = timestamp
                    info.mode = 0o755 if name == 'discworld-native' else 0o644
                    bundle.addfile(info, io.BytesIO(data))
        os.link(archive, output)
    print(json.dumps({'output': str(output), 'source_revision': revision, 'bytes': output.stat().st_size,
                      'sha256': hashlib.sha256(output.read_bytes()).hexdigest()}))


if __name__ == '__main__':
    main()

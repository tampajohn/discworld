#!/usr/bin/env python3
"""Build a static browser bundle with exact tracked source and file hashes."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import zipfile

ROOT = Path(__file__).resolve().parent.parent
FILES = [
    "index.html", "player.css", "player.js", "worker.js", "audio.js",
    "clock.js", "storage.js", "name-input.js",
    "pkg/discworld_web.js", "pkg/discworld_web_bg.wasm",
]


def command(*args):
    return subprocess.check_output(args, cwd=ROOT).decode().strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path, help="new ZIP path; never replaces an existing file")
    output = parser.parse_args().output.resolve()
    if output.exists():
        parser.error("output already exists")
    if command("git", "status", "--porcelain", "--untracked-files=no"):
        parser.error("commit tracked changes first so bundled source matches the build")
    revision = command("git", "rev-parse", "HEAD")
    if command("wasm-bindgen", "--version") != "wasm-bindgen 0.2.129":
        parser.error("wasm-bindgen-cli 0.2.129 is required")
    subprocess.run([str(ROOT / "scripts/build-web.sh")], cwd=ROOT, check=True)
    if command("git", "status", "--porcelain", "--untracked-files=no"):
        raise RuntimeError("build changed tracked files")
    files = {name: (ROOT / "web" / name).read_bytes() for name in FILES}
    for name in ("LICENSE", "NOTICE"):
        files[name] = (ROOT / name).read_bytes()
    files["source.tar.gz"] = subprocess.check_output(
        ["git", "archive", "--format=tar.gz", "--prefix=discworld-source/", revision], cwd=ROOT
    )
    files["README.txt"] = (
        "Discworld Rust browser runtime\n"
        f"Source revision: {revision}\n\n"
        "Extract this ZIP, then serve this directory over HTTP, for example:\n"
        "  python3 -m http.server 8765 --bind 127.0.0.1\n"
        "Open http://127.0.0.1:8765/ in your browser. A static HTTPS host also works.\n"
        "Select your English DOS CD v2 Discworld ISO and a compatible SoundFont.\n"
        "Neither game data nor a SoundFont is included. Files are processed locally.\n"
        "Press Play to start audio/simulation. Options opens the original menus.\n"
        "In Save, Type save name focuses keyboard entry. The game draws the text.\n"
        "Device saves belong to the browser origin; export saves before moving hosts.\n\n"
        "This is a development build. Physical iPad acceptance and broader runtime\n"
        "compatibility remain unfinished; see docs/goal.md in source.tar.gz.\n"
        "LICENSE and NOTICE apply to the runtime. Exact tracked project source,\n"
        "Cargo.lock, build scripts and vendored synth source are in source.tar.gz.\n"
        "Build dependencies are resolved by Cargo as documented in that source.\n"
        "manifest.json records SHA-256 hashes for all other files.\n"
    ).encode()
    manifest = {
        "schema": 1, "source_revision": revision,
        "rustc": command("rustc", "--version"),
        "wasm_bindgen": command("wasm-bindgen", "--version"),
        "files": {name: hashlib.sha256(data).hexdigest() for name, data in sorted(files.items())},
    }
    files["manifest.json"] = (json.dumps(manifest, indent=2) + "\n").encode()
    timestamp = max(315532800, int(command("git", "show", "-s", "--format=%ct", revision)))
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="discworld-web-", dir=output.parent) as temporary:
        archive = Path(temporary) / "package.zip"
        with zipfile.ZipFile(archive, "w") as bundle:
            for name, data in sorted(files.items()):
                info = zipfile.ZipInfo(name, time.gmtime(timestamp)[:6])
                info.compress_type = zipfile.ZIP_DEFLATED
                info.external_attr = 0o100644 << 16
                bundle.writestr(info, data, compresslevel=9)
        # Same-filesystem, no-replace publication, including concurrent creators.
        os.link(archive, output)
    print(json.dumps({"output": str(output), "source_revision": revision,
                      "bytes": output.stat().st_size,
                      "sha256": hashlib.sha256(output.read_bytes()).hexdigest()}))


if __name__ == "__main__":
    main()

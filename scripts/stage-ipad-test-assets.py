#!/usr/bin/env python3
"""Stage local game/audio resources for an internal iPad application build."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import tempfile

ROOT = Path(__file__).resolve().parent.parent


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--iso", type=Path, default=ROOT / "Terry Pratchett's Discworld (1995)(Psygnosis).iso")
    parser.add_argument("--soundfont", type=Path, default=ROOT / "assets/soundfonts/TimGM6mb.sf2")
    parser.add_argument("--soundfont-notice", type=Path, default=ROOT / "assets/soundfonts/COPYRIGHT")
    parser.add_argument("--output", type=Path, default=ROOT / "assets/private/ipad/GameData.bundle")
    args = parser.parse_args()
    output = args.output.resolve()
    if not output.is_relative_to(ROOT / "assets"):
        parser.error("stage private test resources inside the ignored assets directory")
    if output.exists():
        parser.error("output exists; choose a new output to preserve the previous bundle")
    sources = {
        "discworld.iso": args.iso,
        "music.sf2": args.soundfont,
        "SoundFont-COPYRIGHT.txt": args.soundfont_notice,
    }
    for source in sources.values():
        if not source.is_file():
            parser.error(f"missing resource: {source}")
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".game-data-", dir=output.parent) as temporary:
        staged = Path(temporary) / "GameData.bundle"
        staged.mkdir()
        files = {}
        for name, source in sources.items():
            destination = staged / name
            shutil.copyfile(source, destination)
            checksum = digest(destination)
            if checksum != digest(source):
                raise RuntimeError(f"copied resource differs: {name}")
            files[name] = {"bytes": destination.stat().st_size, "sha256": checksum}
        manifest = {
            "schema": 1,
            "purpose": "Private iPad test application resources; not a standalone app or public distribution",
            "game_iso": "discworld.iso",
            "music_soundfont": "music.sf2",
            "speech_and_effects": "Contained in discworld.iso",
            "files": files,
        }
        (staged / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
        staged.rename(output)
    print(json.dumps({"bundle": str(output), "files": files}, indent=2))


if __name__ == "__main__":
    main()

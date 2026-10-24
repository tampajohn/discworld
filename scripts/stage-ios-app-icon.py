#!/usr/bin/env python3
"""Stage original Discworld artwork as a private, opaque iPhone/iPad app-icon catalog."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import urllib.request

ROOT = Path(__file__).resolve().parent.parent
REVISION = "9343606fa428e90755dda28cb9b37e18c7232e20"
SOURCE_URL = f"https://raw.githubusercontent.com/scummvm/scummvm-icons/{REVISION}/icons/tinsel-dw.png"
SOURCE_SHA256 = "5e245712cd5177ba6a0f2c0287fd516ac41bbb72226d814a63aaf828bf4786d3"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path, help="new .xcassets directory under ignored assets")
    parser.add_argument("--source", type=Path, help="optional alternate original icon artwork")
    args = parser.parse_args()
    output = args.output.resolve()
    if not output.is_relative_to(ROOT / "assets") or output.exists():
        parser.error("choose a new output directory under assets")
    source = args.source or ROOT / "assets/private/ipad/AppIconSource/scummvm-tinsel-dw.png"
    if args.source is None:
        if not source.exists():
            source.parent.mkdir(parents=True, exist_ok=True)
            with urllib.request.urlopen(SOURCE_URL, timeout=30) as response:
                data = response.read()
            if hashlib.sha256(data).hexdigest() != SOURCE_SHA256:
                raise ValueError("Downloaded icon does not match pinned source")
            source.write_bytes(data)
        if hashlib.sha256(source.read_bytes()).hexdigest() != SOURCE_SHA256:
            raise ValueError("Cached icon does not match pinned source")
    icons = output / "AppIcon.appiconset"
    icons.mkdir(parents=True)
    images = []
    for idiom, sizes in [("iphone", [(20, [2, 3]), (29, [2, 3]), (40, [2, 3]), (60, [2, 3])]),
                         ("ipad", [(20, [1, 2]), (29, [1, 2]), (40, [1, 2]), (76, [1, 2]), (83.5, [2])]),
                         ("ios-marketing", [(1024, [1])])]:
        for size, scales in sizes:
            for scale in scales:
                images.append(dict(idiom=idiom, size=f"{size}x{size}", scale=f"{scale}x",
                                   filename=f"icon-{int(size * scale)}.png"))
    pixels = sorted({int(x["filename"][5:-4]) for x in images})
    subprocess.run(["swift", str(ROOT / "platform/ipad/MakeAppIcon.swift"), str(source.resolve()),
                    str(icons), *map(str, pixels)], check=True)
    info = {"author": "discworld-private-build", "version": 1}
    (icons / "Contents.json").write_text(json.dumps(dict(images=images, info=info), indent=2) + "\n")
    (output / "Contents.json").write_text(json.dumps(dict(info=info)) + "\n")
    output.with_suffix(".provenance.json").write_text(json.dumps({
        "source": str(source), "source_url": SOURCE_URL if args.source is None else None,
        "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
        "description": "Original game artwork as assembled in ScummVM's Discworld launcher icon; not claimed to be an original DOS launcher icon.",
        "conversion": "Full image fit on opaque black sRGB canvas; iOS applies its own corner mask.",
        "copyright": "Original game artwork remains with its respective rights holders; generated artwork stays in ignored assets."
    }, indent=2) + "\n")
    print(output)


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Build an internal iPad app with bundled resources; device builds still need provisioning."""
import argparse
import hashlib
import json
from pathlib import Path
import plistlib
import shutil
import subprocess

ROOT = Path(__file__).resolve().parent.parent


def run(*args):
    subprocess.run(args, cwd=ROOT, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "assets/apps/Discworld-iPad.app")
    parser.add_argument("--resources", type=Path, default=ROOT / "assets/private/ipad/GameData.bundle")
    parser.add_argument("--platform", choices=["simulator", "device"], default="simulator",
                        help="device builds are unprovisioned and cannot be installed until signed")
    args = parser.parse_args()
    device = args.platform == "device"
    rust_target = "aarch64-apple-ios" if device else "aarch64-apple-ios-sim"
    sdk_name = "iphoneos" if device else "iphonesimulator"
    swift_target = "arm64-apple-ios17.0" if device else "arm64-apple-ios17.0-simulator"
    output = args.output.resolve()
    if not output.is_relative_to(ROOT / "assets") or output.exists():
        parser.error("choose a new output directory inside ignored assets")
    resources = args.resources.resolve()
    manifest = json.loads((resources / "manifest.json").read_text())
    for name in ["discworld.iso", "music.sf2", "SoundFont-COPYRIGHT.txt"]:
        with (resources / name).open("rb") as source:
            checksum = hashlib.file_digest(source, "sha256").hexdigest()
        if checksum != manifest["files"][name]["sha256"]:
            raise RuntimeError(f"resource hash differs: {name}")
    run("cargo", "build", "--locked", "--release", "--target", rust_target, "-p", "discworld-ipad")
    sdk = subprocess.check_output(["xcrun", "--sdk", sdk_name, "--show-sdk-path"], text=True).strip()
    output.mkdir(parents=True)
    run("xcrun", "--sdk", sdk_name, "swiftc", "-swift-version", "5", "-parse-as-library", "-O",
        "-target", swift_target, "-sdk", sdk,
        "-import-objc-header", str(ROOT / "platform/ipad/Bridge.h"),
        str(ROOT / "platform/ipad/App.swift"),
        "-L", str(ROOT / "target" / rust_target / "release"), "-ldiscworld_ipad",
        "-framework", "UIKit", "-framework", "AVFoundation", "-framework", "Security",
        "-o", str(output / "Discworld"))
    info = {
        "CFBundleExecutable": "Discworld", "CFBundleIdentifier": "org.tampajohn.discworld.dev",
        "CFBundleName": "Discworld", "CFBundleDisplayName": "Discworld Dev",
        "CFBundlePackageType": "APPL", "CFBundleVersion": "1", "CFBundleShortVersionString": "0.1",
        "MinimumOSVersion": "17.0", "UIDeviceFamily": [2], "LSRequiresIPhoneOS": True,
        "CFBundleSupportedPlatforms": ["iPhoneOS" if device else "iPhoneSimulator"],
        "UISupportedInterfaceOrientations": ["UIInterfaceOrientationLandscapeLeft", "UIInterfaceOrientationLandscapeRight", "UIInterfaceOrientationPortrait", "UIInterfaceOrientationPortraitUpsideDown"],
        "UILaunchScreen": {},
        "UIApplicationSceneManifest": {"UIApplicationSupportsMultipleScenes": False, "UISceneConfigurations": {}},
    }
    with (output / "Info.plist").open("wb") as target:
        plistlib.dump(info, target)
    shutil.copytree(resources, output / "GameData.bundle")
    run("cargo", "run", "--locked", "--release", "-p", "discworld-tools", "--example", "ipad_controls", "--",
        str(resources / "discworld.iso"), str(output / "Controls"))
    for name in ["LICENSE", "NOTICE"]:
        shutil.copy2(ROOT / name, output / name)
    if device:
        print("Device executable linked. Provisioning and Apple development/distribution signing are still required.")
    else:
        run("codesign", "--force", "--sign", "-", str(output))
        run("codesign", "--verify", "--deep", "--strict", str(output))
    print(output)


if __name__ == "__main__":
    main()

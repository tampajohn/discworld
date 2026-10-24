#!/usr/bin/env python3
"""Configure Simulator for touch-keyboard tests, preserving app/device data."""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import plistlib
import signal
import subprocess
import time
import uuid

ROOT = Path(__file__).resolve().parent.parent
DOMAIN = "com.apple.iphonesimulator"


def preferences():
    result = subprocess.run(["defaults", "export", DOMAIN, "-"], capture_output=True, check=True)
    return plistlib.loads(result.stdout)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("device", help="simulator UDID")
    args = parser.parse_args()
    devices = json.loads(subprocess.check_output(["xcrun", "simctl", "list", "devices", "-j"]))
    devices = [device for group in devices["devices"].values() for device in group]
    if not any(device["udid"] == args.device for device in devices):
        parser.error("unknown simulator UDID")
    settings = preferences()
    device_settings = settings.get("DevicePreferences", {}).get(args.device, {})
    if settings.get("ConnectHardwareKeyboard") is False and device_settings.get("ConnectHardwareKeyboard") is False:
        print("Simulator touch keyboard is configured (shared and device preferences).")
        return
    # Simulator's GUI process is shared. Do not restart it over another booted device.
    if any(device["state"] == "Booted" and device["udid"] != args.device for device in devices):
        parser.error("keyboard setup requires restarting Simulator; shut down other test simulators first")
    output = ROOT / "assets/validation" / ("keyboard-setup-" + datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + "-" + uuid.uuid4().hex[:8])
    output.mkdir(parents=True)
    (output / "before.plist").write_bytes(plistlib.dumps(settings))
    result = subprocess.run(["pgrep", "-x", "Simulator"], capture_output=True, text=True)
    if result.returncode not in (0, 1):
        raise RuntimeError("could not inspect Simulator process")
    for pid in result.stdout.split():
        command = subprocess.check_output(["ps", "-p", pid, "-o", "command="], text=True).strip()
        if "/Simulator.app/Contents/MacOS/Simulator" not in command:
            raise RuntimeError("unexpected process named Simulator")
        os.kill(int(pid), signal.SIGTERM)
        for _ in range(100):
            if subprocess.run(["kill", "-0", pid], stderr=subprocess.DEVNULL).returncode:
                break
            time.sleep(0.1)
        else:
            raise RuntimeError("Simulator did not exit; preferences left unchanged")
    # Reload after GUI shutdown to retain its last geometry/preferences writes.
    settings = preferences()
    settings["ConnectHardwareKeyboard"] = False
    settings.setdefault("DevicePreferences", {}).setdefault(args.device, {})["ConnectHardwareKeyboard"] = False
    after = output / "after.plist"
    after.write_bytes(plistlib.dumps(settings))
    subprocess.run(["defaults", "import", DOMAIN, str(after)], check=True)
    subprocess.run(["open", "-a", "Simulator", "--args", "-CurrentDeviceUDID", args.device], check=True)
    print(f"Disabled shared and device hardware-keyboard connections; backup: {output}")


if __name__ == "__main__":
    main()

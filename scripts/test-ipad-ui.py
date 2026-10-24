#!/usr/bin/env python3
"""Build the native iPad host and run its XCTest UI suite on a simulator."""
import argparse
import json
from pathlib import Path
import plistlib
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("device", help="simulator UDID")
    parser.add_argument("output", type=Path, help="new evidence directory under assets")
    parser.add_argument("--build-only", action="store_true", help="compile UI tests without changing Simulator or launching the app")
    parser.add_argument("--no-window", action="store_true", help="use a separately booted test device without opening Simulator or changing its shared preferences")
    args = parser.parse_args()
    out = args.output.resolve()
    if not out.is_relative_to(ROOT / "assets") or out.exists():
        parser.error("choose a new output directory inside assets")
    if not args.build_only:
        devices = json.loads(subprocess.check_output(["xcrun", "simctl", "list", "devices", "-j"]))
        others = [device for group in devices["devices"].values() for device in group
                  if device["state"] == "Booted" and device["udid"] != args.device]
        if others:
            parser.error("another simulator is booted; preserve the player's session and use --build-only")
    if not args.build_only and not args.no_window:
        subprocess.run([sys.executable, str(ROOT / "scripts/configure-ipad-keyboard.py"), args.device], check=True)
    subprocess.run(["cargo", "build", "--locked", "--release", "--target", "aarch64-apple-ios-sim", "-p", "discworld-ipad"], cwd=ROOT, check=True)
    if not args.build_only and not args.no_window:
        subprocess.run(["open", "-a", "Simulator", "--args", "-CurrentDeviceUDID", args.device], check=True)
    out.mkdir(parents=True)
    subprocess.run(["cargo", "run", "--locked", "--release", "-p", "discworld-tools", "--example", "ipad_controls", "--", str(ROOT / "assets/private/ipad/GameData.bundle/discworld.iso"), str(out / "Controls")], cwd=ROOT, check=True)
    subprocess.run([sys.executable, str(ROOT / "scripts/stage-ios-app-icon.py"), str(out / "Assets.xcassets")], check=True)
    project = out / "Discworld.xcodeproj"
    project.mkdir()
    objects = {}

    def obj(isa, **values):
        key = f"{len(objects) + 1:024X}"
        objects[key] = dict(isa=isa, **values)
        return key

    def config(settings):
        build = obj("XCBuildConfiguration", name="Debug", buildSettings=settings)
        return obj("XCConfigurationList", buildConfigurations=[build], defaultConfigurationIsVisible=0, defaultConfigurationName="Debug")

    def source(path):
        file = obj("PBXFileReference", lastKnownFileType="sourcecode.swift", path=str(path), sourceTree="<absolute>")
        return file, obj("PBXBuildFile", fileRef=file)

    app_source, app_build = source(ROOT / "platform/ipad/App.swift")
    test_source, test_build = source(ROOT / "platform/ipad/UITests.swift")
    resources = obj("PBXFileReference", lastKnownFileType="folder", name="GameData.bundle", path=str(ROOT / "assets/private/ipad/GameData.bundle"), sourceTree="<absolute>")
    resource_build = obj("PBXBuildFile", fileRef=resources)
    controls = obj("PBXFileReference", lastKnownFileType="folder", name="Controls", path=str(out / "Controls"), sourceTree="<absolute>")
    controls_build = obj("PBXBuildFile", fileRef=controls)
    catalog = obj("PBXFileReference", lastKnownFileType="folder.assetcatalog", path=str(out / "Assets.xcassets"), sourceTree="<absolute>")
    catalog_build = obj("PBXBuildFile", fileRef=catalog)
    app_product = obj("PBXFileReference", explicitFileType="wrapper.application", path="Discworld.app", sourceTree="BUILT_PRODUCTS_DIR")
    test_product = obj("PBXFileReference", explicitFileType="wrapper.cfbundle", path="DiscworldUITests.xctest", sourceTree="BUILT_PRODUCTS_DIR")
    products = obj("PBXGroup", name="Products", children=[app_product, test_product], sourceTree="<group>")
    group = obj("PBXGroup", children=[app_source, test_source, resources, controls, catalog, products], sourceTree="<group>")
    common = {"ARCHS": "arm64", "ONLY_ACTIVE_ARCH": "YES", "SDKROOT": "iphonesimulator", "IPHONEOS_DEPLOYMENT_TARGET": "17.0", "TARGETED_DEVICE_FAMILY": "2", "SWIFT_VERSION": "5.0", "CODE_SIGNING_ALLOWED": "NO", "GENERATE_INFOPLIST_FILE": "YES", "PRODUCT_NAME": "$(TARGET_NAME)"}
    app_settings = dict(common, ASSETCATALOG_COMPILER_APPICON_NAME="AppIcon", PRODUCT_BUNDLE_IDENTIFIER="org.tampajohn.discworld.dev", SWIFT_OBJC_BRIDGING_HEADER=str(ROOT / "platform/ipad/Bridge.h"), LIBRARY_SEARCH_PATHS=[str(ROOT / "target/aarch64-apple-ios-sim/release")], OTHER_LDFLAGS=["-ldiscworld_ipad", "-framework", "Security"], INFOPLIST_KEY_UILaunchScreen_Generation="YES", INFOPLIST_KEY_UIApplicationSceneManifest_Generation="YES", INFOPLIST_KEY_UISupportedInterfaceOrientations_iPad="UIInterfaceOrientationPortrait UIInterfaceOrientationPortraitUpsideDown UIInterfaceOrientationLandscapeLeft UIInterfaceOrientationLandscapeRight")
    app = obj("PBXNativeTarget", name="Discworld", productName="Discworld", productReference=app_product, productType="com.apple.product-type.application", buildConfigurationList=config(app_settings), buildPhases=[obj("PBXSourcesBuildPhase", files=[app_build], buildActionMask=2147483647, runOnlyForDeploymentPostprocessing=0), obj("PBXResourcesBuildPhase", files=[resource_build, controls_build, catalog_build], buildActionMask=2147483647, runOnlyForDeploymentPostprocessing=0)], buildRules=[], dependencies=[])
    test = obj("PBXNativeTarget", name="DiscworldUITests", productName="DiscworldUITests", productReference=test_product, productType="com.apple.product-type.bundle.ui-testing", buildConfigurationList=config(dict(common, PRODUCT_BUNDLE_IDENTIFIER="org.tampajohn.discworld.uitests", TEST_TARGET_NAME="Discworld")), buildPhases=[obj("PBXSourcesBuildPhase", files=[test_build], buildActionMask=2147483647, runOnlyForDeploymentPostprocessing=0)], buildRules=[], dependencies=[obj("PBXTargetDependency", target=app)])
    root = obj("PBXProject", buildConfigurationList=config({}), compatibilityVersion="Xcode 14.0", developmentRegion="en", knownRegions=["en", "Base"], mainGroup=group, productRefGroup=products, projectDirPath="", projectRoot="", targets=[app, test], attributes={"LastUpgradeCheck": "2600"})
    with (project / "project.pbxproj").open("wb") as file:
        plistlib.dump(dict(archiveVersion="1", classes={}, objectVersion="56", objects=objects, rootObject=root), file)
    schemes = project / "xcshareddata/xcschemes"
    schemes.mkdir(parents=True)
    reference = lambda key, name: f'<BuildableReference BuildableIdentifier="primary" BlueprintIdentifier="{key}" BuildableName="{name}" BlueprintName="{name.split(".")[0]}" ReferencedContainer="container:Discworld.xcodeproj"/>'
    (schemes / "Discworld.xcscheme").write_text(f'''<?xml version="1.0" encoding="UTF-8"?>
<Scheme LastUpgradeVersion="2600" version="1.3">
<BuildAction><BuildActionEntries><BuildActionEntry buildForTesting="YES" buildForRunning="YES" buildForProfiling="NO" buildForArchiving="NO" buildForAnalyzing="YES">{reference(app, "Discworld.app")}</BuildActionEntry></BuildActionEntries></BuildAction>
<TestAction buildConfiguration="Debug" selectedDebuggerIdentifier="Xcode.DebuggerFoundation.Debugger.LLDB" selectedLauncherIdentifier="Xcode.IDEFoundation.Launcher.LLDB" shouldUseLaunchSchemeArgsEnv="YES"><Testables><TestableReference skipped="NO">{reference(test, "DiscworldUITests.xctest")}</TestableReference></Testables></TestAction>
</Scheme>''')
    subprocess.run(["xcodebuild", "build-for-testing" if args.build_only else "test", "-project", str(project), "-scheme", "Discworld", "-destination", f"platform=iOS Simulator,id={args.device}", "-derivedDataPath", str(out / "derived"), "-resultBundlePath", str(out / "results.xcresult"), "-parallel-testing-enabled", "NO"], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()

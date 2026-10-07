"""The Android app: the game's native library (crates/nfsgba-android) for the phone and emulator ABIs, then the APK.

    .venv/Scripts/python.exe tools/android_build.py                 # release APK, arm64 phones + x86_64 emulator
    .venv/Scripts/python.exe tools/android_build.py --abi x86_64 --debug --install --push-rom --run   # the emulator

Needs the Android SDK (ANDROID_HOME, or the default %LOCALAPPDATA%/Android/Sdk) with an NDK under ndk/ (the newest is
used), the Rust targets (`rustup target add aarch64-linux-android x86_64-linux-android`), `cargo install cargo-ndk`,
and a JDK 17 for Gradle (JAVA_HOME, else Android Studio's bundled one). The APK ships no game data: on the first start
the app asks for the player's ROM (the system file picker) and keeps it in its own storage.

--install and --run use `adb` (one device or emulator connected). --push-rom (debug builds only: `run-as`) copies the
vault's ROM into the app's files, as the picker would, so a test run skips the picker.
"""
import argparse
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ANDROID = ROOT / "android"
PACKAGE = "io.github.mtailordev.nfsgba"
ABIS = {"arm64-v8a": "arm64-v8a", "x86_64": "x86_64"}


def run(*cmd, cwd=ROOT, env=None):
    print("+", " ".join(map(str, cmd)), flush=True)
    subprocess.run([str(c) for c in cmd], cwd=cwd, check=True, env=env)


def sdk() -> Path:
    home = os.environ.get("ANDROID_HOME") or os.environ.get("ANDROID_SDK_ROOT")
    path = Path(home) if home else Path(os.environ.get("LOCALAPPDATA", "")) / "Android" / "Sdk"
    if not path.is_dir():
        sys.exit(f"no Android SDK at {path}: set ANDROID_HOME")
    return path


def ndk(sdk_dir: Path) -> Path:
    if os.environ.get("ANDROID_NDK_HOME"):
        return Path(os.environ["ANDROID_NDK_HOME"])
    found = sorted((sdk_dir / "ndk").glob("*"), key=lambda p: [int(x) for x in p.name.split(".") if x.isdigit()])
    if not found:
        sys.exit(f"no NDK under {sdk_dir / 'ndk'}")
    return found[-1]


def java_home() -> str:
    if os.environ.get("JAVA_HOME"):
        return os.environ["JAVA_HOME"]
    studio = Path(os.environ.get("ProgramFiles", r"C:\Program Files")) / "Android" / "Android Studio" / "jbr"
    if studio.is_dir():
        return str(studio)
    sys.exit("need a JDK 17 for Gradle: set JAVA_HOME")


def canonical_rom() -> Path:
    """The vault's canonical ROM (`data/vault/manifest.json`, as `nfsgba_formats::canonical_rom` reads it)."""
    sys.path.insert(0, str(ROOT / "tools"))
    from common import data_dir  # noqa: E402

    manifest = json.loads((data_dir() / "vault" / "manifest.json").read_text(encoding="utf-8"))
    rom = next(r for r in manifest["roms"] if r["sha1"] == manifest["canonical_target"])
    return data_dir() / rom["vault_file"]


def adb(sdk_dir: Path, *args):
    tool = sdk_dir / "platform-tools" / ("adb.exe" if os.name == "nt" else "adb")
    run(tool, *args)


def main():
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--abi", choices=sorted(ABIS), action="append", help="ABIs to build (default: all)")
    p.add_argument("--debug", action="store_true", help="a debuggable APK (needed for --push-rom)")
    p.add_argument("--install", action="store_true", help="adb install the APK")
    p.add_argument("--push-rom", action="store_true", help="copy the vault's ROM into the app's files (debug only)")
    p.add_argument("--run", action="store_true", help="start the app")
    a = p.parse_args()
    sdk_dir = sdk()
    env = dict(os.environ, ANDROID_HOME=str(sdk_dir), ANDROID_NDK_HOME=str(ndk(sdk_dir)), JAVA_HOME=java_home())
    cargo = shutil.which("cargo") or str(Path.home() / ".cargo" / "bin" / "cargo")
    jni = ANDROID / "app" / "src" / "main" / "jniLibs"
    targets = [x for abi in (a.abi or sorted(ABIS)) for x in ("-t", ABIS[abi])]
    run(cargo, "ndk", *targets, "-P", "26", "-o", jni, "build", "--profile", "mobile", "-p", "nfsgba-android", env=env)
    gradlew = ANDROID / ("gradlew.bat" if os.name == "nt" else "gradlew")
    variant = "Debug" if a.debug else "Release"
    run(gradlew, f"assemble{variant}", "--no-daemon", "-q", cwd=ANDROID, env=env)
    apk = ANDROID / "app" / "build" / "outputs" / "apk" / variant.lower() / f"app-{variant.lower()}.apk"
    print(f"{apk}: {apk.stat().st_size / 1e6:.1f} MB")
    if a.install:
        adb(sdk_dir, "install", "-r", apk)
    if a.push_rom:
        if not a.debug:
            sys.exit("--push-rom needs --debug (run-as works on debuggable builds only)")
        adb(sdk_dir, "push", canonical_rom(), "/data/local/tmp/nfsgba-rom.gba")
        # The app's files directory exists once the app has run; before that it is made here. The app's user may not
        # read /data/local/tmp (SELinux), so the shell reads the file and the app's user writes it.
        adb(sdk_dir, "shell", "run-as", PACKAGE, "mkdir", "-p", "files")
        adb(sdk_dir, "shell", "run-as", PACKAGE, "rm", "-f", "files/rom.gba")
        adb(sdk_dir, "shell", f"cat /data/local/tmp/nfsgba-rom.gba | run-as {PACKAGE} sh -c 'cat > files/rom.gba'")
        adb(sdk_dir, "shell", "rm", "/data/local/tmp/nfsgba-rom.gba")
    if a.run:
        adb(sdk_dir, "shell", "am", "start", "-n", f"{PACKAGE}/.LauncherActivity")


if __name__ == "__main__":
    main()

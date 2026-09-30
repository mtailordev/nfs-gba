"""The web build: the viewer for wasm32 plus the page, into target/site/ (what .github/workflows/pages.yml deploys).

    .venv/Scripts/python.exe tools/web_build.py          # build into target/site/
    .venv/Scripts/python.exe -m http.server 8090 -d target/site   # then open http://localhost:8090 and pick the ROM

Needs the wasm32 target (`rustup target add wasm32-unknown-unknown`) and the `wasm-bindgen` CLI of the version in
Cargo.lock on PATH (a release binary from github.com/wasm-bindgen/wasm-bindgen, or `cargo install wasm-bindgen-cli
--version <v>`); `wasm-opt` (binaryen) on PATH shrinks the module when present. The page ships no game data: the
player picks their ROM and it stays in their browser.
"""
import re
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "target" / "site"
WASM = ROOT / "target" / "wasm32-unknown-unknown" / "web" / "nfsgba-viewer.wasm"


def run(*cmd):
    print("+", " ".join(map(str, cmd)), flush=True)
    subprocess.run(cmd, cwd=ROOT, check=True)


def lock_version(crate):
    m = re.search(rf'name = "{crate}"\nversion = "([^"]+)"', (ROOT / "Cargo.lock").read_text())
    return m.group(1)


def main():
    want = lock_version("wasm-bindgen")
    bindgen = shutil.which("wasm-bindgen")
    have = bindgen and subprocess.run([bindgen, "--version"], capture_output=True, text=True).stdout.split()[-1]
    if have != want:
        sys.exit(f"need wasm-bindgen {want} on PATH (found {have or 'none'}): cargo install wasm-bindgen-cli --version {want}")
    run("cargo", "build", "--profile", "web", "--target", "wasm32-unknown-unknown", "-p", "nfsgba-viewer")
    if OUT.exists():
        shutil.rmtree(OUT)
    run(bindgen, "--target", "web", "--no-typescript", "--out-dir", OUT, "--out-name", "nfsgba", WASM)
    wasm = OUT / "nfsgba_bg.wasm"
    if opt := shutil.which("wasm-opt"):
        # The features rustc enables for wasm32 by default (Rust 1.82+).
        feats = ["bulk-memory", "multivalue", "mutable-globals", "nontrapping-float-to-int", "reference-types", "sign-ext"]
        run(opt, "-Oz", *(f"--enable-{f}" for f in feats), "-o", wasm, wasm)
    else:
        print("wasm-opt not found: the module is not shrunk")
    for f in (ROOT / "web").iterdir():
        shutil.copy(f, OUT / f.name)
    print(f"{OUT}: nfsgba_bg.wasm {wasm.stat().st_size / 1e6:.1f} MB")


if __name__ == "__main__":
    main()

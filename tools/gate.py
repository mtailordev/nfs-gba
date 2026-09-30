"""The merge gate: everything a branch must pass before it merges into main. Prints one line per step and exits
non-zero when any step fails.

    .venv/Scripts/python.exe tools/gate.py [--update-fixtures]   # the flag rewrites the fixture manifest from this run's own test log

Steps: rustfmt; clippy with warnings as errors; the Rust tests with NFSGBA_REQUIRE_DATA=1 (missing data fails, see
docs/engine/testkit.md); the Python tool tests; the fixture manifest (every fixture present, unchanged and listed);
the branch's pending notes CSVs (docs/engine/notes/, changed since the merge base with main) merge without conflict;
every `NOT 1:1 (ID)` marker in crates/ names an ID that is open in docs/FIDELITY.md.
"""
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TOOLS = ROOT / "tools"
ID = re.compile(r"\b[A-Z]\d+\b")
MARKER = re.compile(r"NOT 1:1\s*(?:\(([^)]*)\))?")


def open_ids(fidelity: str) -> set[str]:
    """IDs of the open entries: the first cell of the ledger's table rows."""
    return set(re.findall(r"^\| ([A-Z]\d+) \|", fidelity, re.M))


def marker_problems(files: dict[str, str], fidelity: str) -> list[str]:
    """`NOT 1:1` markers without an ID, or citing an ID that is not open (closed or unknown)."""
    live = open_ids(fidelity)
    out = []
    for path, text in sorted(files.items()):
        for n, line in enumerate(text.splitlines(), 1):
            for m in MARKER.finditer(line):
                ids = ID.findall(m.group(1) or "")
                if not ids:
                    out.append(f"{path}:{n}: no ID: {line.strip()[:110]}")
                out += [f"{path}:{n}: cites {i}, not an open entry: {line.strip()[:90]}" for i in ids if i not in live]
    return out


def run(cmd: list[str], cwd: Path = ROOT, env: dict | None = None) -> tuple[bool, str]:
    p = subprocess.run(cmd, cwd=cwd, env={**os.environ, **(env or {})}, capture_output=True, text=True,
                       encoding="utf-8", errors="replace")
    tail = (p.stdout + p.stderr).strip().splitlines()[-12:]
    return p.returncode == 0, "\n      ".join(tail)


def pending_notes() -> list[str]:
    base = subprocess.run(["git", "merge-base", "HEAD", "main"], cwd=ROOT, capture_output=True, text=True).stdout.strip()
    changed = subprocess.run(["git", "diff", "--name-only", base, "--", "docs/engine/notes"], cwd=ROOT,
                             capture_output=True, text=True).stdout.split()
    untracked = subprocess.run(["git", "ls-files", "--others", "--exclude-standard", "docs/engine/notes"], cwd=ROOT,
                               capture_output=True, text=True).stdout.split()
    return sorted(f for f in set(changed + untracked) if f.endswith(".csv") and (ROOT / f).exists())


def main() -> int:
    cargo = shutil.which("cargo") or str(Path.home() / ".cargo" / "bin" / "cargo")
    py = sys.executable
    results = []
    with tempfile.TemporaryDirectory() as tmp:
        log = Path(tmp) / "fixtures.log"
        steps = [
            ("rustfmt", lambda: run([cargo, "fmt", "--all", "--check"])),
            ("clippy", lambda: run([cargo, "clippy", "--release", "--workspace", "--all-targets", "-q", "--",
                                    "-D", "warnings"])),
            ("rust tests (data required)", lambda: run([cargo, "test", "--release", "--workspace", "-q"],
                                                       env={"NFSGBA_REQUIRE_DATA": "1",
                                                            "NFSGBA_FIXTURE_LOG": str(log)})),
            ("python tests", lambda: run([py, "-m", "unittest", "discover", "-p", "test_*.py"], cwd=TOOLS)),
            *([("fixture manifest update", lambda: run([py, "fixtures.py", "build", "--log", str(log)], cwd=TOOLS))]
              if "--update-fixtures" in sys.argv else []),
            ("fixture manifest", lambda: run([py, "fixtures.py", "check"] + (["--log", str(log)] if log.exists()
                                                                               else []), cwd=TOOLS)),
            ("pending notes", lambda: (True, "none pending") if not (notes := pending_notes()) else
             run([py, "notes_merge.py", *[str(ROOT / n) for n in notes]], cwd=TOOLS,
                 env={"PYTHONIOENCODING": "utf-8"})),
            ("FIDELITY markers", markers),
        ]
        for name, step in steps:
            ok, detail = step()
            results.append(ok)
            print(f"{'PASS' if ok else 'FAIL'}  {name}" + (f"\n      {detail}" if not ok and detail else ""))
            sys.stdout.flush()
    print(f"gate: {results.count(True)} of {len(results)} steps pass")
    return 0 if all(results) else 1


def markers() -> tuple[bool, str]:
    files = {p.relative_to(ROOT).as_posix(): p.read_text(encoding="utf-8")
             for p in (ROOT / "crates").rglob("*") if p.suffix in (".rs", ".wgsl")}
    problems = marker_problems(files, (ROOT / "docs" / "FIDELITY.md").read_text(encoding="utf-8"))
    return not problems, "\n      ".join([f"{len(problems)} problems:"] + problems)


if __name__ == "__main__":
    sys.exit(main())

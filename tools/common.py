"""Helpers shared by the recon tools: data folder lookup, hashing, provenance."""
import hashlib
import os
import sys
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def data_dir() -> Path:
    """$NFSGBA_DATA from the environment, else from the nearest .env at or above the checkout (git worktrees
    under .claude/worktrees/ find the main checkout's), else ./data."""
    val = os.environ.get("NFSGBA_DATA")
    env = next((d / ".env" for d in [ROOT, *ROOT.parents] if (d / ".env").exists()), ROOT / ".env")
    if not val and env.exists():
        for line in env.read_text(encoding="utf-8").splitlines():
            key, sep, v = line.partition("=")
            if sep and key.strip() == "NFSGBA_DATA":
                val = v.strip().strip('"')
    return Path(val) if val else ROOT / "data"


def hashes(b: bytes) -> dict:
    return {"crc32": f"{zlib.crc32(b):08x}", "md5": hashlib.md5(b).hexdigest(), "sha1": hashlib.sha1(b).hexdigest()}


def provenance(tool_file: str) -> dict:
    """Which script (by content hash), Python and command produced an output."""
    p = Path(tool_file).resolve()
    rel = p.relative_to(ROOT).as_posix()
    return {
        "tool": rel,
        "tool_sha1": hashlib.sha1(p.read_bytes()).hexdigest(),
        "python": sys.version.split()[0],
        "command": " ".join(["python", rel] + sys.argv[1:]),
    }


def write_if_changed(path: Path, text: str) -> bool:
    """Keep reruns from touching files whose content is unchanged."""
    if path.exists() and path.read_text(encoding="utf-8") == text:
        return False
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")
    return True

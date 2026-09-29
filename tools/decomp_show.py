"""Print decompiled functions with every literal-pool `DAT_xxxxxxxx` resolved to the word it holds.

    .venv/Scripts/python.exe tools/decomp_show.py race_frame_update 0x0812ae64

Reads `$NFSGBA_DATA/work/e5298b24/ghidra/carbon_decomp.c` (exported by tools/ghidra/ExportDecomp.java) and the
canonical ROM. `DAT_0813a9fc` becomes `DAT_0813a9fc{=0x03005800}`; values that name a function or label in
docs/engine/symbols.csv also show the name. Arguments are function names or addresses.
"""
import json
import re
import struct
import sys

from common import ROOT, data_dir

HEADER = re.compile(r"^// ==== ([0-9a-f]{8}) (\S+)$", re.M)
DAT = re.compile(r"\bDAT_([0-9a-f]{8})\b")


def load():
    m = json.loads((data_dir() / "vault" / "manifest.json").read_text(encoding="utf-8"))
    r = next(r for r in m["roms"] if r["sha1"] == m["canonical_target"])
    rom = (data_dir() / r["vault_file"]).read_bytes()
    text = (data_dir() / "work" / r["sha1"][:8] / "ghidra" / "carbon_decomp.c").read_text(errors="replace")
    names = {}
    for line in (ROOT / "docs" / "engine" / "symbols.csv").read_text(encoding="utf-8").splitlines()[1:]:
        f = line.split(",")
        names[int(f[0], 16)] = f[1]
    return rom, text, names


def functions(text):
    heads = list(HEADER.finditer(text))
    for i, h in enumerate(heads):
        end = heads[i + 1].start() if i + 1 < len(heads) else len(text)
        yield int(h.group(1), 16), h.group(2), text[h.start():end]


def resolve(body, rom, names):
    def word(m):
        at = int(m.group(1), 16)
        if not 0x08000000 <= at < 0x08000000 + len(rom) - 3:
            return m.group(0)
        v = struct.unpack_from("<I", rom, at - 0x08000000)[0]
        name = names.get(v & ~1)
        return f"{m.group(0)}{{={v:#010x}{' ' + name if name else ''}}}"

    return DAT.sub(word, body)


def main(args):
    rom, text, names = load()
    by_name = {}
    for addr, name, body in functions(text):
        by_name[name] = by_name[f"{addr:#010x}"] = body
    for a in args:
        key = a if a in by_name else f"{int(a, 16):#010x}"
        print(resolve(by_name[key], rom, names))


if __name__ == "__main__":
    main(sys.argv[1:])

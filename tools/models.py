"""Parse Carbon's vehicle model bank (docs/formats/vehicle-models.md) and export each model as Wavefront OBJ.

    python tools/models.py      # -> $NFSGBA_DATA/out/models/<sha1-8>/model_NNN.obj

Coordinates are the raw int16 model units; -y is up. Rerunning changes nothing.
"""
import json
import struct

from common import ROOT, data_dir, provenance, write_if_changed

ROM_BASE = 0x08000000
LEVEL_TABLE = 0x7F2B08  # BN7E: 0x68-byte level descriptors; words 13..21 point at the model bank arrays
MODEL = struct.Struct("<8I4H")  # 40 bytes, see the format doc


def bank_arrays(rom: bytes) -> dict:
    w = [x - ROM_BASE for x in struct.unpack_from("<26I", rom, LEVEL_TABLE)]
    return {"models": w[13], "verts": w[14], "indices": w[15], "uv_indices": w[16], "uvs": w[18], "sizes": w[21]}


def parse(rom: bytes) -> list[dict]:
    a = bank_arrays(rom)
    models = []
    for i in range((a["verts"] - a["models"]) // MODEL.size):
        vstart, istart, uvstart, pstart, _, _, uvistart, sstart, flags, npoly, nvert, nuv = \
            MODEL.unpack_from(rom, a["models"] + MODEL.size * i)
        verts = [struct.unpack_from("<3h", rom, a["verts"] + 6 * (vstart + k)) for k in range(nvert)]
        polys, p, q = [], istart, uvistart
        for k in range(npoly):
            n = rom[a["sizes"] + sstart + k]
            polys.append({"v": struct.unpack_from(f"<{n}H", rom, a["indices"] + 2 * p),
                          "uv": struct.unpack_from(f"<{n}H", rom, a["uv_indices"] + 2 * q)})
            p, q = p + n, q + n
        uvs = [struct.unpack_from("<2H", rom, a["uvs"] + 4 * (uvstart + k)) for k in range(nuv)]
        models.append({"index": i, "flags": flags, "first": (vstart, istart, uvstart, pstart, uvistart, sstart),
                       "verts": verts, "polys": polys, "uvs": uvs})
    return models


def obj(m: dict, header: str) -> str:
    lines = [header, f"o model_{m['index']:03d}"]
    lines += [f"v {x} {y} {z}" for x, y, z in m["verts"]]
    lines += ["f " + " ".join(str(k + 1) for k in p["v"]) for p in m["polys"]]
    return "\n".join(lines) + "\n"


def main() -> None:
    manifest = json.loads((data_dir() / "vault" / "manifest.json").read_text(encoding="utf-8"))
    r = next(r for r in manifest["roms"] if r["sha1"] == manifest["canonical_target"])
    rom = (data_dir() / r["vault_file"]).read_bytes()
    p = provenance(__file__)
    header = f"# model from ROM sha1 {r['sha1']}, {p['tool']} sha1 {p['tool_sha1'][:12]}"
    out = data_dir() / "out" / "models" / r["sha1"][:8]
    models = parse(rom)
    for m in models:
        write_if_changed(out / f"model_{m['index']:03d}.obj", obj(m, header))
    print(f"{len(models)} models -> {out}")


if __name__ == "__main__":
    main()

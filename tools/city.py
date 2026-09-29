"""Parse Carbon's city sectors (docs/formats/city-sectors.md) and export a top-down SVG map and a 3D OBJ.

    python tools/city.py    # -> $NFSGBA_DATA/out/city/<sha1-8>/city.svg, city.obj

Raw units; -y is up. Solid walls become quads between their top and bottom heights, sectors become floor
polygons, portals are left open. Rerunning changes nothing.
"""
import json
import struct

from common import data_dir, provenance, write_if_changed

ROM_BASE = 0x08000000
LEVEL_TABLE = 0x7F2B08  # BN7E level descriptor: +0x18 sectors, +0x14 walls
SECTOR, WALL = 0x30, 0x44


def parse(rom: bytes) -> list[dict]:
    rec = struct.unpack_from("<26I", rom, LEVEL_TABLE)
    walls_at, sectors_at = rec[5] - ROM_BASE, rec[6] - ROM_BASE
    sectors = []
    for s in range((walls_at - sectors_at) // SECTOR):
        first, count = struct.unpack_from("<2H", rom, sectors_at + SECTOR * s)
        walls = []
        for k in range(first, first + count):
            o = walls_at + WALL * k
            x, z = struct.unpack_from("<2i", rom, o)
            top0, bottom0, top1, bottom1 = struct.unpack_from("<4h", rom, o + 8)
            link, = struct.unpack_from("<h", rom, o + 0x30)
            material, flags = struct.unpack_from("<2H", rom, o + 0x2C)
            walls.append({"x": x, "z": z, "top": (top0, top1), "bottom": (bottom0, bottom1),
                          "link": link, "material": material, "flags": flags})
        sectors.append({"first": first, "walls": walls})
    return sectors


def segments(sector: dict):
    """(wall, start point, end point) for each edge of the closed sector polygon."""
    w = sector["walls"]
    for k, a in enumerate(w):
        b = w[(k + 1) % len(w)]
        yield a, (a["x"], a["z"]), (b["x"], b["z"])


def svg(sectors: list[dict], header: str) -> str:
    xs = [w["x"] for s in sectors for w in s["walls"]]
    zs = [w["z"] for s in sectors for w in s["walls"]]
    x0, z1 = min(xs), max(zs)
    out = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {max(xs) - x0} {z1 - min(zs)}">', f"<!-- {header} -->",
           '<g fill="none" stroke-width="80">']
    for s in sectors:
        for w, (ax, az), (bx, bz) in segments(s):
            colour = "#bbb" if w["link"] >= 0 else "#000"
            out.append(f'<line x1="{ax - x0}" y1="{z1 - az}" x2="{bx - x0}" y2="{z1 - bz}" stroke="{colour}"/>')
    return "\n".join(out + ["</g></svg>"]) + "\n"


def obj(sectors: list[dict], header: str) -> str:
    lines, n = [header, "o city"], 0
    for s in sectors:
        for w, (ax, az), (bx, bz) in segments(s):
            if w["link"] >= 0:
                continue
            lines += [f"v {ax} {w['top'][0]} {az}", f"v {bx} {w['top'][1]} {bz}",
                      f"v {bx} {w['bottom'][1]} {bz}", f"v {ax} {w['bottom'][0]} {az}", f"f {n + 1} {n + 2} {n + 3} {n + 4}"]
            n += 4
        floor = [f"v {w['x']} {w['bottom'][0]} {w['z']}" for w in s["walls"]]
        lines += floor + ["f " + " ".join(str(n + k + 1) for k in range(len(floor)))]
        n += len(floor)
    return "\n".join(lines) + "\n"


def main() -> None:
    manifest = json.loads((data_dir() / "vault" / "manifest.json").read_text(encoding="utf-8"))
    r = next(r for r in manifest["roms"] if r["sha1"] == manifest["canonical_target"])
    rom = (data_dir() / r["vault_file"]).read_bytes()
    p = provenance(__file__)
    header = f"city from ROM sha1 {r['sha1']}, {p['tool']} sha1 {p['tool_sha1'][:12]}"
    sectors = parse(rom)
    out = data_dir() / "out" / "city" / r["sha1"][:8]
    write_if_changed(out / "city.svg", svg(sectors, header))
    write_if_changed(out / "city.obj", obj(sectors, "# " + header))
    print(f"{len(sectors)} sectors, {sum(len(s['walls']) for s in sectors)} walls -> {out}")


if __name__ == "__main__":
    main()

"""Pixel agreement between viewer screenshots and the game, on the GBA's 240×160 grid.

    .venv/Scripts/python.exe crates/nfsgba-viewer/diff_shots.py A.png B.png [--exclude x0,y0,x1,y1 ...] [--out d.png]

Each image is reduced to 240×160 by taking the centre pixel of each GBA pixel (the viewer's window is the GBA
screen scaled up; a 240×160 image is used as is). Prints how many pixels are equal, leaving out the `--exclude`
rectangles (GBA pixels, x1/y1 exclusive). `--out` writes a 240×160 image: equal pixels grey, excluded ones black,
different ones as in A.
"""
import argparse

from PIL import Image


def gba(path, phase=(0.5, 0.5)):
    im = Image.open(path).convert("RGB")
    w, h = im.size
    px, py = phase
    return [[im.getpixel((int((x + px) * w / 240), int((y + py) * h / 160))) for x in range(240)] for y in range(160)]


def main():
    p = argparse.ArgumentParser()
    p.add_argument("a")
    p.add_argument("b")
    p.add_argument("--exclude", action="append", default=[])
    p.add_argument("--out")
    p.add_argument("--phase", default="0.5,0.5", help="where in each GBA pixel to sample A (0..1, 0..1)")
    args = p.parse_args()
    a, b = gba(args.a, tuple(map(float, args.phase.split(",")))), gba(args.b)
    boxes = [tuple(map(int, e.split(","))) for e in args.exclude]
    keep = [[not any(x0 <= x < x1 and y0 <= y < y1 for x0, y0, x1, y1 in boxes) for x in range(240)] for y in range(160)]
    total = sum(map(sum, keep))
    same = sum(1 for y in range(160) for x in range(240) if keep[y][x] and a[y][x] == b[y][x])
    def close(x, y):
        return any(a[y][x] == b[j][i] for j in range(max(0, y - 1), min(160, y + 2)) for i in range(max(0, x - 1), min(240, x + 2)))

    near = sum(1 for y in range(160) for x in range(240) if keep[y][x] and close(x, y))
    print(f"{same} of {total} pixels equal ({100 * same / total:.2f}%); within 1 pixel: {near} ({100 * near / total:.2f}%)")
    if args.out:
        # Equal: grey; equal within 1 pixel: dark grey; excluded: black; different: A's colour.
        out = Image.new("RGB", (240, 160))
        for y in range(160):
            for x in range(240):
                if not keep[y][x]:
                    c = (0, 0, 0)
                elif a[y][x] == b[y][x]:
                    c = (128, 128, 128)
                elif close(x, y):
                    c = (64, 64, 64)
                else:
                    c = a[y][x]
                out.putpixel((x, y), c)
        out.resize((960, 640), Image.NEAREST).save(args.out)


if __name__ == "__main__":
    main()

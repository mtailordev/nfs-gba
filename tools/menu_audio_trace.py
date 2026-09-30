"""Power-on to the Quick Play race start in headless mGBA (tools/retro.py): the sound buffer the DMA plays in each video
frame, for crates/nfsgba-game/tests/session.rs (test `menu_audio_matches_the_game`). A test oracle, not a runtime.

    .venv/Scripts/python.exe tools/menu_audio_trace.py [NAME]     # writes $NFSGBA_DATA/work/e5298b24/session/NAME-audio.bin

The key script is the one of session_trace.py. The mixer keeps two 176-byte signed 8-bit buffers (0x03005DEC and
+0xB0) and restarts the DMA on the one it mixed last in every VBlank; DMA1's source register says which. Per frame the file
holds the 176 bytes of the buffer playing during the frame after the one just run (the VBlank of the next frame starts
it), i.e. the bytes `Session::sound` returns for the next frame.
"""
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import retro
import session_trace
from common import data_dir

FRAMES = 2700
BUFFERS = 0x03005DEC
LEN = 0xB0


def main(argv):
    name = argv[0] if argv else "quickplay"
    out = data_dir() / "work" / "e5298b24" / "session"
    out.mkdir(parents=True, exist_ok=True)
    retro.SESSION = out / f"session-{name}-audio"
    r = retro.Retro(video=False)
    held = {}
    for first, n, key in session_trace.SCRIPT:
        for f in range(first, first + n):
            held[f] = (key,)
    data = bytearray()
    for f in range(FRAMES):
        r.run(1, keys=held.get(f, ()))
        s = r.serialize()
        iw = s[0x19000:0x21000]
        io = s[0x400:0x800]
        sad = struct.unpack_from("<I", io, 0xBC)[0]
        k = (sad - BUFFERS) // LEN if BUFFERS <= sad < BUFFERS + 2 * LEN else 0
        a = BUFFERS - 0x03000000 + LEN * k
        data += iw[a:a + LEN]
    (out / f"{name}-audio.bin").write_bytes(bytes(data))
    print(name, FRAMES, "frames,", sum(1 for b in data if b), "non-zero bytes")


if __name__ == "__main__":
    main(sys.argv[1:])

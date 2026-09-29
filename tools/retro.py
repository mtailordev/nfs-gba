"""Headless mGBA libretro core driven through ctypes. Core: ext/libretro/mgba_libretro.dll (never committed).
CLI: retro.py [--ss race.ss] FRAMES [--dump PREFIX]   (runs, prints the RAM sha1, writes an oracle snapshot).  Not a game-code runtime; a test oracle only."""
import ctypes as C
import hashlib
import struct
import sys
import zlib
from pathlib import Path

from common import ROOT, data_dir

DLL = ROOT / "ext" / "libretro" / "mgba_libretro.dll"
ROM = data_dir() / "vault" / "roms" / "BN7E_v0_e5298b24.gba"
SESSION = data_dir() / "work" / "e5298b24" / "retro"
KEY = dict(B=0, SELECT=2, START=3, UP=4, DOWN=5, LEFT=6, RIGHT=7, A=8, L=10, R=11)
EXP = 0x10000
ENV_CB = C.CFUNCTYPE(C.c_bool, C.c_uint, C.c_void_p)
VIDEO_CB = C.CFUNCTYPE(None, C.c_void_p, C.c_uint, C.c_uint, C.c_size_t)
AUDIO_CB = C.CFUNCTYPE(C.c_size_t, C.c_void_p, C.c_size_t)
SAMPLE_CB = C.CFUNCTYPE(None, C.c_int16, C.c_int16)
POLL_CB = C.CFUNCTYPE(None)
INPUT_CB = C.CFUNCTYPE(C.c_int16, C.c_uint, C.c_uint, C.c_uint, C.c_uint)


class Desc(C.Structure):
    _fields_ = [("flags", C.c_uint64), ("ptr", C.c_void_p), ("offset", C.c_size_t), ("start", C.c_size_t),
                ("select", C.c_size_t), ("disconnect", C.c_size_t), ("len", C.c_size_t), ("addrspace", C.c_char_p)]


class MemMap(C.Structure):
    _fields_ = [("descs", C.POINTER(Desc)), ("n", C.c_uint)]


class GameInfo(C.Structure):
    _fields_ = [("path", C.c_char_p), ("data", C.c_void_p), ("size", C.c_size_t), ("meta", C.c_char_p)]


class SysInfo(C.Structure):
    _fields_ = [("name", C.c_char_p), ("version", C.c_char_p), ("exts", C.c_char_p), ("need_fullpath", C.c_bool),
                ("block_extract", C.c_bool)]


def state_from_png(path):
    """Raw mGBA state out of a .ss PNG (zlib data in chunk gbAs)."""
    b = Path(path).read_bytes()
    i = 8
    while i < len(b):
        n, t = struct.unpack(">I4s", b[i:i + 8])
        if t == b"gbAs":
            return zlib.decompress(b[i + 8:i + 8 + n])
        i += 12 + n
    raise ValueError("no gbAs chunk")


class Retro:
    def __init__(self, rom=ROM, video=True):
        SESSION.mkdir(parents=True, exist_ok=True)
        self.core = C.CDLL(str(DLL))
        self.video, self.keys, self.frame_buf = video, 0, None
        self.maps, self.fmt, self.frames = [], 0, 0
        self._dirs = [C.c_char_p(str(SESSION).encode()) for _ in range(2)]
        self._cbs = (ENV_CB(self._env), VIDEO_CB(self._video), AUDIO_CB(lambda d, n: n), SAMPLE_CB(lambda l, r: None),
                     POLL_CB(lambda: None), INPUT_CB(self._input))
        c = self.core
        c.retro_set_environment(self._cbs[0])
        c.retro_set_video_refresh(self._cbs[1])
        c.retro_set_audio_sample_batch(self._cbs[2])
        c.retro_set_audio_sample(self._cbs[3])
        c.retro_set_input_poll(self._cbs[4])
        c.retro_set_input_state(self._cbs[5])
        c.retro_init()
        si = SysInfo()
        c.retro_get_system_info(C.byref(si))
        self.version = f"{si.name.decode()} {si.version.decode()}"
        self.rom = Path(rom).read_bytes()
        self._buf = C.create_string_buffer(self.rom, len(self.rom))
        gi = GameInfo(str(rom).encode(), C.cast(self._buf, C.c_void_p), len(self.rom), None)
        if not c.retro_load_game(C.byref(gi)):
            raise RuntimeError("retro_load_game failed")
        c.retro_get_memory_data.restype = C.c_void_p

    def _env(self, cmd, data):
        if cmd == 10:  # SET_PIXEL_FORMAT
            self.fmt = C.cast(data, C.POINTER(C.c_int))[0]
        elif cmd in (9, 31):  # system / save dir
            C.cast(data, C.POINTER(C.c_char_p))[0] = self._dirs[0]
        elif cmd == 3:  # CAN_DUPE
            C.cast(data, C.POINTER(C.c_bool))[0] = True
        elif cmd == 15:  # GET_VARIABLE: defaults
            return False
        elif cmd == 17:  # VARIABLE_UPDATE
            C.cast(data, C.POINTER(C.c_bool))[0] = False
        elif cmd == 47 | EXP:  # GET_AUDIO_VIDEO_ENABLE: bit0 video, bit1 audio
            C.cast(data, C.POINTER(C.c_int))[0] = 3 if self.video else 0
        elif cmd == 36 | EXP:  # SET_MEMORY_MAPS
            m = C.cast(data, C.POINTER(MemMap))[0]
            self.maps = [(d.start, d.len, d.ptr, d.offset) for d in m.descs[:m.n]]
        else:
            return False
        return True

    def _video(self, data, w, h, pitch):
        if data and self.video:
            self.frame_buf = (C.string_at(data, pitch * h), w, h, pitch)

    def _input(self, port, dev, idx, id_):
        return (self.keys >> id_) & 1 if port == 0 and dev == 1 else 0

    def run(self, frames=1, keys=()):
        """keys: iterable of key names held for all frames, or callable(frame_no)->names."""
        for _ in range(frames):
            k = keys(self.frames) if callable(keys) else keys
            self.keys = sum(1 << KEY[x] for x in k)
            self.core.retro_run()
            self.frames += 1

    # The core sends no memory maps (self.maps stays empty). IWRAM/VRAM come from retro_get_memory_data (ids 2/3);
    # the rest is read/written inside the serialized state at GBASerializedState offsets (found by search, not docs).
    STATE = {2: (0x21000, 0x40000), 3: (0x19000, 0x8000), 4: (0x400, 0x400), 5: (0x800, 0x400), 6: (0x1000, 0x18000),
             7: (0xC00, 0x400)}
    DIRECT = {6: 3}

    def _span(self, addr, n):
        reg, off = addr >> 24, addr & 0xFFFFFF
        base, size = self.STATE[reg]
        if off + n > size:
            raise IndexError(f"{addr:#x}+{n} outside region")
        return reg, base, off

    def read(self, addr, n):
        reg, base, off = self._span(addr, n)
        if reg in self.DIRECT:
            return C.string_at(self.core.retro_get_memory_data(self.DIRECT[reg]) + off, n)
        return self.serialize()[base + off:base + off + n]

    def write(self, addr, data):
        reg, base, off = self._span(addr, len(data))
        if reg in self.DIRECT:
            return C.memmove(self.core.retro_get_memory_data(self.DIRECT[reg]) + off, data, len(data))
        s = bytearray(self.serialize())
        s[base + off:base + off + len(data)] = data
        assert self.unserialize(bytes(s))

    def serialize(self):
        n = self.core.retro_serialize_size()
        buf = C.create_string_buffer(n)
        assert self.core.retro_serialize(buf, n)
        return buf.raw

    def unserialize(self, raw):
        return bool(self.core.retro_unserialize(C.c_char_p(raw), len(raw)))

    def load_ss(self, path):
        raw = state_from_png(path)
        if not self.unserialize(raw):
            raise RuntimeError("unserialize rejected the state")

    def screenshot(self, path):
        from PIL import Image
        data, w, h, pitch = self.frame_buf
        mode = {0: "BGRA;15", 1: "BGRA", 2: "BGR;16"}
        if self.fmt == 1:  # XRGB8888
            im = Image.frombuffer("RGBA", (pitch // 4, h), data, "raw", "BGRA", 0, 1).crop((0, 0, w, h))
        else:  # RGB565
            im = Image.frombuffer("RGB", (pitch // 2, h), data, "raw", "BGR;16", 0, 1).crop((0, 0, w, h))
        im.convert("RGB").save(path)

    DOMAINS = (("wram", 0x02000000, 0x40000), ("iwram", 0x03000000, 0x8000), ("io", 0x04000000, 0x400),
               ("palette", 0x05000000, 0x400), ("vram", 0x06000000, 0x18000), ("oam", 0x07000000, 0x400))

    def dump(self, prefix):
        """Writes an oracle / `Dump::load` snapshot: work/e5298b24/PREFIX.<domain>.bin (no BIOS: the oracle runs SWIs itself)."""
        out = data_dir() / "work" / "e5298b24" / prefix
        out.parent.mkdir(parents=True, exist_ok=True)
        for name, addr, size in self.DOMAINS:
            Path(f"{out}.{name}.bin").write_bytes(self.read(addr, size))

    def ram_sha1(self):
        s = self.serialize()
        return hashlib.sha1(s[0x21000:0x61000] + s[0x19000:0x21000]).hexdigest()

if __name__ == "__main__":
    a = sys.argv[1:]
    r = Retro()
    if a[:1] == ["--ss"]:
        r.load_ss(a[1])
        a = a[2:]
    dump = a[a.index("--dump") + 1] if "--dump" in a else None
    a = [x for x in a if x not in ("--dump", dump)]
    r.run(int(a[0]) if a else 1)
    if dump:
        r.dump(dump)
    print(r.version, r.frames, r.ram_sha1())

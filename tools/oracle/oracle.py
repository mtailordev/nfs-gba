"""Function oracle: run any game function (ROM or IWRAM, ARM or Thumb) in unicorn on a RAM snapshot and report
its registers and every byte it changed. Ports are checked against it instead of driving mGBA into a state.

Python:
    sys.path.insert(0, "tools/oracle"); from oracle import Gba
    gba = Gba()                                   # canonical ROM + data/work/<sha8>/mgba/race.*.bin
    r = gba.call(0x0816A708, r0=7, r1=-2)         # __divsi3 (Thumb)
    r.regs["r0"], r.stop, r.writes, r.read(0x05000000, 512)

CLI (JSON lines on stdin, one result per line on stdout; Rust tests can spawn it or read saved output):
    .venv/Scripts/python.exe tools/oracle/oracle.py < requests.jsonl
    {"fn": "0x0816a708", "regs": {"r0": 7, "r1": -2}}
    {"fn": "0x0813a514", "regs": {"r0": "0x030000c0"}, "mem": [["0x03005614", "f8020000"]], "read": [["0x05000000", 512]]}
    Optional keys: "snapshot" (path under data/work/<sha8>/, default "mgba/race"), "mode" ("arm"/"thumb"),
    "stack" (words at sp), "align" ("stop"/"ignore"), "log_writes" (bool), "max_insns".

Hardware model (docs/engine/harness.md): the GBA memory map from the snapshot (BIOS, EWRAM, IWRAM + its
0x03FF8000 mirror, IO, palette, VRAM, OAM) and the ROM at 0x08/0x0A/0x0C000000 (read-only). The CPU model is
ARMv5TE (ARM946), the closest unicorn has to the ARM7TDMI. BIOS calls run in Python (`SWI`). No interrupts,
timers, DMA or video: IO reads return the snapshot's values, DMA starts are reported in `notes`.
Unaligned accesses stop the call (`align="stop"`, default): the ARM7TDMI rotates them and unicorn does not.
"""
import json
import struct
import sys
from dataclasses import dataclass, field
from pathlib import Path

import numpy as np
import unicorn
import unicorn.arm_const as A
from unicorn import UC_ARCH_ARM, UC_HOOK_INTR, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE, UC_MEM_WRITE, UC_MODE_ARM, Uc

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from common import data_dir  # noqa: E402
from mgba_ctl import canonical  # noqa: E402

REGIONS = [  # mGBA memory-domain name, base, size; all writable
    ("bios", 0x0000_0000, 0x4000),
    ("wram", 0x0200_0000, 0x4_0000),
    ("iwram", 0x0300_0000, 0x8000),
    ("io", 0x0400_0000, 0x400),
    ("palette", 0x0500_0000, 0x400),
    ("vram", 0x0600_0000, 0x1_8000),
    ("oam", 0x0700_0000, 0x400),
]
TRAP = 0x0F00_0000  # unused on the GBA; the return address every call gets
SP = 0x0300_7C00  # below the reference race's live stack (sp = 0x03007CE4 at the dump)
SCRATCH = 0x2000  # stack bytes below `sp` left out of `writes`
REG_NAMES = [f"r{i}" for i in range(13)] + ["sp", "lr", "pc", "cpsr"]
REGS = {n: getattr(A, f"UC_ARM_REG_{n.upper()}") for n in REG_NAMES}
DMA_CNT_H = {0x040000BA: 0, 0x040000C6: 1, 0x040000D2: 2, 0x040000DE: 3}


@dataclass
class Result:
    regs: dict
    stop: str  # "return", "halt", "intrwait", "vblank", "unaligned ...", "swi 0x..", "limit: ...", "error: ..."
    writes: list  # [(address, bytes)]: runs of changed bytes, stack scratch below sp left out
    notes: list = field(default_factory=list)
    log: list = field(default_factory=list)  # [(address, size, value)] of CPU writes when log_writes=True
    base: "Gba" = None

    def read(self, addr: int, n: int) -> bytes:
        """Memory after the call: the snapshot with this call's writes applied."""
        out = bytearray(self.base.read_base(addr, n))
        for a, b in self.writes:
            lo, hi = max(a, addr), min(a + len(b), addr + n)
            if lo < hi:
                out[lo - addr:hi - addr] = b[lo - a:hi - a]
        return bytes(out)


class Gba:
    def __init__(self, snapshot: str = "mgba/race"):
        rom_path, sha8 = canonical()
        self.snapshot = snapshot
        prefix = data_dir() / "work" / sha8 / snapshot
        rom = rom_path.read_bytes()
        self.rom = np.frombuffer(rom + bytes(-len(rom) % 0x400), dtype=np.uint8).copy()
        self.uc = uc = Uc(UC_ARCH_ARM, UC_MODE_ARM)
        uc.ctl_set_cpu_model(A.UC_CPU_ARM_946)
        self.base, self.mem = {}, {}
        for name, addr, size in REGIONS:
            f = Path(f"{prefix}.{name}.bin")
            data = f.read_bytes() if f.exists() else bytes(size)
            self.base[name] = np.frombuffer(data[:size].ljust(size, b"\0"), dtype=np.uint8).copy()
            self.mem[name] = self.base[name].copy()
            uc.mem_map_ptr(addr, size, unicorn.UC_PROT_ALL, self.mem[name].ctypes.data)
        uc.mem_map_ptr(0x03FF_8000, 0x8000, unicorn.UC_PROT_ALL, self.mem["iwram"].ctypes.data)  # IWRAM mirror
        for addr in (0x0800_0000, 0x0A00_0000, 0x0C00_0000):  # ROM and its wait-state mirrors
            uc.mem_map_ptr(addr, len(self.rom), unicorn.UC_PROT_READ | unicorn.UC_PROT_EXEC, self.rom.ctypes.data)
        uc.mem_map(TRAP, 0x400)
        uc.hook_add(UC_HOOK_INTR, self._intr)
        uc.hook_add(UC_HOOK_MEM_WRITE, self._dma, begin=0x040000B0, end=0x040000DF)
        self._mem_hook = None

    # --- memory helpers -------------------------------------------------------------------------------------------
    def _region(self, addr: int):
        if 0x03FF_8000 <= addr < 0x0400_0000:
            addr -= 0x00FF_8000
        for name, base, size in REGIONS:
            if base <= addr < base + size:
                return name, addr - base
        if 0x0800_0000 <= addr < 0x0E00_0000:
            return "rom", (addr - 0x0800_0000) % 0x0200_0000
        raise ValueError(f"unmapped address {addr:#010x}")

    def read_base(self, addr: int, n: int) -> bytes:
        """Snapshot bytes (before any call)."""
        name, off = self._region(addr)
        arr = self.rom if name == "rom" else self.base[name]
        return arr[off:off + n].tobytes()

    def poke(self, addr: int, data: bytes):
        """Change the snapshot itself (every later call sees it)."""
        name, off = self._region(addr)
        if name == "rom":
            raise ValueError("the ROM is read-only")
        arr = np.frombuffer(bytes(data), dtype=np.uint8)
        self.base[name][off:off + len(arr)] = arr
        self.mem[name][off:off + len(arr)] = arr
        self.uc.ctl_remove_cache(addr, addr + len(arr))  # code may have been translated from the old bytes

    def _invalidate(self, name: str):
        """Drop translated code of a region whose bytes changed behind the CPU's back."""
        base, size = next((b, s) for n, b, s in REGIONS if n == name)
        self.uc.ctl_remove_cache(base, base + size)
        if name == "iwram":
            self.uc.ctl_remove_cache(0x03FF_8000, 0x0400_0000)

    # --- the call ------------------------------------------------------------------------------------------------
    def call(self, fn: int, mode: str | None = None, regs: dict | None = None, stack=(), mem=(), align="stop",
             log_writes=False, keep=False, max_insns=100_000_000, **reg_kw) -> Result:
        """Run `fn` until it returns to the trap. `mode`: "thumb" if fn is odd or omitted for a ROM address, "arm"
        for IWRAM/EWRAM by default. `regs`/keyword registers: r0..r12, sp. `stack`: words placed at sp (5th and
        later arguments). `mem`: [(address, bytes)] applied for this call only. `keep=True` keeps the new state as
        the next call's starting point (not the snapshot)."""
        uc = self.uc
        thumb = mode == "thumb" or (mode is None and (fn & 1 or 0x0800_0000 <= fn < 0x0E00_0000))
        fn &= ~1
        regs = {**(regs or {}), **reg_kw}
        self._stop, self._notes, self._log = None, [], []
        uc.reg_write(A.UC_ARM_REG_CPSR, 0xDF)  # System mode, IRQ and FIQ masked, ARM (emu_start sets T)
        for n in REG_NAMES[:13]:
            uc.reg_write(REGS[n], regs.get(n, 0) & 0xFFFF_FFFF)
        sp = regs.get("sp", SP)
        uc.reg_write(A.UC_ARM_REG_SP, sp)
        uc.reg_write(A.UC_ARM_REG_LR, TRAP | thumb)
        # Stack words and `mem` are inputs: they join the baseline for this call, so `writes` shows only outputs.
        inputs = [(sp + 4 * i, struct.pack("<I", w & 0xFFFF_FFFF)) for i, w in enumerate(stack)]
        inputs += [(addr, bytes(data)) for addr, data in mem]
        saved = [(addr, self.read_base(addr, len(data))) for addr, data in inputs]
        for addr, data in inputs:
            self.poke(addr, data)
        want_hook = align == "stop" or log_writes
        if want_hook and self._mem_hook is None:
            self._mem_hook = uc.hook_add(UC_HOOK_MEM_READ | UC_HOOK_MEM_WRITE, self._access)
        elif not want_hook and self._mem_hook is not None:
            uc.hook_del(self._mem_hook)
            self._mem_hook = None
        self._align, self._logging = align == "stop", log_writes
        try:
            uc.emu_start(fn | thumb, TRAP, count=max_insns)  # a timeout would cost a thread per call
            pc = uc.reg_read(A.UC_ARM_REG_PC)
            stop = self._stop or ("return" if pc == TRAP else f"limit: {max_insns} instructions")
        except unicorn.UcError as e:
            stop = self._stop or f"error: {e} at pc {uc.reg_read(A.UC_ARM_REG_PC):#010x}"
        out = {n: uc.reg_read(REGS[n]) for n in REG_NAMES}
        writes, dirty = self._diff(sp)
        for name in dirty:
            if keep:  # the new state becomes the next call's start
                self.base[name][:] = self.mem[name]
            else:
                self.mem[name][:] = self.base[name]
                self._invalidate(name)
        if not keep:
            for addr, data in reversed(saved):
                self.poke(addr, data)
        return Result(out, stop, writes, self._notes, self._log, self)

    def _diff(self, sp: int):
        runs, dirty = [], []
        for name, base, _ in REGIONS:
            changed = np.flatnonzero(self.mem[name] != self.base[name])
            if changed.size:
                dirty.append(name)
            if name == "iwram":  # stack scratch below sp: restored, but not reported
                lo, hi = sp - SCRATCH - base, sp - base
                changed = changed[(changed < lo) | (changed >= hi)]
            if changed.size == 0:
                continue
            breaks = np.flatnonzero(np.diff(changed) > 1)
            starts = np.concatenate(([changed[0]], changed[breaks + 1]))
            ends = np.concatenate((changed[breaks], [changed[-1]])) + 1
            runs += [(base + int(s), self.mem[name][s:e].tobytes()) for s, e in zip(starts, ends)]
        return runs, dirty

    # --- hooks ---------------------------------------------------------------------------------------------------
    def _access(self, uc, access, addr, size, value, _):
        if self._align and addr & (size - 1):
            kind = "write" if access == UC_MEM_WRITE else "read"
            pc = uc.reg_read(A.UC_ARM_REG_PC)
            self._stop = f"unaligned {kind} of {size} at {addr:#010x} (pc {pc:#010x}; the ARM7TDMI would rotate)"
            uc.emu_stop()
        if self._logging and access == UC_MEM_WRITE:
            self._log.append((addr, size, value & ((1 << (8 * size)) - 1)))

    def _dma(self, uc, access, addr, size, value, _):
        for reg, ch in DMA_CNT_H.items():
            if addr <= reg < addr + size and (value >> (8 * (reg - addr))) & 0x8000:
                self._notes.append(f"DMA{ch} enabled (not emulated)")

    def _intr(self, uc, intno, _):
        pc = uc.reg_read(A.UC_ARM_REG_PC)
        if intno != 2:
            self._stop = f"error: exception {intno} at pc {pc:#010x}"
            uc.emu_stop()
            return
        thumb = uc.reg_read(A.UC_ARM_REG_CPSR) & 0x20
        insn = uc.mem_read(pc - 2, 2) if thumb else uc.mem_read(pc - 4, 4)
        num = insn[0] if thumb else insn[2]
        handler = SWI.get(num)
        if handler is None:
            self._stop = f"swi {num:#04x} (not implemented) at pc {pc - (2 if thumb else 4):#010x}"
            uc.emu_stop()
            return
        handler(self, uc)


# --- BIOS calls (GBATEK) -----------------------------------------------------------------------------------------
def _r(uc, i):
    return uc.reg_read(REGS[f"r{i}"])


def _w(uc, i, v):
    uc.reg_write(REGS[f"r{i}"], v & 0xFFFF_FFFF)


def _s32(v):
    return v - (1 << 32) if v & 0x8000_0000 else v


def _stop(reason):
    def f(gba, uc):
        gba._stop = reason
        uc.emu_stop()
    return f


def _div(num, den, gba, uc):
    if den == 0:  # the real BIOS never returns
        gba._stop = "error: BIOS Div by zero (the hardware hangs)"
        uc.emu_stop()
        return
    q = abs(num) // abs(den) * (1 if (num < 0) == (den < 0) else -1)
    _w(uc, 0, q)
    _w(uc, 1, num - q * den)
    _w(uc, 3, abs(q))


def _cpuset(gba, uc):
    src, dst, cnt = _r(uc, 0), _r(uc, 1), _r(uc, 2)
    if not src & 0x0E00_0000:  # the BIOS refuses sources in its own area
        return
    wide, fill, n = cnt >> 26 & 1, cnt >> 24 & 1, cnt & 0x1F_FFFF
    unit = 4 if wide else 2
    src, dst = src & ~(unit - 1), dst & ~(unit - 1)
    _copy(uc, src, dst, n, unit, fill)


def _cpufastset(gba, uc):
    src, dst, cnt = _r(uc, 0) & ~3, _r(uc, 1) & ~3, _r(uc, 2)
    if not src & 0x0E00_0000:
        return
    _copy(uc, src, dst, (cnt & 0x1F_FFFF) + 7 & ~7, 4, cnt >> 24 & 1)


def _copy(uc, src, dst, n, unit, fill):
    if fill:
        uc.mem_write(dst, bytes(uc.mem_read(src, unit)) * n)
    else:
        for i in range(n):  # unit by unit, so overlapping copies behave as on hardware
            uc.mem_write(dst + unit * i, bytes(uc.mem_read(src + unit * i, unit)))


def _lz77(vram):
    def f(gba, uc):
        src, dst = _r(uc, 0), _r(uc, 1)
        if not src & 0x0E00_0000:  # the BIOS refuses sources in its own area
            return
        size = int.from_bytes(uc.mem_read(src + 1, 3), "little")
        out = bytearray()
        s = src + 4

        def byte():
            nonlocal s
            s += 1
            return uc.mem_read(s - 1, 1)[0]

        def at(k):  # back-references read memory, also before dst
            return out[k] if k >= 0 else uc.mem_read(dst + k, 1)[0]

        while len(out) < size:
            flags = byte()
            for bit in range(8):
                if len(out) >= size:
                    break
                if flags & (0x80 >> bit):
                    hi, lo = byte(), byte()
                    n, disp = (hi >> 4) + 3, ((hi & 0xF) << 8 | lo) + 1
                    for _ in range(n):
                        out.append(at(len(out) - disp))
                else:
                    out.append(byte())
        del out[size:]
        if vram:  # halfword writes: a trailing odd byte is never stored
            del out[len(out) & ~1:]
        uc.mem_write(dst, bytes(out))
    return f


def _sqrt(gba, uc):
    import math
    _w(uc, 0, math.isqrt(_r(uc, 0)))


SWI = {
    0x02: _stop("halt"),
    0x04: _stop("intrwait"),
    0x05: _stop("vblank"),
    0x06: lambda g, u: _div(_s32(_r(u, 0)), _s32(_r(u, 1)), g, u),
    0x07: lambda g, u: _div(_s32(_r(u, 1)), _s32(_r(u, 0)), g, u),
    0x08: _sqrt,
    0x0B: _cpuset,
    0x0C: _cpufastset,
    0x11: _lz77(False),
    0x12: _lz77(True),
}


# --- CLI -----------------------------------------------------------------------------------------------------------
def _int(v):
    return int(v, 0) if isinstance(v, str) else int(v)


def main():
    machines = {}
    for line in sys.stdin:
        if not line.strip():
            continue
        q = json.loads(line)
        snap = q.get("snapshot", "mgba/race")
        gba = machines.get(snap) or machines.setdefault(snap, Gba(snap))
        r = gba.call(
            _int(q["fn"]),
            mode=q.get("mode"),
            regs={k: _int(v) for k, v in q.get("regs", {}).items()},
            stack=[_int(w) for w in q.get("stack", [])],
            mem=[(_int(a), bytes.fromhex(h)) for a, h in q.get("mem", [])],
            align=q.get("align", "stop"),
            log_writes=q.get("log_writes", False),
            max_insns=q.get("max_insns", 100_000_000),
        )
        out = {
            "regs": r.regs,
            "stop": r.stop,
            "writes": [[f"{a:#010x}", b.hex()] for a, b in r.writes],
            "reads": [r.read(_int(a), int(n)).hex() for a, n in q.get("read", [])],
            "notes": r.notes,
        }
        if r.log:
            out["log"] = [[f"{a:#010x}", s, v] for a, s, v in r.log]
        print(json.dumps(out), flush=True)


if __name__ == "__main__":
    main()

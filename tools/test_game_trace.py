"""Smoke test for tools/recorders/game.py: `pack` keeps the first state whole and every later state as byte runs that
rebuild it exactly (the format crates/nfsgba-game/src/trace.rs reads)."""
import os
import struct
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import numpy as np

from recorders import game as game_trace


def unpack(work: Path, name: str):
    s = np.fromfile(work / f"{name}.base.bin", dtype=np.uint8).copy()
    states = [s.copy()]
    d = (work / f"{name}.delta").read_bytes()
    at = 0
    while at < len(d):
        (n,) = struct.unpack_from("<I", d, at)
        at += 4
        for _ in range(n):
            o, ln = struct.unpack_from("<II", d, at)
            s[o:o + ln] = np.frombuffer(d, dtype=np.uint8, count=ln, offset=at + 8)
            at += 8 + ln
        states.append(s.copy())
    return states


class PackTest(unittest.TestCase):
    def test_round_trip(self):
        rng = np.random.default_rng(1)
        states = [rng.integers(0, 256, game_trace.STATE, dtype=np.uint8)]
        for _ in range(3):
            s = states[-1].copy()
            for at in rng.integers(0, game_trace.STATE - 40, 50):
                s[at:at + int(rng.integers(1, 40))] ^= 0x5A
            states.append(s)
        with tempfile.TemporaryDirectory() as tmp:
            work = Path(tmp)
            np.concatenate(states).tofile(work / "t.frames.bin")
            with mock.patch.object(game_trace, "session", lambda: work):
                game_trace.pack("t")
            self.assertFalse(os.path.exists(work / "t.frames.bin"))
            for a, b in zip(unpack(work, "t"), states):
                self.assertTrue(np.array_equal(a, b))


if __name__ == "__main__":
    unittest.main()

-- Marks the player's car finished (entity +0x4A = 2, what the lap logic does at the last lap): the car handler then
-- starts the race end (scenario `over` of tools/recorders/game.py; run with `lua` right after the state is loaded).
local entity = emu:read32(0x030000FC) + emu:read32(0x03000060) * 0xA4
emu:write16(entity + 0x4A, 2)

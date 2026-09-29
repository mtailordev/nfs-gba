-- Scenario setup for the "fadein" game trace: palette RAM and the sky gradient buffer (0x0200120C, the reference
-- race's) black, and main_frame's fade counter 0x03005630 = 20 (10 frames of fade in).
for i = 0, 0x3FF do emu:write8(0x05000000 + i, 0) end
for i = 0, 239 do emu:write8(0x0200120C + i, 0) end
emu:write32(0x03005630, 20)

-- Race-end probe (G1, phases 6-8), loaded AFTER game.lua (the `lua` remote command): mGBA runs the breakpoints of one
-- address last-registered first, so its breakpoint at main_frame's entry (0x0812AE64) runs before the recorder's. `luax TR3TIME=N` arms it: at the first
-- main_frame entry of a race frame the loop runs whole (game state 5, phase 2, no fade, start set-up done: game.lua's
-- `racing`) it writes N into the race time (0x03005800, the counter `hud_timer` reads; it sets phase 8 past 59:59.98), so
-- the recorded states (taken at that same entry) already hold it. The only poke: the triggering condition itself.
local function racing()
  return emu:read32(0x03005808) == 5 and emu:read32(0x03000048) == 2 and emu:read32(0x03005630) == 0
    and emu:read32(0x03005714) ~= 3
end

emu:setBreakpoint(function()
  if TR3TIME and racing() then
    emu:write32(0x03005800, TR3TIME)
    TR3TIME = nil
  end
end, 0x0812AE64)

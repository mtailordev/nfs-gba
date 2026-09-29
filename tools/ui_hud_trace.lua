-- Race HUD trace for mGBA (nightly), started by tools/ui_hud_trace.py. See docs/formats/ui.md ("HUD logic").
-- Loads savestate $NFSGBA_TRACE_STATE and records every race frame around the HUD: tag 0 at the call of
-- hud_update in race_frame_update (0x0813aa9c), tag 1 after the sprite_screen_update that follows it
-- (0x0813aaa8). Stops after $NFSGBA_TRACE_FRAMES frames and writes done.txt. $NFSGBA_TRACE_KEYS optionally
-- holds keys: "START:LEN:KEY[+KEY],..." (frames counted from the savestate).
-- Record: u32 tag, u32 frame; IWRAM 0x03000000..0x03000200 and 0x03005300..0x03006900; the sprite screen's
-- 0x37 objects (0x10 each, at *0x03000178); entities 0..3 (0xA4 each, at *(0x030000C0 + 0x3C)); for each
-- entity u32 driver pointer then 0x500 bytes of the driver (zeros when null); u32 profile +0x402 byte;
-- OBJ VRAM 0x06014000..0x06018000; OBJ palette 0x05000200..0x05000400; u16 DISPCNT.
local dir = os.getenv("NFSGBA_MGBA_DIR")
local state = os.getenv("NFSGBA_TRACE_STATE")
local frames = tonumber(os.getenv("NFSGBA_TRACE_FRAMES"))
local out = assert(io.open(dir .. "/" .. os.getenv("NFSGBA_TRACE_NAME") .. ".trace", "wb"))
local KEYS = {A = 0, B = 1, SELECT = 2, START = 3, RIGHT = 4, LEFT = 5, UP = 6, DOWN = 7, R = 8, L = 9}
local presses = {}
for start, len, keys in (os.getenv("NFSGBA_TRACE_KEYS") or ""):gmatch("(%d+):(%d+):([%u+]+)") do
  local mask = 0
  for k in keys:gmatch("%u+") do mask = mask | (1 << assert(KEYS[k], "unknown key " .. k)) end
  table.insert(presses, {tonumber(start), tonumber(start) + tonumber(len), mask})
end
local recorded, started, frame0 = 0, false, 0
local ZEROS = string.rep("\0", 0x500)

local function record(tag)
  if not out then return end
  out:write(string.pack("<I4I4", tag, emu:currentFrame()))
  out:write(emu:readRange(0x03000000, 0x200), emu:readRange(0x03005300, 0x1600))
  out:write(emu:readRange(emu:read32(0x03000178), 0x370))
  local entities = emu:read32(0x030000C0 + 0x3C)
  out:write(emu:readRange(entities, 4 * 0xA4))
  for i = 0, 3 do
    local d = emu:read32(entities + 0xA4 * i + 0x8C)
    out:write(string.pack("<I4", d))
    out:write(d ~= 0 and emu:readRange(d, 0x500) or ZEROS)
  end
  out:write(string.pack("<I4", emu:read8(emu:read32(0x030056EC) + 0x402)))
  out:write(emu:readRange(0x06014000, 0x4000), emu:readRange(0x05000200, 0x200))
  out:write(string.pack("<I2", emu:read16(0x04000000)))
  if tag == 1 then
    recorded = recorded + 1
    if recorded >= frames then
      out:close()
      out = nil
      local f = assert(io.open(dir .. "/done.txt", "w"))
      f:write("trace done")
      f:close()
    end
  end
end

callbacks:add("frame", function()
  if not started then
    started = true
    emu:loadStateFile(state)
    frame0 = emu:currentFrame()
    emu:setBreakpoint(function() record(0) end, 0x0813aa9c)
    emu:setBreakpoint(function() record(1) end, 0x0813aaa8)
  end
  local t, mask = emu:currentFrame() - frame0, 0
  for _, p in ipairs(presses) do
    if t >= p[1] and t < p[2] then mask = mask | p[3] end
  end
  emu:setKeys(mask)
end)

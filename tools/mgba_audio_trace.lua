-- LS_Play sound-engine trace for mGBA (nightly), started by tools/audio_trace.py. See docs/formats/audio.md.
-- Loads savestate $NFSGBA_TRACE_STATE, then records the engine around every call of the per-frame update
-- FUN_08151b10 (called from the VBlank handler at 0x0812ac4a): tag 0 on entry, tag 1 when it has returned
-- (0x0812ac4e). Calls into the engine's API made outside the update are logged as tag 2. Stops after
-- $NFSGBA_TRACE_FRAMES updates and writes done.txt. $NFSGBA_TRACE_KEYS optionally holds keys:
-- "START:LEN:KEY[+KEY],..." (frames counted from the savestate).
-- Tags 0 and 1: u32 tag, frame, engine address, buffer 0, buffer 1, buffer length (eng+0x144c); 16 bytes of the
-- globals at 0x03006370; the 0x160C-byte engine; buffer 0; buffer 1.
-- Tag 2: u32 tag, frame, function address, r0, r1, r2, r3.
local dir = os.getenv("NFSGBA_MGBA_DIR")
local state = os.getenv("NFSGBA_TRACE_STATE")
local frames = tonumber(os.getenv("NFSGBA_TRACE_FRAMES"))
local out = assert(io.open(dir .. "/" .. os.getenv("NFSGBA_TRACE_NAME") .. ".trace", "wb"))
local KEYS = {A = 0, B = 1, SELECT = 2, START = 3, RIGHT = 4, LEFT = 5, UP = 6, DOWN = 7, R = 8, L = 9}
local API = {0x08152e40, 0x08152f44, 0x08152f88, 0x08152fb8, 0x08152fec, 0x08151758, 0x0815240c, 0x081518d0,
             0x081518e4, 0x081518f8, 0x0815264c, 0x08151f78, 0x081516bc, 0x081522b8, 0x08152310}
local presses = {}
for start, len, keys in (os.getenv("NFSGBA_TRACE_KEYS") or ""):gmatch("(%d+):(%d+):([%u+]+)") do
  local mask = 0
  for k in keys:gmatch("%u+") do mask = mask | (1 << assert(KEYS[k], "unknown key " .. k)) end
  table.insert(presses, {tonumber(start), tonumber(start) + tonumber(len), mask})
end
local updates, started, inUpdate, frame0 = 0, false, false, 0

local function record(tag)
  if not out then return end
  inUpdate = tag == 0
  local eng = emu:read32(0x03006370)
  local b0, b1, n = emu:read32(eng), emu:read32(eng + 4), emu:read32(eng + 0x144c)
  out:write(string.pack("<I4I4I4I4I4I4", tag, emu:currentFrame(), eng, b0, b1, n))
  out:write(emu:readRange(0x03006370, 16), emu:readRange(eng, 0x160c), emu:readRange(b0, n), emu:readRange(b1, n))
  if tag == 1 then
    updates = updates + 1
    if updates >= frames then
      out:close()
      out = nil
      local f = assert(io.open(dir .. "/done.txt", "w"))
      f:write("trace done")
      f:close()
    end
  end
end

local function call(addr)
  if not out or inUpdate then return end
  out:write(string.pack("<I4I4I4I4I4I4I4", 2, emu:currentFrame(), addr, emu:readRegister("r0"),
    emu:readRegister("r1"), emu:readRegister("r2"), emu:readRegister("r3")))
end

callbacks:add("frame", function()
  if not started then
    started = true
    emu:loadStateFile(state)
    frame0 = emu:currentFrame()
    emu:setBreakpoint(function() record(0) end, 0x08151b10)
    emu:setBreakpoint(function() record(1) end, 0x0812ac4e)
    for _, a in ipairs(API) do emu:setBreakpoint(function() call(a) end, a) end
  end
  local t, mask = emu:currentFrame() - frame0, 0
  for _, p in ipairs(presses) do
    if t >= p[1] and t < p[2] then mask = mask | p[3] end
  end
  emu:setKeys(mask)
end)

-- Race HUD trace for mGBA (nightly), a probe of tools/recorders (`record.py hud`). See docs/formats/ui.md ("HUD logic").
-- Loads savestate $NFSGBA_TRACE_STATE and records every race frame around the HUD: tag 0 at the call of
-- hud_update in race_frame_update (0x0813aa9c), tag 1 after the sprite_screen_update that follows it
-- (0x0813aaa8). Stops after $NFSGBA_TRACE_FRAMES frames, or 600 frames after the last HUD frame (race over),
-- and writes done.txt. $NFSGBA_TRACE_KEYS optionally holds keys: "START:LEN:KEY[+KEY],..." (frames counted
-- from the savestate). $NFSGBA_TRACE_POKES optionally holds HUD inputs to set just before hud_update (so the
-- tag-0 record holds them): "START:LEN:TARGET[/SIZE]=VALUE,...", TARGET an address, "E<i>+off" (entity i) or
-- "D<i>+off" (entity i's driver), SIZE 1, 2 or 4 (default). A TARGET prefixed with "!" is temporary: the old
-- value is restored after the tag-1 record, so only the HUD sees the poke (for example a teleported car).
-- Calls made outside hud_update are recorded too: tag 2 at the entry and tag 3 at the exit of hud_message_show
-- (0x08142ec0 / 0x08142f6a), FUN_08142e44 (message cancel, 0x08142e44 / 0x08142eae) and hud_reset
-- (0x08142148 / 0x081421b2) and FUN_08143010 (HUD toggle, 0x08143010 / 0x08143052; it nests hud_reset):
-- u32 tag, u32 function, r0..r3, IWRAM 0x03000000..0x03000200 and
-- 0x03005300..0x03006900, the 0x37 objects.
-- Record (tags 0, 1): u32 tag, u32 frame; IWRAM 0x03000000..0x03000200 and 0x03005300..0x03006900; the sprite screen's
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
local pokes = {}
for start, len, target, size, value in (os.getenv("NFSGBA_TRACE_POKES") or ""):gmatch(
    "(%d+):(%d+):([^=,/]+)/?(%d?)=([^,]+)") do
  table.insert(pokes, {tonumber(start), tonumber(start) + tonumber(len), target, tonumber(size) or 4,
                       math.tointeger(tonumber(value))})
end
local recorded, started, frame0, last, restore = 0, false, 0, nil, {}
local ZEROS = string.rep("\0", 0x500)

local function read(a, size)
  if size == 1 then return emu:read8(a) elseif size == 2 then return emu:read16(a) end
  return emu:read32(a)
end

local function write(a, size, v)
  if size == 1 then emu:write8(a, v & 0xFF) elseif size == 2 then emu:write16(a, v & 0xFFFF)
  else emu:write32(a, v & 0xFFFFFFFF) end
end

local function address(target)
  local kind, i, off = target:gsub("^!", ""):match("^([ED])(%d)%+(.+)$")
  if not kind then return tonumber((target:gsub("^!", ""))) end
  local entity = emu:read32(0x030000C0 + 0x3C) + 0xA4 * tonumber(i)
  return (kind == "E" and entity or emu:read32(entity + 0x8C)) + tonumber(off)
end

local function finish()
  out:close()
  out = nil
  local f = assert(io.open(dir .. "/done.txt", "w"))
  f:write("trace done")
  f:close()
end

local function call(tag, fn)
  if not out then return end
  out:write(string.pack("<I4I4I4I4I4I4", tag, fn, emu:readRegister("r0"), emu:readRegister("r1"),
    emu:readRegister("r2"), emu:readRegister("r3")))
  out:write(emu:readRange(0x03000000, 0x200), emu:readRange(0x03005300, 0x1600))
  out:write(emu:readRange(emu:read32(0x03000178), 0x370))
end

local CALLS = {{0x08142ec0, 0x08142f6a}, {0x08142e44, 0x08142eae}, {0x08142148, 0x081421b2}, {0x08143010, 0x08143052}}

local function record(tag)
  if not out then return end
  if tag == 0 then
    local t = emu:currentFrame() - frame0
    for _, p in ipairs(pokes) do
      if t >= p[1] and t < p[2] then
        local a = address(p[3])
        if p[3]:sub(1, 1) == "!" then table.insert(restore, 1, {a, p[4], read(a, p[4])}) end
        write(a, p[4], p[5])
      end
    end
  end
  last = emu:currentFrame()
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
    for _, r in ipairs(restore) do write(r[1], r[2], r[3]) end
    restore = {}
    recorded = recorded + 1
    if recorded >= frames then finish() end
  end
end

callbacks:add("frame", function()
  if not started then
    started = true
    emu:loadStateFile(state)
    frame0 = emu:currentFrame()
    emu:setBreakpoint(function() record(0) end, 0x0813aa9c)
    emu:setBreakpoint(function() record(1) end, 0x0813aaa8)
    for _, c in ipairs(CALLS) do
      emu:setBreakpoint(function() call(2, c[1]) end, c[1])
      emu:setBreakpoint(function() call(3, c[1]) end, c[2])
    end
  end
  -- Race over: no HUD frame for 600 frames ends the trace after a whole tag-0/tag-1 pair.
  if out and last and emu:currentFrame() - last > 600 and recorded > 0 then
    local f = assert(io.open(dir .. "/trace-end.txt", "w"))
    f:write(string.format("no HUD frame since %d; pc %08x lr %08x\n", last, emu:readRegister("pc"),
      emu:readRegister("lr")))
    f:close()
    finish()
  end
  local t, mask = emu:currentFrame() - frame0, 0
  for _, p in ipairs(presses) do
    if t >= p[1] and t < p[2] then mask = mask | p[3] end
  end
  emu:setKeys(mask)
end)

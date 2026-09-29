-- Game-frame recorder for the game loop (docs/engine/game-loop.md), loaded next to tools/mgba_remote.lua
-- (NFSGBA_MGBA_SCRIPTS). Writing `NAME FRAMES` into gtrace.txt (write gtrace.tmp, then rename) arms it: from the
-- next entry of main_frame (0x0812AE64) it records FRAMES game frames. At each main_frame entry it appends the
-- whole machine state to NAME.frames.bin (EWRAM 256 KiB, IWRAM 32 KiB, palette 1 KiB, VRAM 96 KiB, OAM 1 KiB, in
-- that order) and a row to NAME.csv: the video frame, the held keys, and the VBlank counter 0x030053B4 at
-- main_frame's entry, at update_entities' entry (0x0813765C) and at hud_update's entry (0x08142F84), plus the
-- timer-3 ticks main_frame stored at 0x03005934 (read at update_entities). One extra state is written after the
-- last frame, so FRAMES frames give FRAMES + 1 states. tools/game_trace.py turns NAME.frames.bin into deltas.
local dir = os.getenv("NFSGBA_MGBA_DIR")
local name, left, csv, bin, row = nil, 0, nil, nil, nil
local inEntities = false

local function log(msg)
  local f = assert(io.open(dir .. "/gtrace_log.txt", "a"))
  f:write(emu:currentFrame() .. " " .. msg .. "\n")
  f:close()
end

local function state()
  for _, d in ipairs({"wram", "iwram", "palette", "vram", "oam"}) do
    local m = emu.memory[d]
    bin:write(m:readRange(0, m:size()))
  end
end

local function finishRow()
  if row then
    csv:write(table.concat(row, ",") .. "\n")
    row = nil
  end
end

emu:setBreakpoint(function()
  if not name then return end
  finishRow()
  state()
  if left == 0 then
    csv:close()
    bin:close()
    log("recorded " .. name)
    name = nil
    return
  end
  left = left - 1
  row = {emu:currentFrame(), emu:getKeys(), emu:read32(0x030053B4), "", "", "", "", "", "", ""}
end, 0x0812AE64)

emu:setBreakpoint(function()
  if row and row[4] == "" then
    row[4] = emu:read32(0x030053B4)
    row[6] = emu:read32(0x03005934)
    inEntities = true
  end
end, 0x0813765C)

emu:setBreakpoint(function()
  if row and row[5] == "" then row[5] = emu:read32(0x030053B4) end
  inEntities = false
end, 0x08142F84)

-- Finer IRQ placement (the game has no fixed points where the VBlank IRQ lands): the counter at every sound
-- call during update_entities (column 7, `;`-separated), at route_gap (8, reads the race time) and at hud_timer
-- (9, reads it too).
local function vb() return emu:read32(0x030053B4) end
local function sound()
  if row and inEntities then row[7] = row[7] .. (row[7] == "" and "" or ";") .. vb() end
end
for _, a in ipairs({0x08135FDC, 0x08136028, 0x081360B4}) do emu:setBreakpoint(sound, a) end
emu:setBreakpoint(function()
  local lr = emu:readRegister("lr")
  if lr < 0x08135FDC or lr >= 0x08136028 then sound() end -- carbon_play_sound calls it too
end, 0x08152E40)
emu:setBreakpoint(function() if row and row[8] == "" then row[8] = vb() end end, 0x0813EBAC)
-- The effect-sprite list as FUN_08161f38 (0x08161F38) gets it, before it clears one-shot sprites (column 10, hex):
-- what the matrix-slot code left, for replays that stand in for that code.
local function hex(s) return (s:gsub(".", function(c) return string.format("%02x", c:byte()) end)) end
emu:setBreakpoint(function()
  if row and row[10] == "" then
    local list = emu:read32(0x03000058)
    row[10] = hex(emu:readRange(list, 20 * emu:read16(0x0300005E)))
  end
end, 0x08161F38)
emu:setBreakpoint(function() if row and row[9] == "" then row[9] = vb() end end, 0x081428C0)

callbacks:add("frame", function()
  local f = io.open(dir .. "/gtrace.txt", "r")
  if not f then return end
  local line = f:read("l")
  f:close()
  if not line then return end
  os.remove(dir .. "/gtrace.txt")
  local n, count = line:match("^(%S+)%s+(%d+)")
  name, left = n, tonumber(count)
  csv = assert(io.open(dir .. "/" .. name .. ".csv", "w"))
  csv:write("video_frame,keys,vblanks_start,vblanks_entities,vblanks_hud,timer3,vblanks_sounds,vblanks_gap,vblanks_timer,effects\n")
  bin = assert(io.open(dir .. "/" .. name .. ".frames.bin", "wb"))
  log("armed " .. line)
end)

log("game trace ready")

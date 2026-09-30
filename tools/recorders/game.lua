-- Game-frame recorder for the game loop (docs/engine/game-loop.md), loaded next to tools/mgba_remote.lua
-- (NFSGBA_MGBA_EXTRA). Writing `NAME FRAMES` into gtrace.txt (write gtrace.tmp, then rename) arms it: from the
-- next entry of main_frame (0x0812AE64) it records FRAMES game frames. At each main_frame entry it appends the
-- whole machine state to NAME.frames.bin (EWRAM 256 KiB, IWRAM 32 KiB, palette 1 KiB, VRAM 96 KiB, OAM 1 KiB, in
-- that order) and a row to NAME.csv: the video frame, the held keys, and the VBlank counter 0x030053B4 at
-- main_frame's entry, at update_entities' entry (0x0813765C) and at hud_update's entry (0x08142F84), plus the
-- timer-3 ticks main_frame stored at 0x03005934 (read at update_entities). One extra state is written after the
-- last frame, so FRAMES frames give FRAMES + 1 states. `record.py game pack` turns NAME.frames.bin into deltas.
-- `NAME FRAMES racing` waits for the first main_frame entry of a race frame the game loop runs from start to end
-- (game state 5, race phase 2, no palette fade, the race-start set-up 0x03005714 done) before recording.
local dir = os.getenv("NFSGBA_MGBA_DIR")
local name, left, csv, bin, row = nil, 0, nil, nil, nil
local inEntities, waitRacing = false, false

-- `NAME FRAMES start`: from the main_frame entry that runs game state 4 (the race start), recorded whole.
local waitStart = false
local function racing()
  return emu:read32(0x03005808) == 5 and emu:read32(0x03000048) == 2 and emu:read32(0x03005630) == 0
    and emu:read32(0x03005714) ~= 3
end

local function log(msg)
  local f = assert(io.open(dir .. "/gtrace_log.txt", "a"))
  f:write(emu:currentFrame() .. " " .. msg .. "\n")
  f:close()
end

local function state(out)
  for _, d in ipairs({"wram", "iwram", "palette", "vram", "oam"}) do
    local m = emu.memory[d]
    local o = out or bin
    o:write(m:readRange(0, m:size()))
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
  if waitRacing then
    if not racing() then return end
    waitRacing = false
    log("racing from video frame " .. emu:currentFrame())
  end
  if waitStart then
    if emu:read32(0x03005808) ~= 4 then return end
    waitStart = false
    log("race start from video frame " .. emu:currentFrame())
  end
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
  row = {emu:currentFrame(), emu:getKeys(), emu:read32(0x030053B4), "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", ""}
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
-- call during update_entities (column 7, `;`-separated `entry-return` pairs: an IRQ can land inside a call, e.g.
-- during snd_set_sfx_rate's division, before the call changes the voice), at route_gap (8, reads the race time)
-- and at hud_timer (9, reads it too).
local function vb() return emu:read32(0x030053B4) end
local direct = false
local function sound()
  if row and inEntities then row[7] = row[7] .. (row[7] == "" and "" or ";") .. vb() end
end
local function returned()
  if row and inEntities then row[7] = row[7] .. "-" .. vb() end
end
-- carbon_play_sound, carbon_stop_sound, carbon_set_sound_rate: entries, then their returns.
for _, a in ipairs({0x08135FDC, 0x08136028, 0x081360B4}) do emu:setBreakpoint(sound, a) end
for _, a in ipairs({0x08136020, 0x08136048, 0x081360CA}) do emu:setBreakpoint(returned, a) end
-- snd_play_sfx when called directly (carbon_play_sound calls it too).
emu:setBreakpoint(function()
  local lr = emu:readRegister("lr")
  if lr < 0x08135FDC or lr >= 0x08136028 then direct = true; sound() end
end, 0x08152E40)
emu:setBreakpoint(function() if direct then direct = false; returned() end end, 0x08152F2E)
-- route_gap reads the race time twice around a division (0x0813ECB6/0x0813ECC0, 0x0813EDAC/0x0813EDB8):
-- column 12, the counter at each read, `;`-separated.
local function gapRead()
  if row then row[12] = row[12] .. (row[12] == "" and "" or ";") .. vb() end
end
for _, a in ipairs({0x0813ECB6, 0x0813ECC0, 0x0813EDAC, 0x0813EDB8}) do emu:setBreakpoint(gapRead, a) end
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
-- The opponents' lane-change timer restarts at the race time the AI reads at 0x0813C95C (FUN_0813c5a8, driver
-- struct in r7): column 11, `driver:counter` pairs (driver in hex), `;`-separated.
emu:setBreakpoint(function()
  if row then
    row[11] = row[11] .. (row[11] == "" and "" or ";") .. string.format("%x:%d", emu:readRegister("r7"), vb())
  end
end, 0x0813C95C)

-- The wingman marker's handler (FUN_0814bf98) reads the race time at 0x0814BFC0: column 19.
emu:setBreakpoint(function() if row then row[19] = vb() end end, 0x0814BFC0)

-- lap_crossing (FUN_0813f098) reads the race time at 0x0813F106 (driver struct in r5): column 20, `driver:counter`
-- pairs like the lane timers.
emu:setBreakpoint(function()
  if row then
    row[20] = row[20] .. (row[20] == "" and "" or ";") .. string.format("%x:%d", emu:readRegister("r5"), vb())
  end
end, 0x0813F106)

-- Marks of the race-start frame (columns 13..15, the VBlank counter at the first hit in a frame): rand_seed
-- (setup_race_cars' read of the tick counter), snd_play_module (the race music's request), race_start_from_table_b.
local function mark(col)
  return function() if row and row[col] == "" then row[col] = vb() end end
end
emu:setBreakpoint(mark(13), 0x0815FD1C)
emu:setBreakpoint(mark(14), 0x08151758)
emu:setBreakpoint(mark(15), 0x0813AF7C)

-- The hand-overs to the menus (columns 16..18, the VBlank counter): the pause block's entry (music_stop called from
-- race_frame_update, 0x0813A954..), the state-5 exit's entry (snd_stop_all called from game_state_step,
-- 0x0812ACEC..0x0812AE64), and the screen change either makes (goto_screen 0x0812BB5C / menu_back 0x0812D49C called
-- from one of them), where the machine state is also written whole to NAME.handover.bin.
local function from(lo, hi) local lr = emu:readRegister("lr") return lr >= lo and lr < hi end
local function inRace() return from(0x0813A954, 0x0813AE00) end
local function inState() return from(0x0812ACEC, 0x0812AE64) end
emu:setBreakpoint(function() if row and inRace() then row[16] = vb() end end, 0x0813609C)
emu:setBreakpoint(function() if row and inState() then row[17] = vb() end end, 0x08135F38)
local function handover()
  if row and (inRace() or inState()) then
    row[18] = vb()
    local f = assert(io.open(dir .. "/" .. name .. ".handover.bin", "wb"))
    state(f)
    f:close()
  end
end
emu:setBreakpoint(handover, 0x0812BB5C)
emu:setBreakpoint(handover, 0x0812D49C)

callbacks:add("frame", function()
  local f = io.open(dir .. "/gtrace.txt", "r")
  if not f then return end
  local line = f:read("l")
  f:close()
  if not line then return end
  os.remove(dir .. "/gtrace.txt")
  local n, count, cond = line:match("^(%S+)%s+(%d+)%s*(%S*)")
  name, left, waitRacing, waitStart = n, tonumber(count), cond == "racing", cond == "start"
  csv = assert(io.open(dir .. "/" .. name .. ".csv", "w"))
  csv:write("video_frame,keys,vblanks_start,vblanks_entities,vblanks_hud,timer3,vblanks_sounds,vblanks_gap,vblanks_timer,effects,lanes,gap_reads,seed,music,start,pause,exit,handover,marker,laps\n")
  bin = assert(io.open(dir .. "/" .. name .. ".frames.bin", "wb"))
  log("armed " .. line)
end)

log("game trace ready")

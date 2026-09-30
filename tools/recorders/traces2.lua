-- Breakpoint log for the ledger rows D14 (the grid deal's rand draws) and D8 (the player's rim redraw), loaded next to
-- tools/mgba_remote.lua (NFSGBA_MGBA_EXTRA, or the remote's `lua FILE`). Lines go to $NFSGBA_MGBA_DIR/<TR2NAME>.log
-- (the global TR2NAME, set with `luax TR2NAME="name"` before the race; default traces2); each line starts with the
-- video frame. Numbers are decimal.
--   SEED tick=T idx=I            rand_seed (0x0815FD1C, setup_race_cars) reads the tick counter T; I = the index after
--   R lr=L idx=I val=V idx2=J    every rand_table call (0x0815FCFC): caller, index before, value, index after
--   START tick=T idx=I / END idx=I cars=a,b,c,d paints=a,b,c,d    race_start_from_table_a (0x08139E34) entry / return
--   PICK opp=N wingman=W / PICKED cars=.. paints=..    pick_opponent_cars (0x0813B634) entry / return
--   M screen=S state=G tick=T vb=V keys=K   entry of main_frame (0x0812AE64): the screen and game state it runs, the tick
--                                and VBlank counters, the keys held (bit = KEYINPUT layout: A=1, B=2, ... as emu:getKeys)
--   G arg=A vb=V / W vb=V / X vb=V / MP id=I vb=V / MS vb=V   goto_screen (0x0812BB5C, screen A; 0x82 = resume a paused race),
--                                vblank_intr_wait (0x08151454), restart_engine_sound (0x08139E10), carbon_play_music
--                                (0x08136054), music_stop (0x0813609C), each with the VBlank counter 0x030053B4
--   (other)                      TR2MARK("text") logs a line (the scenario's marks, e.g. POKE)
--   E fn=F n=K player=P phase=H view=V heading=X yaw=Y   entry of FUN_0814A2A0 / FUN_0814B168 (entity K, its +0x2C heading,
--                                *0x03005F9C); V a=A b=B ret=R    rim_side_visible (0x0814F9D0); D lr=L    draw_decal_on_atlas
--                                (0x0813BD90)
local dir = os.getenv("NFSGBA_MGBA_DIR")
local buf = {}

local function log(msg)
  buf[#buf + 1] = emu:currentFrame() .. " " .. msg
end

local function flush()
  if #buf == 0 then return end
  local f = assert(io.open(dir .. "/" .. (TR2NAME or "traces2") .. ".log", "a"))
  f:write(table.concat(buf, "\n") .. "\n")
  f:close()
  buf = {}
end

local function onReturn(cb)
  local lr = emu:readRegister("lr") & 0xFFFFFFFE
  local id
  id = emu:setBreakpoint(function()
    emu:clearBreakpoint(id)
    cb()
  end, lr)
end

local function bytes(addr)
  local t = {}
  for i = 0, 3 do
    local b = emu:read8(addr + i)
    if b >= 128 then b = b - 256 end
    t[#t + 1] = b
  end
  return table.concat(t, ",")
end

local IDX, TICK = 0x030064C8, 0x03000044

emu:setBreakpoint(function()
  local tick = emu:read32(TICK)
  onReturn(function() log(string.format("SEED tick=%d idx=%d", tick, emu:read32(IDX))) end)
end, 0x0815FD1C)

emu:setBreakpoint(function()
  local lr, idx = emu:readRegister("lr") & 0xFFFFFFFE, emu:read32(IDX)
  onReturn(function()
    log(string.format("R lr=%d idx=%d val=%d idx2=%d", lr, idx, emu:readRegister("r0"), emu:read32(IDX)))
  end)
end, 0x0815FCFC)

emu:setBreakpoint(function()
  log(string.format("START tick=%d idx=%d", emu:read32(TICK), emu:read32(IDX)))
  onReturn(function()
    log(string.format("END idx=%d cars=%s paints=%s", emu:read32(IDX), bytes(0x0300611C), bytes(0x03005FEC)))
  end)
end, 0x08139E34)

emu:setBreakpoint(function()
  log(string.format("PICK opp=%d wingman=%d", emu:read32(0x03005784), emu:read32(0x03006104)))
  onReturn(function() log(string.format("PICKED cars=%s paints=%s", bytes(0x0300611C), bytes(0x03005FEC))) end)
end, 0x0813B634)

local function entered(fn)
  return function()
    local e = emu:readRegister("r1") -- (world, entity)
    log(string.format("E fn=%d n=%d player=%d phase=%d view=%d heading=%d yaw=%d", fn, emu:read16(e),
      emu:read32(0x03000060), emu:read32(0x03000048), emu:read32(0x030055F8), emu:read32(e + 0x2C),
      emu:read32(0x03005F9C)))
  end
end
emu:setBreakpoint(entered(0x0814A2A0), 0x0814A2A0)
emu:setBreakpoint(entered(0x0814B168), 0x0814B168)

emu:setBreakpoint(function()
  local a, b = emu:readRegister("r0"), emu:readRegister("r1")
  onReturn(function() log(string.format("V a=%d b=%d ret=%d", a, b, emu:readRegister("r0"))) end)
end, 0x0814F9D0)

emu:setBreakpoint(function()
  log(string.format("D lr=%d", emu:readRegister("lr") & 0xFFFFFFFE))
end, 0x0813BD90)

local VB = 0x030053B4
emu:setBreakpoint(function() log(string.format("G arg=%d vb=%d", emu:readRegister("r0"), emu:read32(VB))) end, 0x0812BB5C)
emu:setBreakpoint(function() log(string.format("W vb=%d", emu:read32(VB))) end, 0x08151454)
emu:setBreakpoint(function() log(string.format("X vb=%d", emu:read32(VB))) end, 0x08139E10)
emu:setBreakpoint(function() log(string.format("MP id=%d vb=%d", emu:readRegister("r0"), emu:read32(VB))) end, 0x08136054)
emu:setBreakpoint(function() log(string.format("MS vb=%d", emu:read32(VB))) end, 0x0813609C)

emu:setBreakpoint(function()
  log(string.format("M screen=%d state=%d tick=%d vb=%d keys=%d", emu:read32(0x03005944), emu:read32(0x03005808),
    emu:read32(TICK), emu:read32(0x030053B4), emu:getKeys()))
end, 0x0812AE64)

function TR2MARK(msg) log(msg) end

-- A key driver for a power-on run that does not depend on when the remote's batch arrived: `luax TR2DRIVE()` starts it
-- (after `luax TR2CHOICE={mode=0,opponents=3,wingman=0}`): the presses of tools/session_trace.py's script (made on the screen they leave), each
-- once its screen (0x03005944, game state 1) has been showing for the given number of frames, held for 12. On the
-- last screen (10, Quick Play setup) the race choice is poked first (the mode, the opponents and their settings copy,
-- the profile's wingman) and a POKE line logged.
local PLAN = {{25, 10, "A"}, {47, 30, "A"}, {47, 60, "A"}, {47, 90, "START"}, {48, 15, "A"}, {23, 600, "START"},
  {22, 59, "A"}, {22, 99, "START"}, {0, 26, "DOWN"}, {0, 66, "A"}, {28, 60, "A"}, {15, 100, "A"}, {10, 100, "A"}}
local KEYBIT = {A = 1, B = 2, SELECT = 4, START = 8, RIGHT = 16, LEFT = 32, UP = 64, DOWN = 128, R = 256, L = 512}
function TR2DRIVE()
  local i, holding, screen, since = 1, 0, -1, 0
  callbacks:add("frame", function()
    local f, now = emu:currentFrame(), emu:read32(0x03005944)
    if now ~= screen then screen, since = now, f end
    if holding > 0 then
      holding = holding - 1
      if holding == 0 then emu:setKeys(0) end
      return
    end
    local step = PLAN[i]
    if not step or screen ~= step[1] or emu:read32(0x03005808) ~= 1 or f - since < step[2] then return end
    if screen == 10 and TR2CHOICE then
      local c, profile = TR2CHOICE, emu:read32(0x030056EC)
      log("POKE")
      emu:write32(profile + 0x200, c.wingman)
      emu:write32(profile + 0x3C8, c.opponents)
      emu:write32(0x030056E0, c.mode)
      emu:write32(0x03005784, c.opponents)
    end
    emu:setKeys(KEYBIT[step[3]])
    holding, i = 12, i + 1
  end)
end

callbacks:add("frame", flush)

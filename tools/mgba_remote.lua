-- Remote control for mGBA (nightly, loaded with --script). Executes command batches written by
-- tools/mgba_ctl.py into $NFSGBA_MGBA_DIR/cmd.txt, one command per line, then writes done.txt:
--   hold KEY[,KEY...] FRAMES   hold keys (A B SELECT START RIGHT LEFT UP DOWN R L), then release
--   wait FRAMES
--   shot NAME                  screenshot to NAME.png
--   dump NAME                  every memory domain except the cartridge to NAME.<domain>.bin, registers to log
--   save NAME / load NAME      savestate NAME.ss
--   trace NAME / untrace       log every call of the car handler FUN_0814bd4c for entity 0 (the player) to
--                              NAME.csv, at its entry: frame, keys, the globals the car step reads, entity 0
--                              (0xA4 bytes) and its physics struct (entity +0x8C, 0x4FC bytes) as hex; the
--                              first call also dumps memory as NAME.<domain>.bin (docs/engine/physics.md)
local dir = os.getenv("NFSGBA_MGBA_DIR")
local KEYS = {A = 0, B = 1, SELECT = 2, START = 3, RIGHT = 4, LEFT = 5, UP = 6, DOWN = 7, R = 8, L = 9}
local queue, batch, wait, held = {}, nil, 0, false
local trace, traceName, breakpoint
-- Globals logged per car step (docs/engine/physics.md): dt, race phase, player input word, 0x0300610C, sector
local GLOBALS = {0x03005640, 0x03000048, 0x030057D8, 0x0300610C, 0x03005614}

local function writeFile(path, data, mode)
  local f = assert(io.open(path, mode or "wb"))
  f:write(data)
  f:close()
end

local function log(msg)
  writeFile(dir .. "/log.txt", emu:currentFrame() .. " " .. msg .. "\n", "a")
end

local function hex(s)
  return (s:gsub(".", function(c) return string.format("%02x", c:byte()) end))
end

local function dump(name)
  for domain, m in pairs(emu.memory) do
    if not domain:match("^cart") then writeFile(dir .. "/" .. name .. "." .. domain .. ".bin", m:readRange(0, m:size())) end
  end
  local regs = {}
  for _, r in ipairs({"r0", "r1", "r2", "r3", "r4", "r5", "r6", "r7", "sp", "lr", "pc", "cpsr"}) do
    table.insert(regs, string.format("%s=%08x", r, emu:readRegister(r)))
  end
  log("dump " .. name .. " " .. table.concat(regs, " "))
end

local function traceStep()
  local entity = emu:read32(0x030000C0 + 0x3C) -- world struct +0x3C: entity array, entity 0 = player
  if emu:readRegister("r1") ~= entity then return end
  if traceName then dump(traceName); traceName = nil end
  local globals = {}
  for _, a in ipairs(GLOBALS) do table.insert(globals, string.format("%d", emu:read32(a))) end
  trace:write(string.format("%d,%d,%s,%s,%s\n", emu:currentFrame(), emu:getKeys(), table.concat(globals, ","),
    hex(emu:readRange(entity, 0xA4)), hex(emu:readRange(emu:read32(entity + 0x8C), 0x4FC))))
end

local function poll()
  local f = io.open(dir .. "/cmd.txt", "r")
  if not f then return end
  local text = f:read("a")
  f:close()
  os.remove(dir .. "/cmd.txt")
  for line in text:gmatch("[^\r\n]+") do
    local id = line:match("^id (%d+)$")
    if id then batch = id else table.insert(queue, line) end
  end
end

local function run(line)
  local op, a, b = line:match("^(%S+)%s*(%S*)%s*(%S*)")
  if op == "hold" then
    local mask = 0
    for k in a:gmatch("[^,]+") do mask = mask | (1 << assert(KEYS[k], "unknown key " .. k)) end
    emu:setKeys(mask)
    held, wait = true, tonumber(b)
  elseif op == "wait" then
    wait = tonumber(a)
  elseif op == "shot" then
    emu:screenshot(dir .. "/" .. a .. ".png")
  elseif op == "dump" then
    dump(a)
  elseif op == "save" then
    emu:saveStateFile(dir .. "/" .. a .. ".ss")
  elseif op == "load" then
    emu:loadStateFile(dir .. "/" .. a .. ".ss")
  elseif op == "trace" then
    trace, traceName = assert(io.open(dir .. "/" .. a .. ".csv", "w")), a
    trace:write("frame,keys,dt,phase,input,flag610c,sector,entity,physics\n")
    breakpoint = emu:setBreakpoint(traceStep, 0x0814BD4C)
  elseif op == "untrace" then
    if breakpoint then emu:clearBreakpoint(breakpoint) end
    if trace then trace:close() end
    trace, traceName, breakpoint = nil, nil, nil
  else
    error("unknown command")
  end
end

callbacks:add("frame", function()
  if wait > 0 then
    wait = wait - 1
    if wait == 0 and held then emu:setKeys(0); held = false end
    return
  end
  if #queue == 0 then
    if batch then writeFile(dir .. "/done.txt", batch); batch = nil end
    if emu:currentFrame() % 4 == 0 then poll() end
    return
  end
  local line = table.remove(queue, 1)
  local ok, err = pcall(run, line)
  if not ok then log("error in '" .. line .. "': " .. tostring(err)) end
end)

log("remote ready")

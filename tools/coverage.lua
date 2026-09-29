-- Function coverage for tools/coverage.py: a breakpoint on every function entry counts hits while a key plan runs.
-- Env: NFSGBA_MGBA_DIR (session dir), COV_FUNCS (file, one hex address per line), COV_OUT (csv to write),
-- COV_PLAN (comma-separated steps):
--   load:NAME        load savestate NAME.ss from the session dir (breakpoints stay)
--   mark:LABEL       count further hits under LABEL (default "start")
--   KEY[+KEY]:N      hold keys for N frames (A B SELECT START RIGHT LEFT UP DOWN R L), "none:N" waits
--   shot:NAME        screenshot NAME.png
-- When the plan ends it writes COV_OUT ("label,address,hits" for every non-zero count) and done.txt.
local dir = os.getenv("NFSGBA_MGBA_DIR")
local KEYS = {A = 0, B = 1, SELECT = 2, START = 3, RIGHT = 4, LEFT = 5, UP = 6, DOWN = 7, R = 8, L = 9}
local counts, cur = {}, {}
counts.start = cur
local order = {"start"}

local plan = {}
for step in (os.getenv("COV_PLAN") or ""):gmatch("[^,]+") do
  local op, arg = step:match("^([%w+]+):(.+)$")
  table.insert(plan, {op, arg})
end

local function hooks()
  for line in io.lines(os.getenv("COV_FUNCS")) do
    local a = tonumber(line, 16)
    if a then emu:setBreakpoint(function() cur[a] = (cur[a] or 0) + 1 end, a) end
  end
end

local function finish()
  local f = assert(io.open(os.getenv("COV_OUT"), "w"))
  for _, label in ipairs(order) do
    for a, n in pairs(counts[label]) do f:write(string.format("%s,0x%08x,%d\n", label, a, n)) end
  end
  f:close()
  local d = assert(io.open(dir .. "/done.txt", "w"))
  d:write("coverage\n")
  d:close()
end

local step, left, started = 0, 0, false
local function start()
  if not started then hooks(); started = true end
end
callbacks:add("start", start) -- before the first instruction, so boot code is counted
if emu then start() end -- script loaded after the game
callbacks:add("frame", function()
  start()
  if left > 0 then
    left = left - 1
    if left == 0 then emu:setKeys(0) end
    return
  end
  step = step + 1
  local p = plan[step]
  if not p then
    if step == #plan + 1 then finish() end
    return
  end
  local op, arg = p[1], p[2]
  if op == "load" then
    emu:loadStateFile(dir .. "/" .. arg .. ".ss")
  elseif op == "mark" then
    if not counts[arg] then counts[arg] = {}; table.insert(order, arg) end
    cur = counts[arg]
  elseif op == "shot" then
    emu:screenshot(dir .. "/" .. arg .. ".png")
  else
    local mask = 0
    for k in op:gmatch("[^+]+") do if KEYS[k] then mask = mask | (1 << KEYS[k]) end end
    emu:setKeys(mask)
    left = tonumber(arg)
  end
end)

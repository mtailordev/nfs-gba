-- Race-start capture, loaded next to tools/mgba_remote.lua (NFSGBA_MGBA_SCRIPTS). Writing a NAME into
-- raceinit.txt (write raceinit.tmp, then rename it) arms it. At the next entry of race_start_from_table_a
-- (0x08139E34, called by game_state_step's state 4) every memory domain except the cartridge is saved as
-- NAME_pre.<domain>.bin; at its return (the caller's lr) again as NAME_post.<domain>.bin. raceinit_log.txt gets
-- the registers and the VBlank counter (0x030053B4) at both points, so IRQ effects in between can be told apart.
local dir = os.getenv("NFSGBA_MGBA_DIR")
local want, ret = nil, nil

local function log(msg)
  local f = assert(io.open(dir .. "/raceinit_log.txt", "a"))
  f:write(emu:currentFrame() .. " " .. msg .. "\n")
  f:close()
end

local function save(name)
  for domain, m in pairs(emu.memory) do
    if not domain:match("^cart") then
      local f = assert(io.open(dir .. "/" .. name .. "." .. domain .. ".bin", "wb"))
      f:write(m:readRange(0, m:size()))
      f:close()
    end
  end
  local regs = {}
  for _, r in ipairs({"r0", "r1", "r2", "r3", "sp", "lr", "pc", "cpsr"}) do
    table.insert(regs, string.format("%s=%08x", r, emu:readRegister(r)))
  end
  log(name .. " " .. table.concat(regs, " ") .. string.format(" vblanks=%d", emu:read32(0x030053B4)))
end

emu:setBreakpoint(function()
  if not want then return end
  save(want .. "_pre")
  local name = want
  want = nil
  local lr = emu:readRegister("lr") & 0xFFFFFFFE
  ret = emu:setBreakpoint(function()
    if not ret then return end
    emu:clearBreakpoint(ret)
    ret = nil
    save(name .. "_post")
  end, lr)
end, 0x08139E34)

callbacks:add("frame", function()
  local f = io.open(dir .. "/raceinit.txt", "r")
  if not f then return end
  local line = f:read("l")
  f:close()
  if not line then return end
  os.remove(dir .. "/raceinit.txt")
  want = line:match("^(%S+)")
  log("armed " .. want)
end)

log("race-init capture ready")

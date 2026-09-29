-- Frame probe for the race renderer, loaded next to tools/mgba_remote.lua (NFSGBA_MGBA_SCRIPTS). Writing a line
-- `NAME [ADDR=VALUE ...]` into probe.txt arms it. At the next start of draw_visible_sectors (IWRAM 0x030048C8,
-- after the camera and the visible list) it first applies the optional 16-bit writes (hex; to steer the renderer
-- into a path the data does not reach on its own), then keeps IWRAM and EWRAM; at that call's end (0x03004964) it
-- adds VRAM; at the next frame's start it also keeps the same page again, now with what the game drew after the
-- world (effects, HUD). Saved as NAME.iwram.bin / NAME.wram.bin / NAME.vram.bin / NAME.final.bin.
local dir = os.getenv("NFSGBA_MGBA_DIR")
local want, writes, taken, page = nil, {}, nil, nil

local function log(msg)
  local f = assert(io.open(dir .. "/probe_log.txt", "a"))
  f:write(emu:currentFrame() .. " " .. msg .. "\n")
  f:close()
end

local function put(name, ext, data)
  local f = assert(io.open(dir .. "/" .. name .. "." .. ext .. ".bin", "wb"))
  f:write(data)
  f:close()
end

emu:setBreakpoint(function()
  if page then
    put(page.name, "final", emu:readRange(page.at, 240 * 160))
    log("saved " .. page.name)
    page = nil
  end
  if want and not taken then
    for _, w in ipairs(writes) do emu:write16(w[1], w[2]) end
    taken = {
      iwram = emu.memory.iwram:readRange(0, emu.memory.iwram:size()),
      wram = emu.memory.wram:readRange(0, emu.memory.wram:size()),
    }
  end
end, 0x030048C8)

emu:setBreakpoint(function()
  if taken then
    put(want, "iwram", taken.iwram)
    put(want, "wram", taken.wram)
    put(want, "vram", emu.memory.vram:readRange(0, emu.memory.vram:size()))
    page = {name = want, at = emu:read32(0x03000080)}
    want, taken = nil, nil
  end
end, 0x03004964)

callbacks:add("frame", function()
  local f = io.open(dir .. "/probe.txt", "r")
  if not f then return end
  local line = f:read("l")
  f:close()
  if not line then return end -- still being written (write probe.tmp, then rename it)
  os.remove(dir .. "/probe.txt")
  writes = {}
  want = line:match("^(%S+)")
  for a, v in line:gmatch("(%x+)=(%x+)") do table.insert(writes, {tonumber(a, 16), tonumber(v, 16)}) end
  taken = nil
  log("armed " .. line)
end)

log("frame probe ready")

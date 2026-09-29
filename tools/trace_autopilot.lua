-- Autopilot for recording car traces (docs/engine/physics.md), loaded with `lua <path>/trace_autopilot.lua`
-- (reloading replaces the logic). Each frame it reads the player's position and heading and steers towards a
-- target, holding A (and any extra keys). Deterministic: it only reads emulated RAM. Control it with `luax`:
--   luax AUTOPILOT.mode="race"   follow the car's own racing-line section, AUTOPILOT.ahead waypoints ahead
--   luax AUTOPILOT.mode="ram"    steer at entity AUTOPILOT.target
--   luax AUTOPILOT.mode="off"    leave the keys alone (hold/wait commands work again)
--   AUTOPILOT.extra = key mask OR'd in (e.g. 0x200 L for nitro with A); AUTOPILOT.gas = false releases A;
--   AUTOPILOT.section / AUTOPILOT.index: follow that racing-line section from that waypoint (a shortcut branch);
--   AUTOPILOT.recover = false: no reversing out when stuck;
--   AUTOPILOT.tipped = N: after N tipped-over steps, raise the tipped counter to 100 (test input for the reset);
--   AUTOPILOT.branch = S, AUTOPILOT.branch_at = N: on the lap at waypoint N, drive racing-line section S (a
--   shortcut, often behind a breakable wall), then rejoin the lap.
-- Steering: heading and target angles are 0x4000 per turn, measured from +z towards +x (the game's atan2);
-- LEFT turns towards smaller angles, RIGHT towards larger (measured in the recorded traces).
AUTOPILOT = AUTOPILOT or {}
local ap = AUTOPILOT
-- Every scenario starts with `luax AUTOPILOT.reset()`, so no setting carries over between recordings.
function ap.reset()
  ap.mode, ap.ahead, ap.extra, ap.dead, ap.gas = "off", 1, 0, 0x60, true
  ap.hunt, ap.target, ap.section, ap.index, ap.recover, ap.log = nil, nil, nil, nil, nil, nil
  ap.reverse, ap.stuck, ap.diff = 0, 0, 0
  ap.tipped, ap.tipped_done, ap.branch, ap.branch_at = nil, nil, nil, nil
  emu:setKeys(0)
end
if not ap.loaded then ap.reset() end
local WORLD = 0x030000C0
local RIGHT, LEFT = 0x10, 0x20
local function s32(v) return v >= 0x80000000 and v - 0x100000000 or v end
local function entity(i) return emu:read32(WORLD + 0x3C) + i * 0xA4 end
local function waypoint(section, index)
  local tbl, line = emu:read32(WORLD + 0x40), emu:read32(WORLD + 0x44)
  local count, first = emu:read16(tbl + section * 8), emu:read32(tbl + section * 8 + 4)
  if count < 2 then return nil end
  if section == 0 then index = index % (count - 1) elseif index >= count then index = count - 1 end
  local w = line + (first + index) * 24
  return s32(emu:read32(w)), s32(emu:read32(w + 4))
end

function ap.step()
  if ap.mode == "off" or emu:read32(0x03000048) == 0 then
    -- Release what the autopilot held, once, so later hold commands start from no keys.
    if ap.driving then emu:setKeys(0); ap.driving = false end
    return
  end
  ap.driving = true
  local e = entity(emu:read32(0x03000060))
  if ap.tipped and not ap.tipped_done then
    -- Test input (RAM, not ROM): once the player has been tipped over for ap.tipped steps, set the tipped-step
    -- counter (physics +0x4E4) to 100, so the stuck reset FUN_0814efa8 runs without 100 real tipped steps.
    local p = emu:read32(e + 0x8C)
    if emu:read16(p + 0x4E4) >= ap.tipped then emu:write16(p + 0x4E4, 100); ap.tipped_done = true end
  end
  local x, z = s32(emu:read32(e + 0x0C)) // 256, s32(emu:read32(e + 0x14)) // 256
  local tx, tz
  if ap.branch and not ap.section and emu:read16(e + 0x72) == 0 and emu:read16(e + 0x90) == ap.branch_at then
    -- Take the shortcut: follow section ap.branch from its first waypoint (once).
    ap.section, ap.index, ap.branch = ap.branch, 0, nil
  end
  if ap.mode == "ram" then
    local t = entity(ap.target or 1)
    tx, tz = s32(emu:read32(t + 0x0C)) // 256, s32(emu:read32(t + 0x14)) // 256
  elseif ap.section then
    tx, tz = waypoint(ap.section, ap.index + ap.ahead)
    if tx and (tx - x) ^ 2 + (tz - z) ^ 2 < 1500 ^ 2 then ap.index = ap.index + 1 end
    -- Past the section's last waypoint: back to the racing line.
    if ap.index + ap.ahead >= emu:read16(emu:read32(WORLD + 0x40) + ap.section * 8) then ap.section = nil end
  else
    tx, tz = waypoint(emu:read16(e + 0x72), emu:read16(e + 0x90) + ap.ahead)
  end
  if ap.hunt then
    -- Steer at the nearest other racer within ap.hunt units instead.
    local best = ap.hunt ^ 2
    for i = 1, emu:read32(0x030057EC) do
      local t = entity(i)
      local ox, oz = s32(emu:read32(t + 0x0C)) // 256, s32(emu:read32(t + 0x14)) // 256
      local d = (ox - x) ^ 2 + (oz - z) ^ 2
      if d < best then best, tx, tz = d, ox, oz end
    end
  end
  local keys = ap.extra
  local speed = emu:read32(emu:read32(e + 0x8C) + 0x44)
  if (ap.reverse or 0) > 0 then
    -- Stuck recovery: back off with the opposite lock.
    ap.reverse = ap.reverse - 1
    keys = keys | 2 | ((ap.diff or 0) > 0 and LEFT or RIGHT)
  elseif tx then
    local want = math.floor(math.atan(tx - x, tz - z) * 0x4000 / (2 * math.pi)) & 0x3FFF
    local heading = (emu:read32(e + 0x2C) >> 8) & 0x3FFF
    local diff = (want - heading + 0x2000) % 0x4000 - 0x2000
    if diff > ap.dead then keys = keys | RIGHT elseif diff < -ap.dead then keys = keys | LEFT end
    -- Coast through sharp turns.
    if ap.gas and (math.abs(diff) < 0xA00 or speed < 0x6000) then keys = keys | 1 end
    ap.diff = diff
    ap.stuck = (ap.gas and speed < 0x800 and emu:read32(0x03000048) == 2) and (ap.stuck or 0) + 1 or 0
    if ap.stuck > 60 and ap.recover ~= false then ap.stuck, ap.reverse = 0, 45 end
  end
  emu:setKeys(keys)
  if ap.log and emu:currentFrame() % 10 == 0 then
    local f = io.open(os.getenv("NFSGBA_MGBA_DIR") .. "/autopilot.txt", "a")
    f:write(string.format("%d pos=(%d,%d) target=(%s,%s) heading=%d diff=%s speed=%d keys=%x rev=%d\n",
      emu:currentFrame(), x, z, tostring(tx), tostring(tz), (emu:read32(e + 0x2C) >> 8) & 0x3FFF,
      tostring(ap.diff), speed, keys, ap.reverse or 0))
    f:close()
  end
end

if not ap.loaded then
  ap.loaded = true
  callbacks:add("frame", function() ap.step() end)
end

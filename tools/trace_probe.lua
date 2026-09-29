-- Probe for the car-physics paths (docs/engine/physics.md), loaded with the remote command `lua <path>/trace_probe.lua`.
-- Counts calls of the functions below, split into the player's (an argument register holds the player's entity)
-- and others, logs the first calls of each and every change of the race phase (0x03000048) and of 0x0300610C
-- to probe.txt in the session directory, and writes the counts every 600 frames.
local dir = os.getenv("NFSGBA_MGBA_DIR")
local out = assert(io.open(dir .. "/probe.txt", "a"))
-- name, address, register holding the entity (for the player split)
local FUNCS = {
  {"car_suspension", 0x0814DE40, "r1"}, {"car_tipped_dynamics", 0x081484F0, "r1"},
  {"car_put_back_on_road", 0x0814EFA8, "r1"}, {"car_car_response", 0x08144FA4, "r2"},
  {"hunter_hit", 0x0814101C, "r0"}, {"find_sector_far", 0x0814DBBC, "r0"},
  {"wingman_command", 0x0814078C, "r1"}, {"traffic_spawn", 0x08143D48, "r2"},
  {"hunter_life_tick", 0x08140F78, "r1"}, {"break_wall", 0x0813B5A0, "r0"},
  {"hunter_wall_hit", 0x0814136C, "r0"}, {"push_back_loop", 0x0813DF98, "r0"},
  {"route_side_segment", 0x0813F234, "r1"}, {"traffic_hit", 0x08145DAC, "r1"},
  {"car_put_back_on_road@", 0x0814EFA8, "r1"},
}
local counts = {}
local function line(s) out:write(emu:currentFrame() .. " " .. s .. "\n"); out:flush() end
local function player() return emu:read32(0x030000C0 + 0x3C) + emu:read32(0x03000060) * 0xA4 end
for _, f in ipairs(FUNCS) do
  local name, addr, reg = f[1], f[2], f[3]
  emu:setBreakpoint(function()
    local key = name
    if name == "traffic_spawn" then key = name .. emu:readRegister("r2")
    elseif emu:readRegister(reg) == player() then key = name .. "@player" end
    counts[key] = (counts[key] or 0) + 1
    if counts[key] <= 3 then
      line(string.format("%s r0=%08x r1=%08x r2=%08x r3=%08x phase=%d", key, emu:readRegister("r0"),
        emu:readRegister("r1"), emu:readRegister("r2"), emu:readRegister("r3"), emu:read32(0x03000048)))
    end
  end, addr)
end
local phase, flag
callbacks:add("frame", function()
  local p, f = emu:read32(0x03000048), emu:read32(0x0300610C)
  if p ~= phase or f ~= flag then line(string.format("phase %d flag610c %d", p, f)); phase, flag = p, f end
  if emu:currentFrame() % 600 == 0 then
    local s = {}
    for name, n in pairs(counts) do table.insert(s, name .. "=" .. n) end
    table.sort(s)
    line("counts " .. table.concat(s, " "))
  end
end)
line("probe ready")

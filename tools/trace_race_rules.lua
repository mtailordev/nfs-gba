-- Race-rule tracer for mGBA (loaded by tools/mgba_remote.lua through NFSGBA_MGBA_EXTRA). Appends one line per
-- traced call to $NFSGBA_MGBA_DIR/rr.log: "frame=F fn=NAME pre.KEY=V... post.KEY=V..." with comma-separated ints
-- (see docs/formats/career.md, "Race-rule traces"). Racer fields, in order (RACER below):
--   id sec seg state eflags x z place dist best lapst fin left flags life ww wall hit ko0 ko1 ko2 f444 f4d6
-- Globals (GLOBALS): mode lapped laps opponents time finished view player difficulty state48 rand ww_flag route
-- Commands in rr_cmd.txt (polled every 8 frames): "auto on", "auto off", "auto back N" (reverse for N frames),
-- "mark TEXT", "every N" (sample period for unchanged per-frame calls; 0 = changes only), "log NAME" (write to
-- NAME.log from now on), "planes" (log the world line and plane tables now, as fn=planes_now).
local dir = os.getenv("NFSGBA_MGBA_DIR")
local out = assert(io.open(dir .. "/rr.log", "a"))
local every = 64

local function s(v, bits) return v >= (1 << (bits - 1)) and v - (1 << bits) or v end
local function r8(a) return emu:read8(a) end
local function r16(a) return emu:read16(a) end
local function r32(a) return emu:read32(a) end
local function i8(a) return s(emu:read8(a), 8) end
local function i16(a) return s(emu:read16(a), 16) end
local function i32(a) return s(emu:read32(a), 32) end
local function hex(a, n)
  local t = {}
  for i = 0, n - 1 do t[#t + 1] = string.format("%02x", emu:read8(a + i)) end
  return table.concat(t)
end

local WORLD = 0x030000C0
local function entities() return r32(WORLD + 0x3C) end
local function racers() return r32(0x03005784) + 1 end

local function racer(e)
  local d = r32(e + 0x8C)
  if d < 0x02000000 or d >= 0x02040000 then return nil end
  return table.concat({
    r16(e), r16(e + 0x72), i16(e + 0x90), r16(e + 0x4A), r16(e + 0x08), i32(e + 0x0C), i32(e + 0x14),
    i32(d + 0xA8), i32(d + 0xAC), r32(d + 0xB4), r32(d + 0xB8), r32(d + 0xBC), i8(d + 0xC5), r16(d + 0x4D8),
    i32(d + 0x4E8), i16(d + 0x4EC), i16(d + 0x4EE), i16(d + 0x4F0), r32(d + 0xF8), r32(d + 0xFC), r32(d + 0x100),
    i32(d + 0x444), r16(d + 0x4D6),
  }, ",")
end

local function globals()
  return table.concat({
    r32(0x030056E0), r32(0x0300608C), i32(0x030056E4), r32(0x03005784), r32(0x03005800), r32(0x030061A4),
    r32(0x030057F8), r32(0x03000060), r32(0x03005608), r32(0x03000048), r32(0x030064C8), r32(0x03005384),
    r32(0x03005720),
  }, ",")
end

-- Every racer, the globals and the results block (0x03005650, 0x40 bytes).
local function race(tag, upto)
  local t, e = {}, entities()
  t[#t + 1] = tag .. ".g=" .. globals()
  t[#t + 1] = tag .. ".res=" .. hex(0x03005650, 0x40)
  for i = 0, math.max(racers() - 1, upto or 0) do
    local r = racer(e + 0xA4 * i)
    if r then t[#t + 1] = tag .. ".c" .. i .. "=" .. r end
  end
  return table.concat(t, " ")
end

local function write(fn, text)
  out:write("frame=" .. emu:currentFrame() .. " fn=" .. fn .. " " .. text .. "\n")
  out:flush()
end

-- Entry/exit tracing: at entry, `pre()` captures the inputs; at the return address (LR) `post(pre)` returns the
-- line, or nil to skip it.
local pending, retset = {}, {}
-- One mGBA breakpoint per address, dispatching to every handler there (a return address can coincide with a fixed
-- probe). The address is bound in the closure: at a breakpoint `pc` reads ahead of the instruction.
local handlers = {}
local function on(addr, fn)
  if not handlers[addr] then
    handlers[addr] = {}
    emu:setBreakpoint(function()
      for _, h in ipairs(handlers[addr]) do
        local ok, err = pcall(h)
        if not ok then write("debug", string.format("at=%08x error=", addr) .. tostring(err):gsub("%s", "_")) end
      end
    end, addr)
  end
  table.insert(handlers[addr], fn)
end

local function trace(addr, name, pre, post)
  on(addr, function()
    local ret = emu:readRegister("lr") & ~1
    if not retset[ret] then
      retset[ret] = true
      on(ret, function()
        local stack = pending[ret]
        if not stack or #stack == 0 then return end
        local p = table.remove(stack)
        local line = p.post(p.pre)
        if line then write(p.name, line) end
      end)
    end
    pending[ret] = pending[ret] or {}
    table.insert(pending[ret], {name = name, pre = pre(), post = post})
  end)
end

local function sampled(changed) return changed or (every > 0 and emu:currentFrame() % every == 0) end

-- lap_crossing(world, entity): the whole race before and after.
local open = nil -- the tracker/AI call around a lap_crossing, which records its mid state
trace(0x0813F098, "lap_crossing", function()
  local e = emu:readRegister("r1")
  if open then open.mid = racer(e) end
  return {who = r16(e), text = race("pre", r16(e))}
end, function(p)
  return "who=" .. p.who .. " " .. p.text .. " " .. race("post", p.who)
end)

-- The player's racing-line tracker FUN_0813EDD8(world, entity); exit at its epilogue 0x0813EFF6.
on(0x0813EDD8, function()
  local e = emu:readRegister("r1")
  local d = r32(e + 0x8C)
  local v = {}
  for i = 0, 5 do v[#v + 1] = i32(d + 0x11C + 4 * i + (i >= 3 and 0x18 or 0)) end
  open = {kind = "track", e = e, pre = racer(e), g = globals(), vec = table.concat(v, ",")}
end)
on(0x0813EFF6, function()
  local o = open
  if not o or o.kind ~= "track" then return end
  open = nil
  local post = racer(o.e)
  if sampled(post ~= o.pre or o.mid) then
    write("track_player", "g=" .. o.g .. " vec=" .. o.vec .. " pre=" .. o.pre .. (o.mid and (" mid=" .. o.mid) or "") ..
      " post=" .. post .. " ww_flag=" .. r32(0x03005384))
  end
end)

-- The AI's racing-line advance inside FUN_0814D078: from 0x0814D21C (r6 = segment, r5 = next point,
-- [sp+0x3C] = entity) to the join at 0x0814D3AA.
on(0x0814D21C, function()
  local e = r32(emu:readRegister("sp") + 0x3C)
  local n = emu:readRegister("r5")
  local ok = {}
  for i = 0, 9 do ok[#ok + 1] = r32(0x030060C0 + 4 * i) end
  open = {kind = "ai", e = e, pre = racer(e), g = globals(), seg = s(emu:readRegister("r6") & 0xFFFF, 16),
    next = r16(n + 0xC) .. "," .. r16(n + 0xE), ok = table.concat(ok, ",")}
end)
on(0x0814D3AA, function()
  local o = open
  if not o or o.kind ~= "ai" then return end
  open = nil
  local post = racer(o.e)
  if sampled(post ~= o.pre or o.mid) then
    write("ai_advance", "g=" .. o.g .. " seg=" .. o.seg .. " next=" .. o.next .. " ok=" .. o.ok .. " pre=" .. o.pre ..
      (o.mid and (" mid=" .. o.mid) or "") .. " post=" .. post .. " rand=" .. r32(0x030064C8))
  end
end)

-- The plane-table build at race load (FUN_08138F30(world)): the world's racing line going in, the tables out.
trace(0x08138F30, "build_planes", function()
  return "lapped=" .. r32(0x0300608C) .. " g=" .. globals() .. " sections=" .. hex(r32(WORLD + 0x40), 0x50) ..
    " points=" .. hex(r32(WORLD + 0x44), 0x1800)
end, function(pre)
  return pre .. " planes=" .. hex(r32(0x03005FB4), 0x2000) .. " back=" .. hex(r32(0x03005FB8), 0x400)
end)

-- race_progress(world, entity) -> r0.
trace(0x081400EC, "race_progress", function()
  return {c = racer(emu:readRegister("r1")), g = globals()}
end, function(p)
  if not sampled(false) then return nil end
  return "g=" .. p.g .. " c=" .. p.c .. " ret=" .. s(emu:readRegister("r0"), 32)
end)

-- Positions FUN_0813EA04(world); logged when a place changes.
local function places()
  local t, e = {}, entities()
  for i = 0, racers() - 1 do
    local d = r32(e + 0xA4 * i + 0x8C)
    if d >= 0x02000000 and d < 0x02040000 then t[#t + 1] = i32(d + 0xA8) end
  end
  return table.concat(t, ",")
end
trace(0x0813EA04, "update_places", function() return {text = race("pre"), places = places()} end, function(p)
  if not sampled(places() ~= p.places) then return nil end
  return p.text .. " " .. race("post")
end)

-- Hunter life: tick (world, entity), hit (attacker, victim, impulse), the two drains (entity, amount).
local function tuning()
  local t = {}
  for _, a in ipairs({0x030061B0, 0x030061B4, 0x030061B8, 0x030061BC, 0x030061C0, 0x030061D0, 0x030061E0,
    0x030061F4, 0x030061A8, 0x0300617C, 0x03006184, 0x030061A0}) do t[#t + 1] = i32(a) end
  return table.concat(t, ",")
end
trace(0x08140F78, "hunter_life_tick", function()
  local e = emu:readRegister("r1")
  return {e = e, c = racer(e), g = globals(), t = tuning()}
end, function(p)
  local post = racer(p.e)
  if not sampled(post ~= p.c) then return nil end
  return "g=" .. p.g .. " tune=" .. p.t .. " pre=" .. p.c .. " post=" .. post
end)
trace(0x0814101C, "hunter_hit", function()
  local a, v = emu:readRegister("r0"), emu:readRegister("r1")
  return {a = a, v = v, ca = racer(a), cv = racer(v), imp = s(emu:readRegister("r2"), 32), g = globals(), t = tuning()}
end, function(p)
  return "g=" .. p.g .. " tune=" .. p.t .. " impulse=" .. p.imp .. " pre.a=" .. p.ca .. " pre.v=" .. p.cv ..
    " post.a=" .. racer(p.a) .. " post.v=" .. racer(p.v)
end)
for _, f in ipairs({{0x0814136C, "hunter_drain_a"}, {0x081413B0, "hunter_drain_b"}}) do
  trace(f[1], f[2], function()
    local e = emu:readRegister("r0")
    return {e = e, c = racer(e), amt = s(emu:readRegister("r1"), 32), g = globals(), t = tuning()}
  end, function(p)
    return "g=" .. p.g .. " tune=" .. p.t .. " amount=" .. p.amt .. " pre=" .. p.c .. " post=" .. racer(p.e)
  end)
end

-- finish_time_estimate(world, entity, elapsed) -> r0.
trace(0x0814F050, "finish_estimate", function()
  local e = emu:readRegister("r1")
  return {e = e, c = racer(e), g = globals(), el = emu:readRegister("r2")}
end, function(p)
  return "g=" .. p.g .. " elapsed=" .. p.el .. " pre=" .. p.c .. " post=" .. racer(p.e) .. " post.g=" .. globals() ..
    " ret=" .. s(emu:readRegister("r0"), 32)
end)

-- Career: payout, style rating, unlock rebuild, save encode. The profile is *0x030056EC.
local function profile() return r32(0x030056EC) end
trace(0x0812EFE8, "career_race_payout", function()
  local p = profile()
  return {cash = r32(p + 0xC), ev = hex(p + 0x205, 18), zone = r8(p + 0x1FB), slot = r8(p + 0x1FC),
    res = hex(0x03005730, 0x40), flag = r32(0x030000A0), mode = r32(0x030056E0), opp = r32(0x03005784),
    car = hex(r32(0x0300539C), 17 * 15), pcar = i8(p + 0x10)}
end, function(p)
  local q = profile()
  return "career=" .. p.flag .. " mode=" .. p.mode .. " opponents=" .. p.opp .. " zone=" .. p.zone .. " slot=" ..
    p.slot .. " car=" .. p.pcar .. " records=" .. p.car .. " pre.cash=" .. p.cash .. " pre.events=" .. p.ev ..
    " pre.ranked=" .. p.res .. " order=" .. hex(0x03005730, 8) .. " post.ranked=" .. hex(0x03005730, 0x40) ..
    " post.cash=" .. r32(q + 0xC) .. " post.events=" .. hex(q + 0x205, 18) .. " paid=" .. i32(q + 0x3B8)
end)
trace(0x0812C30C, "style_rating", function()
  local p = profile()
  local car = i8(p + 0x10)
  return {car = car, rec = hex(r32(0x0300539C) + 17 * car, 17)}
end, function(p)
  return "car=" .. p.car .. " record=" .. p.rec .. " ret=" .. s(emu:readRegister("r0"), 32)
end)
trace(0x08135958, "rebuild_unlocks", function()
  local p = profile()
  local f = {}
  for _, o in ipairs({0x47C, 0x480, 0x484, 0x48C, 0x488, 0x478}) do f[#f + 1] = r32(p + o) end
  return {ev = hex(p + 0x205, 18), f1f8 = r8(p + 0x1F8), flags = table.concat(f, ",")}
end, function(p)
  return "events=" .. p.ev .. " f1f8=" .. p.f1f8 .. " flags=" .. p.flags .. " unlocks=" .. hex(profile() + 0x42D, 40)
end)
trace(0x081492C0, "save_encode", function()
  local b = emu:readRegister("r0")
  local p = profile()
  local g = {}
  for _, a in ipairs({0x030053E4, 0x03000040, 0x03005698, 0x03005798, 0x0300578C, 0x030053A4, 0x03005600,
    0x03000050, 0x03000070}) do g[#g + 1] = r32(a) end
  return {b = b, heap = hex(b, 0x200), profile = hex(p, 0x490), g = table.concat(g, ","),
    cars = hex(r32(0x0300539C), 17 * 15)}
end, function(p)
  return "globals=" .. p.g .. " heap=" .. p.heap .. " profile=" .. p.profile .. " cars=" .. p.cars ..
    " out=" .. hex(p.b, 0x200)
end)

-- Autopilot: aim at the first racing-line point more than LOOK units away (following the player's section and
-- its end link), lift off in sharp turns, brake in very sharp ones, and back out when stuck.
local auto, back = false, 0
local LOOK = 2500
local KEY = {A = 1, B = 2, RIGHT = 16, LEFT = 32}
local stuck, last = 0, nil
local function point(sec, idx)
  local h = r32(WORLD + 0x40) + 8 * sec
  local w = r32(WORLD + 0x44) + 0x18 * (r32(h + 4) + idx)
  return i32(w), i32(w + 4), w
end
local function target(e, x, z)
  local sec, idx = r16(e + 0x72), i16(e + 0x90)
  local tx, tz
  for _ = 1, 8 do
    idx = idx + 1
    local count = r16(r32(WORLD + 0x40) + 8 * sec)
    if idx > count - 1 then
      if sec == 0 then
        idx = idx - (count - 1)
      else
        local _, _, w = point(sec, count - 1)
        if r16(w + 0xE) == 0xFFFF then idx = count - 1 else idx, sec = r16(w + 0xE) + idx - count + 1, r16(w + 0xC) end
      end
    end
    tx, tz = point(sec, idx)
    if (tx - x) ^ 2 + (tz - z) ^ 2 > LOOK ^ 2 then break end
  end
  return tx, tz
end
local function steer()
  local e = entities() + 0xA4 * r32(0x03000060)
  local x, z = i32(e + 0x0C) // 256, i32(e + 0x14) // 256
  if last and (x - last[1]) ^ 2 + (z - last[2]) ^ 2 < 4 then stuck = stuck + 1 else stuck = 0 end
  last = {x, z}
  if stuck > 60 then back, stuck = 45, 0 end
  local tx, tz = target(e, x, z)
  local want = math.floor(math.atan(tx - x, tz - z) / (2 * math.pi) * 16384) & 0x3FFF
  local diff = ((want - ((r32(e + 0x2C) >> 8) & 0x3FFF) + 8192) & 0x3FFF) - 8192
  if back > 0 then
    back = back - 1
    return KEY.B | (diff > 0 and KEY.LEFT or KEY.RIGHT)
  end
  local keys = math.abs(diff) > 4000 and KEY.B or math.abs(diff) > 2000 and 0 or KEY.A
  if diff > 150 then keys = keys | KEY.RIGHT elseif diff < -150 then keys = keys | KEY.LEFT end
  if emu:currentFrame() % 30 == 0 then
    write("auto", string.format("x=%d z=%d tx=%d tz=%d head=%d want=%d diff=%d keys=%d seg=%d",
      x, z, tx, tz, (r32(e + 0x2C) >> 8) & 0x3FFF, want, diff, keys, i16(e + 0x90)))
  end
  return keys
end

callbacks:add("frame", function()
  if auto and r32(0x03005808) == 5 then emu:setKeys(steer()) end
  if emu:currentFrame() % 8 ~= 0 then return end
  local f = io.open(dir .. "/rr_cmd.txt", "r")
  if not f then return end
  local text = f:read("a")
  f:close()
  os.remove(dir .. "/rr_cmd.txt")
  for line in text:gmatch("[^\r\n]+") do
    local op, arg = line:match("^(%S+)%s*(.*)$")
    if op == "auto" and arg == "on" then auto = true
    elseif op == "auto" and arg == "off" then auto = false; emu:setKeys(0)
    elseif op == "auto" then back = tonumber(arg:match("back (%d+)")) or 0; auto = true
    elseif op == "every" then every = tonumber(arg)
    elseif op == "log" then out:close(); out = assert(io.open(dir .. "/" .. arg .. ".log", "a"))
    elseif op == "mark" then write("mark", "text=" .. arg:gsub("%s", "_"))
    elseif op == "planes" then
      write("planes_now", "g=" .. globals() .. " sections=" .. hex(r32(WORLD + 0x40), 0x50) .. " points=" ..
        hex(r32(WORLD + 0x44), 0x1800) .. " planes=" .. hex(r32(0x03005FB4), 0x2000) .. " back=" ..
        hex(r32(0x03005FB8), 0x400))
    end
  end
end)

write("mark", "text=tracer_loaded")

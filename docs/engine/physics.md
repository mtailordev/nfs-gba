# Car physics (player car per-frame step)

The per-frame update of a racing car, from the entity handler down to the rigid body, ported exactly to
`crates/nfsgba-sim` and checked against traces of the reference build (mGBA). ROM `BN7E` v0, SHA-1 `e5298b24…`.

**Status:** the step is **exact and verified** for the player car driving on its own: accelerating, braking,
reversing, steering, handbrake, automatic gear changes, wall contacts, sector changes through portals, route
tracking over the start/finish line, and the sound commands the step issues. 965 traced steps over 8 scenarios
match byte for byte (entity, the whole 0x4FC-byte physics struct, every RAM byte the step writes, every sound
call). What is not ported yet is listed under [Not ported](#not-ported); those paths stop the port with an
`Unported` error instead of guessing.

## Frame structure

- The game runs its simulation once per rendered frame, about every 4 video frames in a race (15 fps).
- **Frame time:** `FUN_0812ae64` reads timer 3 (1,024-cycle prescaler) each frame. It stores `25,500 / ticks`,
  clamped to 10..100, at `0x03005640` (typically 22..24). The value is 15 when `0x03005624` is 2. The car step
  turns this into `dt = FUN_08149178(frame_time << 8)`, capped at 0xC00 (4.12 fixed point).
- **Entity update:** `FUN_0813765c(world, first)` walks the entity array (world `+0x3C`, 0xA4 bytes each) up to
  world `+0xF8 + +0xFA`. For each entity with flags `+0x08 & 3 == 3` it calls the handler table `0x087F38B8`
  at index `+0x4E`, with (world, entity). If the entity's sector is 0xFFFF it prints a debug message instead.
  Entries 0..3 are the car handler `FUN_0814bd4c`. `sort_sector_entities` (IWRAM) can also call handlers while
  drawing. A per-frame bitmap keeps each entity to one update.
- **Controls:** `FUN_0815eb68` reads `KEYINPUT` (held keys at `0x030064C4`, newly pressed at `0x030064C0`). The
  player's control word is `0x030057D8 + 2·index` (u16, `0xFC00 | keys`; AI cars write theirs). Actions go through
  the binding table `0x087F5494`: 4 sets × 9 actions × (held mask, held value, pressed mask, pressed value). The
  set index (byte `0x0300629C`) is 0 with the automatic gearbox, else 1 (`FUN_08144f1c`). Actions: 0 accelerate
  (A), 1 brake (B), 2 left, 3 right, 4/5 gear down/up (sets 1–3), 6 handbrake (R in set 0), 7 nitro (A+L), 8 mode
  power (R+L). Steering reads the raw LEFT (0x20) / RIGHT (0x10) bits. The manual gearbox reads the raw R
  (0x100) / L (0x200) bits.

## Call tree (racing car)

```
FUN_0814bd4c  car handler                       car::handler
  FUN_081375ac / FUN_08137578  unlink/link the entity in its sector's list (world +0x0C, links at +0x02)
  state +0x4A: 0 → FUN_0814b98c init (not ported); 2 or 0x100 → FUN_0814b168; 2 also arms the race start
  FUN_0814b168  racing step                      car::racing_step
    FUN_0813bd90  player's wheel sprite, when seen from the side (rendering, left to the renderer)
    FUN_0814b098  nitro tank drain               car::nitro
    FUN_0813d1f0  dynamics                       car::dynamics
    visibility flags (+0x08 bit 2, +0x0A bit 0)
    FUN_0814de40  suspension step, only while 0x0300610C is 0 (the dynamics set it to 1)
```

`FUN_0813d1f0` in order (`car::dynamics`):
1. Off-route warning `0x0300601C` (±1 when far from the racing line at speed); previous and pressed keys
   (`+0x4AE`); action 8 (`FUN_0814078c`); stuck check (`FUN_0814efa8`, not ported).
2. Route segment by sector (`FUN_0813f234`); traffic countdown for the player (`FUN_08143b2c`); gearbox timer
   `+0x9C` −1; binding set; `dt`; gear ratio of the current gear; torque-boost timer `+0x42C/+0x428`.
3. Steering `+0x20` (±0xA000 per step up to ±0x80000, reset when released or reversed).
4. Speeds: `+0xA0 = 32·dot(velocity, forward)`, `+0x44 = |+0xA0|`. Race stats: distance `+0x2D0`, top speed `+0x2DC`.
5. Throttle `+0x24` (+0x2000 per step while accelerating, 0 otherwise), brake `+0x28`. Automatic gearbox: brake
   below speed 0x3000 selects reverse (gear 0); in reverse, accelerate below 0x3000 selects first (gear 2) and
   brake is the throttle.
6. Nitro on/off `+0x4D1` (action 7, tank `+0x4C8` ≥ 2, gear ≥ 2).
7. Slip angle = `atan2(velocity) − atan2(forward)`. With the same sign as the yaw rate (`+0x15C`), it damps the
   angular momentum y by curve `0x087F4164` × handling `+0x148`. Rear grip `+0x334/+0x3C8` = base `+0x338` ×
   (1 + (curve `0x087F41A0`(0x733·|slip| + 0x8CD·|yaw|) − 0x1000)/2).
8. Speed `= |velocity|` (`FUN_08147a6c`); stationary counter `+0x4EE`; neutral rev handling.
9. Engine: rpm `+0x3C` follows the wheels (`0x109A · (Σ wheel spin >> 8) × ratio >> 30 / 6`), or the throttle in
   neutral. The step is limited to ±1,000 per frame, and the excess becomes engine braking. rpm is clamped to
   max (`+0x454`) and idle (handling `+0x68`). Torque = curve `+0x464` (10 points over max rpm / 8) ×
   `+0x460` (halved at the limiter; × `+0x4CE` with nitro). Drive = torque × throttle × ratio × `+0x4BC` ×
   `+0x428` (64-bit products, each >> 15), minus/plus rpm × `+0x458` (engine braking).
10. Handbrake (action 6) halves the rear grip and brakes the rear wheels 4×. Drive and brake go to the four wheel
    spins `+0x64` (drive share `+0x70`, brake `+0x74`).
11. Sector of the entity position (`FUN_0814fa04`), keeping the old one when none is found. Sets `0x0300610C = 1`.
    Gravity: momentum y += dt × (g `0x03006030` × mass) >> 11.
12. Ground contact `FUN_08147ec4` (or `FUN_081484f0` when tipped over, `+0x138 < 0xF21`); car-to-car test
    `FUN_08145320`; parked stop (slow, 4 wheels down, no pedal: momentum, velocity, angular momentum and ω zeroed).
13. Contact flags `+0x448 &= 8`; walls `FUN_08145ca8`; the body is integrated twice with `dt` (`FUN_08147b18`).
14. Entity position = body position − R·(0, `+0x43C`, `+0x440`); sector again (0xFFFF → push-back loop, not ported).
15. "Drag": the game scales a stack vector by `(speed/8)² >> 16 × handling +0x11C >> 6` and subtracts it from the
    velocity. That slot held the normalised velocity, but by now it holds the rotated centre-of-mass offset, so
    the offset is what gets subtracted. (Handling `+0x11C` is 0 for every car, so this does nothing in practice.)
    Then momentum = mass × velocity; odometer `+0x90` += `+0xA0`.
16. Heading `+0x00` = `atan2(forward x, z)`, entity `+0x2C` = heading << 8. Engine pitch, skid counter and skid
    sound (mean wheel slip > 0x3D090).
17. Gears: automatic `FUN_0813c02c` (upshift at `+0x44C` rpm, downshift when torque × ratio is better one gear
    down, then 5 steps' pause), or the manual state machine on R/L (`0x03006074`). Route: waypoint `FUN_0813edd8`,
    progress `+0xAC` `FUN_0814032c`, gap to the next car `FUN_0813ebac` (`0x0300615C`), nearest lane `+0xC0`,
    mode-2 damage `FUN_08140f78` (not ported).

### Rigid body (`FUN_08147b18`, physics `+0xC8`)
Explicit Euler, run twice per frame: position += dt·v; q += dt·(q ⊗ (ω/2, 0)), renormalised through the
reciprocal table (`2^24/(k+1)` at `0x087C45F0`); R = matrix(q); v = inverse mass × momentum; ω = inverse
inertia (a scalar) × angular momentum. Forces are applied as impulses to momentum and angular momentum.

### Wheels on the ground (`FUN_08147ec4`)
For each wheel: position in world axes = R·local. The sector under the wheel comes from the IWRAM point-in-sector
test (`FUN_03000800`); the floor height from `FUN_0814ca84`. Penetration = body y + wheel y + ride height
(`+0x204`) − floor. When it is positive:
- load = spring × penetration >> 8 + (n·v_contact) × damping >> 7 (damping −10 while not moving down), at least 0;
- tyre velocity = contact velocity + wheel spin along the rolling direction (front wheels turned by `+0x20 >> 8`,
  >> 9 in reverse);
- friction = −v/16, or −v̂ × grip/16 when |v|² > grip², where grip = wheel grip × load × surface factor
  (`0x087F5904`, floor sector `+0x08`), capped at 0x4000;
- the wheel spin changes with the friction along the rolling direction; the impulses dt·(friction, −load) go to
  momentum and, crossed with the contact point, to angular momentum.

A body more than 8 units below the floor and still falling loses its vertical velocity first.

### Walls (`FUN_08145ca8` → `FUN_081459b8` → `FUN_081457b8`)
The car tests the point 3/256 of its forward axis ahead of it against the walls of its sector. It recurses once
through open portals, into the linked sector's floor sector. A wall hits when the point is within 100 units of the
wall's plane (the normal at wall `+0x34/+0x36`), within its span, and no more than √0x270F (about 99.99) units
past either end. For a hit, the approaching normal velocity at the contact point `vn` (spin included) gives:
- restitution `k = clamp(−(vn·0x1A2 + 0x19D10000) >> 24, −25, −16)`;
- impulse `j = k·vn >> 10`, along the normal.

A dynamic wall whose state `+0x16` is 1 breaks when `j` > 0x2000 (`FUN_0813b5a0`), and its partner from
`0x087F3CDA` breaks with it. Other effects: flags `+0x448` (0x10 contact, 2/4 side hits), `+0x4B4` bit 0 (j > 0x4000),
nearest lane `+0xC0`, mode-2 damage (`FUN_0814136c`). The player's first contact starts the scrape or crash sound.

### Sectors and floors
- `FUN_0814fa04` stores the query point in world `+0xC0/+0xC4/+0xC8` and the start sector in `+0xEA`, then
  calls `FUN_03000800`. That function tests the start sector and then every sector behind its open portals: a
  wall is open when its flags lack 0x1000 and its link `+0x32` is not 0xFFFF. The test is `FUN_030047a8`, a
  convex-polygon test with wrapping 32-bit cross products. A sector without a floor defers to `+0x20`.
  `FUN_0814dbbc` (two portals away) is the fallback.
- Floor height `FUN_0814ca84` = first wall `+0x38` + sector `+0x26` (+ dynamic wall `+6`, + plane `+6`), then
  the floor plane through the first wall's corner, `−(a·dx + c·dz) × recip(b) >> 16`. The plane (a, b, c) is sector
  `+0x14/+0x16/+0x18`, or a sloped plane at world `+0x1C` when sector `+0x0A` is not 0xFFFF.

## Entity fields (0xA4 bytes)

| Offset | Type | Meaning |
|---|---|---|
| `+0x00` | u16 | entity index |
| `+0x02` | u16 | next entity in the sector's list (world `+0x0C` heads) |
| `+0x08` | u16 | flags: bit 0 active; bit 1 set = updated by `update_entities`, clear = while drawing; bit 2 collidable/visible |
| `+0x0A` | u16 | flags: bit 0 = this is entity `0x030057F8` |
| `+0x0C/+0x10/+0x14` | i32 ×3 | position, 8.8 city units (`-y` up) |
| `+0x28` | i32 | view depth, written by the renderer |
| `+0x2C` | i32 | heading << 8 |
| `+0x4A` | u16 | state: 0 init, 2 racing (arms the race start), 0x100 racing, 1/3 idle |
| `+0x4E` | u16 | handler index (table `0x087F38B8`) |
| `+0x72` | u16 | route segment (0 = main route) |
| `+0x78` | u16 | sector |
| `+0x88` | u8 | changed outside the car step (unknown) |
| `+0x89` | u8 | handling record index (car) |
| `+0x8C` | u32 | physics struct (0x4FC bytes, EWRAM) |
| `+0x90` | i16 | route waypoint within the segment |

## Physics struct (0x4FC bytes, entity `+0x8C`)

| Offset | Meaning |
|---|---|
| `+0x00`, `+0x04` | heading (14-bit angle) and its sign word |
| `+0x08..+0x14`, `+0x48`, `+0x4C..+0x78` | suspension state of `FUN_0814de40` (not ported) |
| `+0x20` | steering, ±0x80000 |
| `+0x24` | throttle, 0..0x9999 |
| `+0x28` | brake on |
| `+0x34`, `+0x38` | set at init from handling `+0x00/+0x08/+0x0C/+0x10` |
| `+0x3C` | engine rpm |
| `+0x40` | gear: 0 reverse, 1 neutral, 2..7 first..sixth |
| `+0x44` | speed, \|`+0xA0`\| |
| `+0x90` | odometer |
| `+0x9C` | automatic gearbox pause |
| `+0xA0` | forward speed × 32 |
| `+0xA8`, `+0xAC` | race position, race progress |
| `+0xB4`, `+0xB8` | best lap time, lap start time (race time `0x03005800`) |
| `+0xC0` | u16 nearest lane (`0x087F4120`) |
| `+0xC5` | u8 laps left |
| `+0xC8` | rigid body: `+0xC8` mass, `+0xCC` inverse mass, `+0xD0` position (8.8), `+0xE8` quaternion (x,y,z,w; 1.0 = 0x1000), `+0xF8` momentum, `+0x110` angular momentum, `+0x11C` velocity, `+0x128` rotation matrix (row 2 `+0x140..+0x148` = forward), `+0x158` angular velocity, `+0x164` quaternion rate, `+0x178` inverse inertia |
| `+0x18C + 0x94·i` | wheel i (0,1 front): `+0x0C` contact point, `+0x18` position in world axes, `+0x48` position in car axes, `+0x64` spin, `+0x68` spring, `+0x6C` damping, `+0x70` drive share, `+0x74` brake, `+0x78` ride height (wheel 0's is used for all), `+0x7C` slip², `+0x80` grip, `+0x84` base grip, `+0x88` 0x700, `+0x8C` recip(…), `+0x90` sector under the wheel |
| `+0x3DC` | sum of wheel spins |
| `+0x3E0..` | 10 upgrade levels (init) |
| `+0x408..+0x424` | gear ratios (reverse, neutral, first..sixth) × final drive |
| `+0x428`, `+0x42C` | torque multiplier (0x8000 once the timer runs out), timer |
| `+0x438..+0x440` | centre-of-mass offset (handling `+0x110..+0x118`) |
| `+0x444` | launch/limiter counter |
| `+0x448` | contact flags (0x10 wall, 2/4 side, 1 blocks the parked stop, 8 kept) |
| `+0x44C`, `+0x450`, `+0x454`, `+0x458` | upshift rpm, lower rpm, max rpm, engine braking |
| `+0x460` | torque multiplier 0x1000..0x1800 (neutral revs) |
| `+0x464..` | torque curve (18 words copied from handling `+0x6C`; 10 used) |
| `+0x4AE` | u16 previous control word |
| `+0x4B4` | bit 0: hard wall hit |
| `+0x4BC` | torque scale |
| `+0x4C0` | last touched wall with flag 0x4000 |
| `+0x4C8`, `+0x4CC`, `+0x4CE`, `+0x4D1` | nitro tank, drain factor, torque factor, on |
| `+0x4D8` | u16 route flags: 1 before the start line, 2 after waypoints 1..9, 0x10 no yaw damping |
| `+0x4E4`, `+0x4E6`, `+0x4E8`, `+0x4EC`, `+0x4EE` | tipped counter, airborne counter, damage (mode 2), wrong-way counter, stationary counter |

## Handling records (`0x087F1100`, 0x158 bytes, index entity `+0x89`)

15 records (one per car in the car table). Offsets are bytes; "init" means `FUN_0814b98c` / `FUN_0814b2a8`
(not ported) copies the value into the physics struct.

| Offset | Meaning | Example (record 0) |
|---|---|---|
| `+0x00` | mass | 1252 |
| `+0x08..+0x10` | inertia terms (init `+0x34/+0x38`) | 39616, 39616, 79233 |
| `+0x34..+0x50` | gear ratios: reverse, neutral, first..sixth (× final drive at init) | −570985, 0, 556227 … 131853 |
| `+0x54` | top gear | 7 |
| `+0x58`, `+0x150` | torque scale, base and fully upgraded | 11456, 21043 |
| `+0x64`, `+0x68` | max rpm, idle rpm | 8000, 850 |
| `+0x6C..+0xB0` | torque curve | |
| `+0xC8..+0xE8` | front: spring, damping, ride height, wheel x/y/z, drive share, brake, grip | 64, 40, 8192, (16053, −4096, 22467), 2048, 524288, 3712 |
| `+0xEC..+0x10C` | rear: the same | |
| `+0x110..+0x118` | centre-of-mass offset | (0, −4096, 5632) |
| `+0x11C` | drag (0 in every record) | 0 |
| `+0x148` | yaw damping | 2048 |
| `+0x14C`, `+0x154` | final drive, base and fully upgraded | 281, 190 |

Upgrades: `0x087F5988` holds 10 categories × 5 attribute weights. Init lerps torque scale, final drive, grip and
brakes by the car's upgrade levels (`+0x3E0`).

The **car table** `0x087F0BD8` (0x58 bytes, `docs/formats/vehicle-models.md`) is not read by the car step. Its
`+0x18..+0x57` holds packed 16-bit pairs (`+0x20..+0x34`) and small numbers that look like menu stats
(`+0x48/+0x4C` 12..16; `+0x50` u16 1600 then bytes 20..90; `+0x54` 30..90). This is a hypothesis: the readers
are `garage_draw_car` and `setup_race_cars`, which are not decoded yet.

## Verification

- **Traces:** `tools/mgba_remote.lua` `trace NAME` breaks on `FUN_0814bd4c` when r1 is the player's entity.
  Each hit logs the frame, keys, the globals the step reads (frame time, race phase, control word, `0x0300610C`,
  `0x03005614`), the entity and the physics struct. The first hit also dumps memory. `tools/trace_race.py`
  records the scenarios from `race.ss` (the reference race, 23 s in, opponents out of reach): accel, brake,
  steer, wall, drive (R, A+L, R+L, sector 760→759), reverse (sectors 760→751→748), handbrake and long (760→759→760,
  waypoints 33→34→0→1 over the start line). Reruns give identical files.
- **Oracle:** `tools/trace_oracle.py` runs `FUN_0814bd4c` from the ROM in unicorn on each traced entry state. It
  reproduces all 965 steps and writes what each step changed in RAM plus the sound calls it made
  (`<name>.oracle.txt`).
- **Rust** (`crates/nfsgba-sim/tests/trace.rs`):
  - `each_step_matches_the_trace`: from every traced entry state, the step gives the next traced state, the same
    RAM writes and the same sound commands as the game code;
  - `replay_matches_the_trace`: feeding only the recorded keys and frame times from the first state reproduces
    every traced state.

  The comparisons skip three entity fields that other code maintains between steps: `+0x02` (sector list link),
  `+0x28` (renderer depth) and `+0x88`. Changing a single constant (the damping −10) breaks both tests.
- **Replay limits:** the opponents' and the race's global state do not advance in the Rust replay; they are frozen
  at the first step's dump. The traced scenarios never reach an opponent, so this does not matter here.

## Not ported

Each of these stops the port with `Unported` (NOT 1:1 until ported and traced):
- `FUN_0814b98c` / `FUN_0814b2a8` / `FUN_08147ca0` / `FUN_0813e430` / `FUN_0813f744`: car init (state 0).
- `FUN_0814de40`: suspension step while `0x0300610C` is 0 (countdown/finish phases 1 and 4).
- `FUN_081484f0`: tipped over or airborne dynamics.
- `FUN_08144fa4` / `FUN_0814101c`: car-to-car collision response. The proximity test is ported.
- `FUN_0814dbbc`: sector search two portals away; the push-back loop when the car leaves every sector.
- `FUN_0814efa8`: putting a stuck car back on the road.
- `FUN_08143d48`: traffic spawn; `FUN_0814078c`: mode power (modes 1..12); `FUN_08140f78`: mode-2 damage.
- `FUN_0813f098` beyond the lap bookkeeping (`FUN_0813f008`, race order, finish).
- `FUN_0813bd90`: the wheel sprite. It is rendering, and the renderer should own it.

Ported but not exercised by the traces (so not verified): the manual gearbox, active nitro and its drain
(`FUN_0814b098`, the IWRAM divider at 0x03000220 with its sign slip), breakable walls, side-hit flags,
mode-2 wall damage, the neutral-gear rev logic, and side route segments. The IWRAM divider's unit test
records the slip.

## Integration notes

**address-map.md** rows:

| Address | What |
|---|---|
| `0x03005640` | frame time (25,500 / timer-3 ticks, 10..100; 15 if `0x03005624` is 2), written by `FUN_0812ae64` |
| `0x030057D8` | u16 control word per entity (`0xFC00 \| keys` for the player) |
| `0x0300629C` | u8 control binding set (0 automatic, 1 manual) |
| `0x03005798` | automatic gearbox flag |
| `0x03000060` | local player entity index |
| `0x030056EC` | race state pointer (EWRAM): `+0x2D0` distance, `+0x2D8` skid count, `+0x2DC` top speed, `+0x2E0` gear-change flag, `+0x2E8` accelerator flag, `+0x2EC` wall contact, `+0x2EF` engine sound effect, `+0x318` skid sound flags |
| `0x0300610C` | set by the dynamics; while 0 the racing step runs the suspension step `FUN_0814de40` |
| `0x0300601C` | off-route warning (−1/0/1) |
| `0x03005384` | wrong way (more than 27 steps against the route) |
| `0x0300615C` | time gap to the car ahead/behind |
| `0x03006030` | gravity (0x4F0) |
| `0x03006074` | manual gearbox state |
| `0x03005FB4`, `0x03005FB8`, `0x03006120`, `0x030060C0` | route waypoint lines (0x20 each), segment joins, segment lengths, segments visited |
| `0x0300608C` | circuit flag |
| `0x030057EC` | number of racers checked for car-to-car contact |
| `0x03005780`, `0x03005630` | race start armed, start countdown |
| world `+0xC0/+0xC4/+0xC8/+0xEA` | sector query point and start sector |
| world `+0x18`, `+0x1C` | dynamic wall states (0x20), sloped floor planes (0x14) |
| world `+0xF8`, `+0xFA` | entity counts for the update loop |
| `0x087F1100` | handling records (15 × 0x158) |
| `0x087F38B8` | entity handler table |
| `0x087F5494` | control bindings (4 × 9 × 8 bytes) |
| `0x087F5904` | grip per floor surface (8 words) |
| `0x087F4164`, `0x087F41A0` | curves: yaw damping by slip angle, rear grip by slip and yaw |
| `0x087F37D8` | per route: side-segment sector lists |
| `0x087F3CDA` | breakable-wall partners |
| `0x087F4120` | lane offsets (4) |
| `0x087F55FC` | car-to-car axle offsets (2) |
| `0x087F5988` | upgrade weights (10 × 5 words) |
| `0x087C05F0`, `0x087C45F0` | sine table (0x2000 × i16), reciprocal table (2^24/(k+1)) |
| `0x03000220` (IWRAM, ARM) | signed divide with remainder store (sign slip for negative divisors) |

**symbols.csv** rows (address, name, kind, comment):

```
0x0813765c,update_entities,function,calls the handler table 0x087F38B8 for each active entity
0x0814bd4c,car_handler,function,entity handler 0..3: unlink/link in the sector list; init or racing step
0x0814b98c,car_init,function,allocates the 0x4FC physics struct; handling record 0x087F1100
0x0814b2a8,car_setup_handling,function,copies handling + upgrades into the physics struct
0x0814b168,car_racing_step,function,nitro drain + dynamics + visibility; suspension step when 0x0300610C is 0
0x0814b098,car_nitro_drain,function,nitro tank +0x4C8
0x0813d1f0,car_dynamics,function,one car step: controls engine gearbox tyres walls integration route (docs/engine/physics.md)
0x0813c02c,car_auto_shift,function,automatic gearbox
0x08147ec4,car_wheel_contact,function,four wheels on the ground: suspension load and tyre friction impulses
0x081484f0,car_airborne,function,dynamics when tipped over (+0x138 < 0xF21)
0x0814de40,car_suspension,function,suspension step (phases 1 and 4)
0x08145320,car_car_proximity,function,swept proximity test against the other racers
0x08144fa4,car_car_response,function,car-to-car collision response
0x08145ca8,car_walls,function,wall test at the point ahead of the car; contact sounds
0x081459b8,car_wall_search,function,walls of a sector (recursing through open portals)
0x081457b8,car_wall_response,function,wall impulse with clamped restitution
0x0813b5a0,break_wall,function,breakable dynamic wall and its partner lose the solid flag
0x08147b18,body_integrate,function,rigid body Euler step (quaternion)
0x08148fe8,body_update_velocities,function,v = m^-1 p and w = I^-1 L
0x0814fa04,find_sector,function,sector containing a point (query in world +0xC0..+0xEA)
0x03000800,find_sector_near,function,IWRAM: query sector or one through its open portals
0x030047a8,point_in_sector,function,IWRAM: convex wall-loop test
0x0814dbbc,find_sector_far,function,sector search two portals away
0x0814ca84,floor_height,function,floor height of a sector at x z (8.8)
0x0814f4a8,floor_sector,function,sector record or the one it defers its floor to (+0x20)
0x08144f38,control_action,function,control binding test (table 0x087F5494)
0x08144f1c,select_bindings,function,binding set from the automatic flag
0x0813e860,route_normalize,function,normalise a waypoint index across segments
0x0814007c,route_waypoint,function,waypoint address of segment + index
0x0814009c,route_waypoint_at,function,normalised waypoint address
0x0813e93c,route_along_line,function,how far along its waypoint line a car is
0x0814032c,route_progress,function,race progress distance
0x0813f234,route_track_segment,function,side-segment switching by sector
0x0813edd8,route_track_waypoint,function,waypoint advance and wrong-way counter
0x0813f098,route_lap,function,lap completion
0x0813ebac,route_gap,function,time gap to the neighbouring car
0x081402bc,route_lateral,function,signed distance across the route
0x08140274,nearest_lane,function,nearest of the 4 lane offsets
0x0815f9cc,atan2,function,angle of (x z); 0x4000 per turn
0x0815f948,sin14,function,sine of a 14-bit angle (table 0x087C05F0)
0x0815f988,cos14,function,cosine
0x0815fa54,isqrt,function,integer square root (at least 1)
0x08149178,recip,function,about 2^24/x via the reciprocal table
0x08147a6c,normalize,function,normalise to 0x1000; returns the length
0x0815fdd0,normalize14,function,normalise to 0x4000
0x08147618,quat_mul,function,quaternion product
0x081476d8,quat_matrix,function,rotation matrix of a quaternion
0x0816a8b4,__muldi3,function,64-bit multiply
0x0816a93c,__udivsi3,function,unsigned divide
0x0816a9b4,__umodsi3,function,unsigned remainder
0x08135fdc,sound_play,function,start sound effect (table 0x087EE24C)
0x08136028,sound_stop,function,stop sound effect
0x081360b4,sound_pitch,function,set sound effect pitch
0x0812ae64,race_frame_timing,function,frame time 0x03005640 from timer 3
```

**FIDELITY.md:** D4 is partly closed. The player car's per-frame step is exact and verified (see Verification
above). New open entries:
- the car init and handling setup;
- the suspension step (countdown);
- airborne/tipped dynamics;
- the car-to-car response;
- the far sector search and push-back;
- stuck reset, traffic, mode power/damage, lap completion;
- the wheel sprite, which belongs to the renderer.

Unverified branches are listed under Not ported.

# Car physics (the car entity's per-frame update)

The per-frame update of a racing car, from the entity handler down to the rigid body, ported exactly to
`crates/nfsgba-sim` and checked against traces of the reference build (mGBA). ROM `BN7E` v0, SHA-1 `e5298b24…`.

**Status: exact and verified** for the player's car:
- the car's init step: heap allocation, handling and upgrades, rigid-body placement, 20 settling steps, route tables,
  decal unpack;
- the race intro and the launch among the opponents;
- accelerating, braking, reversing, steering and the handbrake;
- automatic gear changes;
- wall contacts;
- sector changes through portals;
- route tracking over the start/finish line;
- a traffic spawn;
- the sound commands the step issues.

1,149 traced steps over 9 scenarios match byte for byte, checked two ways:
- **Per step:** each step starts from the reference build's full RAM. It must give the next traced entity and
  physics struct, write the same RAM bytes the game's own code writes, and issue the same sound calls.
- **Replay:** the player's car runs on its own state across the whole trace.

Every path of the car step is ported (the physics-paths work closed D9–D13; [Paths no race reaches](#paths-no-race-reaches)). The only piece left out by design is the rim redraw onto the atlas, which is rendering.

## Frame structure

- The game runs its simulation once per rendered frame, about every 4 video frames in a race (15 fps).
- **Frame time:** `main_frame` (`FUN_0812ae64`) reads timer 3 (1,024-cycle prescaler) each frame. It stores
  `25,500 / ticks`, clamped to 10..100, at `0x03005640` (typically 20..24). The value is 15 when `0x03005624`
  is 2. The car step turns this into `dt = FUN_08149178(frame_time << 8)`, capped at 0xC00 (4.12 fixed point).
- **Entity update:** `update_entities` (`FUN_0813765c`) walks the entity array (world `+0x3C`, 0xA4 bytes each)
  from world `+0xF8`, for world `+0xFA` entities. For each entity with flags `+0x08 & 3 == 3` it calls the handler
  table `0x087F38B8` at index `+0x4E`. If the entity's sector is 0xFFFF it prints a debug message instead.
  - Entries 0..3 are the car handler `FUN_0814bd4c`; 0x36 is traffic.
  - `sort_sector_entities` (IWRAM) calls handlers for entities whose flag bit 1 is clear, while drawing.
- **Controls:** `FUN_0815eb68` reads `KEYINPUT` (held keys at `0x030064C4`, newly pressed at `0x030064C0`). The
  player's control word is `0x030057D8 + 2·index` (u16, `0xFC00 | keys`; AI cars write theirs).
  - Actions go through the binding table `0x087F5494`: 4 sets × 9 actions × (held mask, held value, pressed mask,
    pressed value). The set (byte `0x0300629C`) is 0 with the automatic gearbox (`0x03005798`), else 1
    (`FUN_08144f1c`).
  - The actions: 0 accelerate (A), 1 brake (B), 2 left, 3 right, 4/5 gear down/up (sets 1–3), 6 handbrake (R in
    set 0), 7 nitro (A+L), 8 wingman command (R+L).
  - Steering reads the raw LEFT (0x20) / RIGHT (0x10) bits; the manual gearbox reads the raw R (0x100) / L (0x200).

## Call tree

```
FUN_0814bd4c  car handler                              car::handler
  FUN_081375ac / FUN_08137578  unlink/link the entity in its sector's list (world +0x0C, links at +0x02)
  state +0x4A: 0 → car_init FUN_0814b98c; 0x100 racing; 2 finished → racing step, then the race-over flag
                0x03005780, the palette fade 0x03005630, and at fade end phase 3 with this car's index
  FUN_0814b168  racing step                             car::racing_step
    FUN_0813bd90  draw_decal_on_atlas (player, rim_side_visible): rendering, left to the renderer
    FUN_0814b098  nitro tank drain                      car::nitro
    FUN_0813d1f0  dynamics                              car::dynamics
    visibility flags (+0x08 bit 2, +0x0A bit 0)
    FUN_0814de40  suspension step, only while 0x0300610C is 0 (the dynamics set it to 1)
```

### Car init (`FUN_0814b98c`, entity state 0; `init.rs`)
1. Allocate the 0x4FC-byte physics struct on the heap (cleared) and set the entity's state to 0x100.
2. Seed the heading and the inertia terms `+0x34/+0x38`; gear 1, laps `+0xC5/+0xC6`, and flags and counters.
3. Pick the lane `+0xC0`; read the upgrade levels `+0x3E0` from the car record (`*0x0300539C`, 0x11 bytes per car,
   bytes 7..16 of 4 × 2 bits); set up the nitro (`FUN_0814f198`).
4. For the player, unpack the decal (`unpack_decal`): LZ77 from the vehicle material, through a 0x1011-byte ring
   on the heap (`lz77_ring_decode`, IWRAM `0x030042F4`), then remap.
5. Put the entity on the floor; gravity `0x03006030 = 0x4F0`.
6. `FUN_0814b2a8` builds the engine and the car from the handling record plus the upgrades (weights
   `0x087F5988`, lerped): max, upshift and shift rpm, engine braking, gear ratios × final drive, torque scale,
   the torque curve and its peak, and the wheels (position, spring, damping, ride height, drive share, brake, grip
   (doubled unless `0x0300614C`), base grip × 0xD0/256). The rigid body is placed by `FUN_08147ca0`: heading
   matrix → quaternion (`FUN_081477ac`), mass × 3, inertia `+0xC4` × 0x120/256. The centre-of-mass offset y is
   forced to −0x1800.
7. Base grips × 3/4. `FUN_0813e430` resets the race-controller globals and lets the car settle: 20 steps of
   sector, gravity, wheels (`dt` 0x800) and two integrations.
8. `FUN_0813f744` rebuilds the route distance tables (waypoint `+0x10`, side-segment scales at `0x03006120`).

### The dynamics (`FUN_0813d1f0`) in order (`car::dynamics`)
1. **Warnings and bookkeeping:**
   - off-route warning `0x0300601C` (±1 when far from the racing line at speed);
   - previous and pressed keys (`+0x4AE`);
   - wingman command (`FUN_0814078c`);
   - stuck check: tipped over for more than 100 steps (`+0x4E4`) with a corner down (`+0x4E6` = 0) and slow →
     `car_put_back_on_road` (`FUN_0814efa8`): the car onto its waypoint's floor, the body 0x1900 above, upright along
     the waypoint line (`orient_upright`, `FUN_08148f24`); momenta are kept.
2. **Route and timers:**
   - route segment by sector (`FUN_0813f234`);
   - the traffic countdown for the player (`FUN_08143b2c` → `traffic_spawn`);
   - gearbox timer `+0x9C` −1, the binding set, `dt`, the current gear's ratio;
   - torque-boost timer `+0x42C/+0x428`.
3. **Steering** `+0x20`: ±0xA000 per step up to ±0x80000, reset when released or reversed.
4. **Speeds:** `+0xA0 = 32·dot(velocity, forward)`, `+0x44 = |+0xA0|`. The profile (`*0x030056EC`) gets
   distance `+0x2D0` and top speed `+0x2DC`.
5. **Pedals** (frozen once the race is over, `0x03005780`): throttle `+0x24` (+0x2000 per step), brake `+0x28`.
   With the automatic gearbox:
   - braking below speed 0x3000 selects reverse (gear 0);
   - in reverse, accelerating below 0x3000 selects first (gear 2), and the brake is the throttle.
6. **Nitro** `+0x4D1`: action 7, with the tank `+0x4C8` at least 2 and gear 2 or higher.
7. **Slip and grip:**
   - slip angle = `atan2(velocity) − atan2(forward)`;
   - when it has the yaw rate's (`+0x15C`) sign, it damps the angular momentum y by curve `0x087F4164` × handling
     `+0x148`;
   - rear grip `+0x334/+0x3C8` = base `+0x338` × (1 + (curve `0x087F41A0`(0x733·|slip| + 0x8CD·|yaw|) −
     0x1000)/2).
8. **Speed** `= |velocity|`; stationary counter `+0x4EE`; neutral-gear rev logic.
9. **Engine:**
   - rpm `+0x3C` follows the wheels (`0x109A · (Σ wheel spin >> 8) × ratio >> 30 / 6`), or the throttle in neutral;
   - the rpm step is capped at ±1,000 per frame, and the excess becomes engine braking;
   - rpm is clamped to max (`+0x454`) and idle (handling `+0x68`);
   - torque = curve `+0x464` (10 points over max rpm / 8) × `+0x460` (halved at the limiter, × `+0x4CE` with nitro);
   - drive = torque × throttle × ratio × `+0x4BC` × `+0x428` (64-bit products, each >> 15), minus/plus
     rpm × `+0x458` (engine braking).
10. **Wheels:**
    - handbrake (action 6): rear grip halved, rear wheels braked 4×;
    - drive and brake go to the four wheel spins `+0x64` (drive share `+0x70`, brake `+0x74`).
11. **Sector and gravity:** sector of the entity position (`find_sector`), keeping the old one when none is found;
    `0x0300610C = 1`; gravity: momentum y += dt × (g `0x03006030` × mass) >> 11.
12. **Contacts:**
    - ground contact `FUN_08147ec4` (or `FUN_081484f0` when tipped over, `+0x138 < 0xF21`);
    - car-to-car test `FUN_08145320`;
    - parked stop: slow, 4 wheels down, no pedal → momentum, velocity, angular momentum and ω zeroed.
13. **Integration:** contact flags `+0x448 &= 8`; walls `FUN_08145ca8`; the body is integrated twice with `dt`
    (`FUN_08147b18`).
14. **Entity position** = body position − R·(0, `+0x43C`, `+0x440`); then the sector again (0xFFFF → the push-back
    loop at `0x0813DF98`: halve the step's move up to 6 times, searching from the start sector; then pull the car
    back 0x6400 along the move and rebuild the body position; still no sector → the start sector).
15. **"Drag":** the game scales a stack vector by `(speed/8)² >> 16 × handling +0x11C >> 6` and subtracts it
    from the velocity. That slot held the normalised velocity, but by now it holds the rotated centre-of-mass
    offset. (Handling `+0x11C` is 0 for every car, so this does nothing.) Then momentum = mass × velocity, and
    odometer `+0x90` += `+0xA0`.
16. **Heading and sounds:** heading `+0x00` = `atan2(forward x, z)`, entity `+0x2C` = heading << 8. Engine pitch;
    skid counter and skid sound (mean wheel slip > 0x3D090).
17. **Gears and route:**
    - gears: automatic `FUN_0813c02c` (upshift at `+0x44C` rpm, downshift when torque × ratio is better one gear
      down, then a 5-step pause), or the manual state machine on R/L (`0x03006074`);
    - route: waypoint `FUN_0813edd8`, progress `+0xAC` `FUN_0814032c`, gap `FUN_0813ebac` (`0x0300615C`);
    - nearest lane `+0xC0`;
    - hunter life `hunter_life_tick` (hunter races): `nfsgba_formats::career::hunter_life_tick` through the RAM
      adapters `car::race`/`car::racer`/`car::store_racer`.

### Rigid body (`FUN_08147b18`, physics `+0xC8`)
Explicit Euler, run twice per frame:
- position += dt·v;
- q += dt·(q ⊗ (ω/2, 0)), renormalised through the reciprocal table (`2^24/(k+1)` at `0x087C45F0`);
- R = matrix(q); v = inverse mass × momentum; ω = inverse inertia (a scalar) × angular momentum.

Forces are applied as impulses to momentum and angular momentum.

### Wheels on the ground (`FUN_08147ec4`)
For each wheel:
- its position in world axes = R·local;
- the sector under it comes from `find_camera_sector` (IWRAM), and the floor from `floor_height`;
- penetration = body y + wheel y + ride height (`+0x204`, wheel 0's for all) − floor.

When the penetration is positive:
- load = spring × penetration >> 8 + (n·v_contact) × damping >> 7 (damping −10 while not moving down), at least 0;
- tyre velocity = contact velocity + wheel spin along the rolling direction (front wheels turned by `+0x20 >> 8`,
  >> 9 in reverse);
- friction = −v/16, or −v̂ × grip/16 when |v|² > grip². Grip = wheel grip × load × surface factor (`0x087F5904`,
  floor sector `+0x08`), capped at 0x4000;
- the wheel spin changes with the friction along the rolling direction;
- the impulses dt·(friction, −load) go to momentum and, crossed with the contact point, to angular momentum.

### Walls (`FUN_08145ca8` → `FUN_081459b8` → `FUN_081457b8`)
The car tests the point 3/256 of its forward axis ahead of it against its sector's walls. It recurses once through
open portals, into the linked sector's floor sector. A wall is hit when the point:
- is within 100 units of the wall line (normal `+0x34/+0x36`);
- lies within its span, or no more than √0x270F (about 99.99) units past either end.

From the approaching normal velocity `vn`:
- restitution `k = clamp(−(vn·0x1A2 + 0x19D10000) >> 24, −25, −16)`;
- impulse `j = k·vn >> 10` along the normal.

Side effects:
- a breakable dynamic wall gives way above 0x2000 (`break_wall`, partner table `0x087F3CDA`);
- flags `+0x448` (0x10 contact, 2/4 side hits) and `+0x4B4` bit 0 (hard hit);
- the nearest lane;
- hunter races lose hunter life (`FUN_0814136c`);
- the scrape or crash sound.

### Sectors and floors
- `find_sector` (`FUN_0814fa04`) stores the query point in world `+0xC0/+0xC4/+0xC8` and the start sector in
  `+0xEA`. `find_camera_sector` (IWRAM `0x03000800`) tests the start sector, then every sector behind its open
  portals (flags without 0x1000, link `+0x32`), with `point_in_sector` (wrapping 32-bit cross products). A sector
  without a floor defers to `+0x20`. `FUN_0814dbbc` (two portals away) is the fallback.
- `floor_height` (`FUN_0814ca84`) = first wall `+0x38` + sector `+0x26` (+ dynamic wall `+6`, + plane `+6`), then
  the plane through the first wall's corner, `−(a·dx + c·dz) × recip(b) >> 16`. The plane is sector
  `+0x14/+0x16/+0x18`, or a sloped plane at world `+0x1C` when sector `+0x0A` is not 0xFFFF.

### Traffic spawn (`FUN_08143d48`; `traffic.rs`)
The player's step counts down `0x03006264` (reload from `0x03006260`) while traffic is on (`0x03006298`) and fewer
than 4 cars have spawned (`0x03006240`). At 0 it spawns:
- a free entity (`FUN_08137534`) and a 0x28-byte heap block;
- a lane: `rand_table` bit 0 picks the side;
- a waypoint: walk the main route from the player's waypoint (ahead, or behind when driving against the route)
  until the waypoint is past √0x18FFFFF units, at most 20 tries;
- the car is dropped if a racer (tested against the free entity's stale position) or one of the 8 live traffic
  cars (`0x03006270`) is within √0x8FFFF;
- orientation from the IWRAM `atan2_fast` (`0x03004470`); the lane offset from `0x087F5488`;
- a type from `0x087F546C` (counter `0x03005628` mod `0x0300625C`);
- handler 0x36.

Kinds 0 and 2 (`traffic::at_section_start`) come only from spawner entities (handlers 0x2D/0x2E and 0x2A:
`traffic_spawner_behind`, `traffic_spawner_ahead`), which no Carbon race creates: the car starts at the first
waypoint of the spawner's racing-line section, heading for the second (`atan2_fast`), with the unit direction at half
(kind 0) or full (kind 2) length; waypoint index `+0x9C` = 1. All kinds share the tail (`traffic::finish`): type,
sector list, live-traffic slot, the block's direction.

### Other contacts
- **Car to car** (`car_car_proximity` → `car_car_response`, `FUN_08144fa4`; `walls::contact`/`response`): the
  normal runs from the car to the later racer, the contact point is the midpoint. From the contact points' velocities
  (angular part >> 14, linear × 0x1555 >> 8) the normal speed `vn` gives `k = clamp(−16 − (vn + 0x1C000 >> 15), −21,
  −17)` and `j = k·vn >> 8`; the impulse `(n·j >> 10 >> 4)·0x3072 >> 14` changes both velocities; the angular
  impulse is the arm × impulse >> 25 (only 0 or −1 per component survives; kept). Momenta follow the velocities;
  contact flags; `non_racer_car_hit` for traffic; the other car of a collision with the player gets the torque timer
  (`+0x42C` = 0x12); lanes; crash sounds 0x17/0x18 by `j`. The hit mask is never reset between the later racers, so
  after one hit every later racer in range gets a response too (kept). In hunter races with `j` > 0x1000 the slower
  car (normalising both velocities in place) takes `hunter_hit` (`career::hunter_hit`).
- **Tipped over** (`FUN_081484f0`, `contact::tipped`): instead of the wheels, eight body corners (`0x087F2528`)
  below their floor push back: load `pen·0x30 >> 8` plus the floor-normal speed (5/32 or 15/64 by the sign of the
  vertical speed), friction capped by grip × surface, the torque's x component quartered (kept). The caller zeroes
  the wheel spins and counts `+0x4E4`; `+0x4E6` counts steps with no corner down, and landing after more than 4 plays
  sound 0x16 (0x19 after more than 8).
- **Suspension step** (`FUN_0814de40`, `contact::suspension`, only while `0x0300610C` is 0, which no race does): four
  points around the car turned by its heading find their floor; per point the vertical speed gains `0x320000/dt`, the
  height stops at the floor (bouncing back at −1/32), the spring and rate follow through a 64-bit division by a
  quarter of the mass, the rate is damped to 160/256; the entity's y is the mean height.

### Heap (`heap_alloc`, `heap_free`; `heap.rs`)
`*0x030064CC` holds 256 nodes of 8 bytes (u16 own index, next, start, end in words from `*0x030064D0`). Node 0
anchors a list sorted by address; end 0 marks a free node. An allocation takes the first gap that fits after a
block, leaving one spare word. `heap_alloc_zeroed` clears size/4 words, then size%4 bytes at the start again.

## Entity fields (0xA4 bytes)

| Offset | Type | Meaning |
|---|---|---|
| `+0x00` | u16 | entity index |
| `+0x02` | u16 | next entity in the sector's list (world `+0x0C` heads) |
| `+0x04` | u16 | draw-list link (renderer) |
| `+0x08` | u16 | flags: bit 0 active; bit 1 set = updated by `update_entities`, clear = while drawing; bit 2 collidable/visible |
| `+0x0A` | u16 | flags: bit 0 = entity `0x030057F8`; bit 2 set by other code |
| `+0x0C/+0x10/+0x14` | i32 ×3 | position, 8.8 city units (`-y` up) |
| `+0x28` | i32 | view depth (renderer) |
| `+0x2C` | i32 | heading << 8 |
| `+0x4A` | u16 | state: 0 init, 0x100 racing, 2 finished |
| `+0x4E` | u16 | handler index (table `0x087F38B8`) |
| `+0x72` | u16 | route segment (0 = main route) |
| `+0x78` | u16 | sector |
| `+0x88` | u8 | changed by other code (unknown) |
| `+0x89` | u8 | car id (handling record, car record) |
| `+0x8C` | u32 | physics struct (0x4FC bytes, heap) |
| `+0x90` | i16 | route waypoint within the segment |
| `+0x98`, `+0x9A` | u16 | y >> 8 after settling |

## Physics struct (0x4FC bytes, entity `+0x8C`)

| Offset | Meaning |
|---|---|
| `+0x00`, `+0x04` | heading (14-bit angle) and its sign word |
| `+0x08..+0x14`, `+0x48`, `+0x4C..+0x78` | suspension step `FUN_0814de40`: per point vertical speed `+0x08`, spring `+0x4C`, rate `+0x5C`, height `+0x6C`; `+0x48` points on the floor (4 at init) |
| `+0x20` | steering, ±0x80000 |
| `+0x24` | throttle, 0..0x9999 |
| `+0x28` | brake on |
| `+0x34`, `+0x38` | inertia terms from handling `+0x00/+0x08/+0x0C/+0x10` |
| `+0x3C` | engine rpm |
| `+0x40` | gear: 0 reverse, 1 neutral, 2..7 first..sixth |
| `+0x44` | speed, \|`+0xA0`\| |
| `+0x90` | odometer |
| `+0x9C` | automatic gearbox pause |
| `+0xA0` | forward speed × 32 |
| `+0xA8`, `+0xAC` | race position (set by the ranking), race progress |
| `+0xB0`, `+0xB4`, `+0xB8` | 0x7FFFFFFF at init, best lap time, lap start time (race time `0x03005800`) |
| `+0xC0` | u16 nearest lane (`0x087F4120`) |
| `+0xC5`, `+0xC6` | u8 laps left, laps |
| `+0xC8` | rigid body: `+0xC8` mass, `+0xCC` inverse mass, `+0xD0` position (8.8), `+0xE8` quaternion (x,y,z,w; 1.0 = 0x1000), `+0xF8` momentum, `+0x110` angular momentum, `+0x11C` velocity, `+0x128` rotation matrix (row 2 `+0x140..+0x148` = forward), `+0x158` angular velocity, `+0x164` quaternion rate, `+0x174` inertia, `+0x178` inverse inertia |
| `+0x188` | grid/skill value (from difficulty, AI skill or the wingman table `0x087F42E4`) |
| `+0x18C + 0x94·i` | wheel i (0,1 front): `+0x0C` contact point, `+0x18` position in world axes, `+0x48` position in car axes, `+0x64` spin, `+0x68` spring, `+0x6C` damping, `+0x70` drive share, `+0x74` brake, `+0x78` ride height, `+0x7C` slip², `+0x80` grip, `+0x84` base grip, `+0x88` 0x700, `+0x8C` recip(0x700), `+0x90` sector under the wheel |
| `+0x3DC` | sum of wheel spins |
| `+0x3E0..+0x404` | 10 upgrade levels (`+0x404` = 1 in career mode 2) |
| `+0x408..+0x424` | gear ratios (reverse, neutral, first..sixth) × final drive |
| `+0x428`, `+0x42C`, `+0x430` | torque multiplier (0x8000 once the timer runs out), timer, 1 << lane |
| `+0x434`, `+0x436` | u16 0xFFFF at init |
| `+0x438..+0x440` | centre-of-mass offset (y forced to −0x1800) |
| `+0x444` | launch/limiter counter |
| `+0x448` | contact flags (0x10 wall, 2/4 side, 1 blocks the parked stop, 8 kept) |
| `+0x44C`, `+0x450`, `+0x454`, `+0x458`, `+0x45C` | upshift rpm, lower rpm, max rpm, engine braking, shift rpm (torque peak) |
| `+0x460` | torque multiplier 0x1000..0x1800 (neutral revs) |
| `+0x464..+0x4A8` | torque curve (18 words from handling `+0x6C`; 10 used) |
| `+0x4AE` | u16 previous control word |
| `+0x4B4`, `+0x4B8`, `+0x4BC` | hard wall hit, 0x100, torque scale |
| `+0x4C0` | last touched wall with flag 0x4000 |
| `+0x4C4..+0x4D2` | nitro: `+0x4C8` tank, `+0x4CC` drain factor, `+0x4CE` torque factor, `+0x4D0` 6, `+0x4D1` on, `+0x4D2` 0x200 |
| `+0x4D8` | u16 route flags: 1 before the start line, 2 after waypoints 1..9, 0x10 no yaw damping |
| `+0x4E4`, `+0x4E6`, `+0x4E8`, `+0x4EC`, `+0x4EE`, `+0x4F0` | tipped counter, airborne counter, hunter life, wrong-way counter, stationary counter, u16 cleared on hunter wall hits |

## Handling records (`0x087F1100`, 0x158 bytes, index entity `+0x89`)

15 records, one per car. Offsets are bytes. The values are copied by the car init.

| Offset | Meaning | Record 0 |
|---|---|---|
| `+0x00` | mass | 1252 |
| `+0x08..+0x10` | inertia terms | 39616, 39616, 79233 |
| `+0x34..+0x50` | gear ratios: reverse, neutral, first..sixth | −570985, 0, 556227 … 131853 |
| `+0x54` | top gear | 7 |
| `+0x58`, `+0x150` | torque scale, base and fully upgraded | 11456, 21043 |
| `+0x64`, `+0x68` | max rpm, idle rpm | 8000, 850 |
| `+0x6C..+0xB0` | torque curve | |
| `+0xC4` | inertia (× 0x120/256) | |
| `+0xC8..+0xE8` | front: spring, damping, ride height, wheel x/y/z, drive share, brake, grip | 64, 40, 8192, (16053, −4096, 22467), 2048, 524288, 3712 |
| `+0xEC..+0x10C` | rear: the same | |
| `+0x110..+0x118` | centre-of-mass offset (y replaced by −0x1800) | (0, −4096, 5632) |
| `+0x11C` | drag (0 in every record) | 0 |
| `+0x148` | yaw damping | 2048 |
| `+0x14C`, `+0x154` | final drive, base and fully upgraded | 281, 190 |

Upgrades: `0x087F5988` holds 10 categories × 5 attribute weights (a, torque, final drive, grip, brakes).

The car table `0x087F0BD8` (`docs/formats/vehicle-models.md`) is not read by the car step.

## Verification

- **Traces:** `tools/mgba_remote.lua` `trace NAME [SKIP]` breaks on `FUN_0814bd4c` when r1 is the player's entity.
  - Each hit logs the frame, keys, a few globals, the entity and the physics struct to `NAME.csv`, and appends the
    full EWRAM + IWRAM to `NAME.ram.bin`.
  - `tools/trace_race.py` records the scenarios and stores that RAM as deltas from the first step's dump
    (`NAME.ramdelta`, about 0.2–3 MB per scenario).
  - Reruns give identical files.
- **Scenarios** (`tools/trace_race.py`):
  - from the reference race `race.ss` (23 s in, opponents out of reach): accel, brake, steer, wall, drive
    (R, A+L, R+L; sector 760→759), reverse (760→751→748), handbrake, long (760→759→760; waypoints 33→34→0→1
    over the start line);
  - start: from the menu (Quick Play, 3-lap circuit, heavy traffic) through the car init, the intro (phase 9),
    the launch, sectors 263→270→269→265 with a ramp, and a traffic spawn at step 96.
- **Oracle:** `tools/trace_oracle.py` runs `FUN_0814bd4c` from the ROM in unicorn on each step's full RAM
  (ARMv5 core; SWI Div emulated). It reproduces all 1,149 steps and writes each step's RAM changes and sound calls
  (`NAME.oracle.txt`). The sound entry points and `draw_decal_on_atlas` are stubbed and recorded; the IWRAM stack
  is ignored.
- **Rust** (`crates/nfsgba-sim/tests/trace.rs`):
  - `each_step_matches_the_trace`: from the full reference RAM at each step, the port gives the next traced car
    state, the same RAM writes and the same sound commands;
  - `replay_matches_the_trace`: the player's car runs on its own state, with the rest of RAM from the reference at
    each step, and reproduces every traced car state.
- **More scenarios** (physics-paths, session `physics-paths`, then copied to `vehicle-physics`): recorded with the
  autopilot `tools/trace_autopilot.lua` (steers from the racing line or at a car; loaded with the remote's `lua`
  command and driven by `luax`), counted with the probe `tools/trace_probe.lua`:
  - `hunter` (hunter race, ramming opponent 3): car-to-car responses, hunter hits and wall hits;
  - `tipped` (hunter race, hunting the nearest car): 86 tipped-over steps;
  - `stuck`: the tipped drive with the tipped counter raised to 100 in RAM after 20 tipped steps (a test input: the
    reset then runs on the game's own code at step 668);
  - `shortcut` (Longpoint): the lap's shortcut (section 1) with both barrier breaks (steps 833 and 1330);
  - `sprint`, `circuit`, `wingman` (recorded by the ai-traffic work): the wingman trace includes the wingman command.
  Steps where other code changes the player's car before the next one (a traffic car's collision response) are
  marked `external` in `NAME.oracle.txt`: only that step's own writes and sounds are compared, and the replay carries
  on from the game's state.
- **Oracle cases for paths no recording reaches** (`tests/trace.rs`, `tests/suspension.rs`):
  - `tools/trace_fuzz.py` (`fuzz.jsonl`): real steps with extreme speeds (far sector search, the player's and the
    opponents' push-back loops), random controls (manual gearbox, nitro, wingman command), and the car init with
    random career globals on every racer slot; 4,500 cases;
  - `tools/trace_calls.py` (`calls.jsonl`): `traffic_spawn` in all kinds, `wingman_command` in both roles,
    `lap_crossing` (493 crossings with knock-outs and finishes); 4,500 cases;
  - `tools/trace_suspension.py` (`suspension.jsonl`): the suspension step on the reference race with random racers,
    681 sectors, speeds, springs and frame times; 3,000 cases.
  Mutation checks: one changed constant in each ported path (push-back distances, AI halving, manual shift, nitro
  drain and torque, career grid, wingman row, spawn speed, suspension damping, neutral revs, side-hit flags) breaks
  between 10 and 3,000 cases or trace steps.
- **Fields maintained elsewhere** are not compared: entity `+0x02`, `+0x04`, `+0x0A` bit 2, `+0x28`, `+0x88`;
  physics `+0xA8`.
- **Sensitivity:** changing a single constant (the damping −10 to −11) breaks both tests.
- Unit tests pin the integer helpers and the IWRAM divider's quirks.

## Paths no race reaches

Everything in the car step is ported. These parts are exact by the function oracle rather than by a recording:
- the suspension step `FUN_0814de40` (`0x0300610C` is 1 at every car step of every trace; `race_init` and each
  dynamics step set it);
- traffic spawn kinds 0 and 2 (spawner entities no Carbon race creates);
- `find_sector_far` and the push-back loops (only at speeds no drive reaches);
- the manual gearbox, active nitro, and the career branches of the car init (no recording uses them).

NOT 1:1 by design: `draw_decal_on_atlas` (`FUN_0813bd90`) is rendering and belongs to the renderer. It is called
from the racing step and from `unpack_decal`.

## Integration notes

**FIDELITY.md:**
- **D4 (gameplay): partly closed.** The car entity's per-frame update is exact and trace-verified: car init,
  dynamics, ground contact, walls, integration, route tracking, the traffic spawn from the player's step and the
  sound commands. That covers 1,149 steps over 9 scenarios, per step (full RAM) and as a replay
  (`crates/nfsgba-sim/tests/trace.rs`, `docs/engine/physics.md`).
- **D5:** `lap_crossing` is still not trace-checked (the traces cross the line without completing a lap).
- **New open entries:**
  - the suspension step `FUN_0814de40`;
  - tipped-over dynamics `FUN_081484f0`;
  - car-to-car response `FUN_08144fa4` / `hunter_hit`;
  - the far sector search `FUN_0814dbbc` and the push-back;
  - the stuck reset `FUN_0814efa8`;
  - traffic spawn kinds 0/2;
  - the wingman command;
  - `hunter_life_tick`;
  - the lap/finish tail;
  - the AI and traffic handlers (0x36).
- **New rendering entry:** `draw_decal_on_atlas` is called from the car step and from `unpack_decal`.

**address-map.md** (rows not on main yet):

| Address | What |
|---|---|
| `0x03005640` | frame time (25,500 / timer-3 ticks, 10..100; 15 if `0x03005624` is 2), written by `main_frame` |
| `0x030057D8` | u16 control word per entity (`0xFC00 \| keys` for the player) |
| `0x0300629C` | u8 control binding set (0 automatic, 1 manual) |
| `0x03000048` | race phase (9 intro, 2 racing, 3 over; 1 and 4 skip the dynamics) |
| `0x0300610C` | set by the dynamics; while 0 the racing step runs the suspension step |
| `0x0300601C` | off-route warning (−1/0/1) |
| `0x03005384` | wrong way (more than 27 steps against the route) |
| `0x0300615C` | time gap to the car ahead/behind |
| `0x03006030` | gravity (0x4F0, set by the car init) |
| `0x03006074` | manual gearbox state |
| `0x03005FB4` | pointer to the waypoint lines (0x20 each: direction, widening, crossing plane, length) |
| `0x03006120` | per route segment: distance scale (0x100 for the main route) |
| `0x030060C0` | 16 words: route segments visited |
| `0x03006000` | 5 words: the player's upgrade totals; `+0x10` nitro level × 10 |
| `0x030060A4` | the player's final drive |
| `0x03006154` | time limit (0x4650; 0x2328 in career) |
| `0x03006078`, `0x03006084`, `0x03006088`, `0x03006028`, `0x03006090`, `0x03006158`, `0x03006190` | race-start globals set by `race_start_setup` |
| `0x03006270` | 8 × pointer: live traffic cars |
| `0x03006240`, `0x03006264`, `0x03006260`, `0x03006298`, `0x0300625C` | traffic: spawned count (max 4), countdown (byte), countdown reload, traffic on, type count |
| `0x03005628` | counter that picks the traffic type (hypothesis: a frame counter) |
| `0x03006480` | IWRAM divider remainder |
| `0x03006150`, `0x0300614C` | nitro full-tank flag; grip-doubling flag |
| `0x03006184`, `0x030061F0` | hunter wall-hit factor; traffic wall-hit flag |
| world `+0xC0/+0xC4/+0xC8/+0xEA` | sector query point and start sector |
| world `+0xF8`, `+0xFA` | first entity and count for the update loop and the free-entity search |
| `0x087F1100` | handling records (15 × 0x158) |
| `0x087F5494` | control bindings (4 × 9 × 8 bytes) |
| `0x087F5904` | grip per floor surface (8 words) |
| `0x087F4164`, `0x087F41A0` | curves: yaw damping by slip angle, rear grip by slip and yaw |
| `0x087F37D8` | per route: side-segment sector lists |
| `0x087F3CDA` | breakable-wall partners |
| `0x087F4120` | lane offsets (4, for `nearest_lane`) |
| `0x087F5488` | traffic lane offsets |
| `0x087F546C` | traffic types (u16 model, u16 paint) |
| `0x087F55FC` | car-to-car axle offsets (2) |
| `0x087F5988` | upgrade weights (10 × 5 words) |
| `0x087F42E4` | grid/skill value per wingman |
| `0x087F3050`, `0x087F40D0` | per route: race-start byte; per (setting, reverse): race-start word |
| `0x087F3DD0` | the zero vector |
| `0x03000220` (IWRAM, ARM) | signed divide with remainder store (`iwram_divmod`) |

**symbols.csv:** 70 new rows (none duplicates main's 244), plus three corrections of existing rows.

Corrections (replace the existing rows):

```
0x0814b98c,car_init,function,car handler state 0: physics struct; handling; upgrades; settling; route tables
0x0814b168,car_racing_step,function,nitro drain; dynamics; visibility flags; rim redraw; suspension step when 0x0300610C is 0
0x08160de4,heap_free,function,frees a heap_alloc block (verified by the car init and traffic traces)
```

New rows:

```
0x0813765c,update_entities,function,calls the entity handler table 0x087F38B8 for each active entity (flags +0x08 & 3 == 3)
0x0814bd4c,car_handler,function,entity handler 0..3: sector-list unlink/link; state 0 car_init; 0x100 racing; 2 finished (race-over fade)
0x0814f510,car_unlink,function,sector_list_unlink(world; *entity)
0x0814f51c,car_link,function,sector_list_link(world; *entity)
0x081375ac,sector_list_unlink,function,removes an entity from its sector's list (world +0x0C heads; links at entity +0x02)
0x08137578,sector_list_link,function,pushes an entity onto its sector's list
0x0814b2a8,car_setup_handling,function,engine/gears/body/wheels from the handling record 0x087F1100 and the upgrades
0x0814b098,car_nitro_drain,function,drains the nitro tank physics +0x4C8 while nitro is on
0x0813d1f0,car_dynamics,function,one car step: controls; engine; gearbox; tyres; walls; integration; route (docs/engine/physics.md)
0x0813c02c,car_auto_shift,function,automatic gearbox (5-step pause at physics +0x9C)
0x08147ec4,car_wheel_contact,function,four wheels on the ground: suspension load and tyre friction impulses
0x081484f0,car_tipped_dynamics,function,dynamics while tipped over (physics +0x138 < 0xF21)
0x0814de40,car_suspension,function,suspension step while 0x0300610C is 0
0x08145320,car_car_proximity,function,swept proximity test against the other racers
0x08144fa4,car_car_response,function,car-to-car collision response
0x08145ca8,car_walls,function,wall test at the point ahead of the car; scrape/crash sounds
0x081459b8,car_wall_search,function,walls of a sector; recursing once through open portals
0x081457b8,car_wall_response,function,wall impulse with clamped restitution; side flags; hunter hit
0x0814136c,hunter_wall_hit,function,hunter races: wall impulse costs hunter life (physics +0x4E8)
0x0813b5a0,break_wall,function,a breakable dynamic wall and its partner (0x087F3CDA) lose the solid flag
0x08141284,traffic_wall_hit,function,clears 0x030061F0 (non-racer hitting a wall)
0x081410a8,is_non_racer,function,entity index > opponent count
0x08147b18,body_integrate,function,rigid body Euler step (position; quaternion; matrix; velocities)
0x08148fe8,body_update_velocities,function,v = m^-1 p and w = I^-1 L
0x08147ca0,body_init,function,rigid body at a position and heading; at rest
0x081477ac,matrix_to_quat,function,unit quaternion of a rotation matrix
0x08147618,quat_mul,function,quaternion product (20.12)
0x081476d8,quat_matrix,function,rotation matrix of a quaternion (20.12)
0x08147a6c,normalize_12,function,normalise a vector to 0x1000 via isqrt and the reciprocal table; returns the length
0x0815fdd0,normalize_14,function,normalise a vector to 0x4000 by division
0x08149150,dot_12,function,20.12 dot product
0x08149038,vec_sub,function,a - b
0x08149058,vec_add,function,a + b
0x081490e0,vec_scale,function,s * v >> 12
0x08149078,vec_mat_mul,function,v times a 3x3 matrix (20.12)
0x0815fbd0,cross,function,cross product without scaling
0x0815f9cc,atan2_q14,function,angle of (x; z); 0x4000 per turn
0x0815fa54,isqrt,function,integer square root of a word (at least 1)
0x08149178,recip_q24,function,about 2^24 / x via the reciprocal table 0x087C45F0
0x0816a8b4,__muldi3,function,libgcc 64-bit multiply
0x0814fa04,find_sector,function,sector containing a point (query at world +0xC0..+0xEA); find_camera_sector then find_sector_far
0x0814dbbc,find_sector_far,function,sector search two portals away
0x0814ca84,floor_height,function,floor height of a sector at x z (8.8): wall +0x38 + sector +0x26 and the floor plane
0x0814f4a8,floor_sector,function,sector record or the one it defers its floor to (+0x20)
0x08144f38,control_action,function,control binding test (table 0x087F5494; set 0x0300629C)
0x08144f1c,select_bindings,function,binding set 0 with the automatic gearbox; else 1
0x0814007c,route_waypoint,function,waypoint address of segment + index
0x0814009c,route_waypoint_at,function,normalised waypoint address
0x0813e93c,route_along_line,function,how far along its waypoint line a car is
0x0814032c,car_route_progress,function,a car's distance along the route (physics +0xAC)
0x0813f234,route_track_segment,function,main route / side segment switching by sector (0x087F37D8)
0x0813edd8,route_track_waypoint,function,waypoint advance; wrong-way counter physics +0x4EC
0x0813ebac,route_gap,function,time gap to the neighbouring car (0x0300615C)
0x081402bc,route_lateral,function,signed distance across the route at the car's waypoint
0x08140274,nearest_lane,function,nearest of the 4 lane offsets 0x087F4120
0x0814078c,wingman_command,function,control action 8 (R+L) with a wingman
0x0814efa8,car_put_back_on_road,function,resets a stuck car onto the route
0x08160b94,heap_alloc_zeroed,function,heap_alloc then clear (size/4 words; then size%4 bytes at the start)
0x08160cbc,fill_words,function,fill n words
0x08160c84,fill_bytes,function,fill n bytes
0x08140148,car_upgrades,function,upgrade levels physics +0x3E0 from the car record bytes +7..+16
0x0814f198,car_nitro_setup,function,nitro tank and factors
0x0813e430,race_start_setup,function,race-controller globals; 20 settling steps of the car on the ground
0x0813f744,route_distances,function,cumulative waypoint distances (+0x10); side-segment scales 0x03006120
0x081400bc,race_time_limit,function,*0x03006154 = 0x4650 (0x2328 in career)
0x08143b2c,traffic_countdown,function,player step: spawns traffic when the countdown 0x03006264 runs out
0x08143d48,traffic_spawn,function,traffic car on the main route ahead/behind the player in a random lane
0x08137534,free_entity,function,first entity from world +0xF8 (+0xFA of them) whose flag bit 0 is clear
0x03004470,atan2_fast,function,IWRAM ARM: atan2 via recip_div_16 (divisor clamped to 0x7FFE)
0x03000220,iwram_divmod,function,IWRAM ARM: signed divide storing a remainder; negative divisors divide by -|a|
```

## Integration notes (physics-paths)

**FIDELITY.md** (every item exact; `crates/nfsgba-sim` has no `Unported` left outside `ai.rs`/`traffic_ai.rs`):
- **D9 closed.** Suspension step `FUN_0814de40` (3,000 oracle cases; unreachable in races), tipped-over dynamics
  `FUN_081484f0` (`tipped` trace: 709 steps, 86 tipped; opponent path in `ai_trace` and the fuzz), stuck reset
  `FUN_0814efa8` with `FUN_08148f24` (`stuck` trace step 668; the AI's two call sites use it too).
- **D10 closed.** Car-to-car response `FUN_08144fa4` and hunter hits (`hunter` trace, 697 steps; `ai_trace` no longer
  expects stops for it). Hunter life per frame, hits and wall drains use `career::hunter_life_tick`, `hunter_hit`,
  `hunter_drain` through RAM adapters (the walls.rs comment now names `hunter_wall_hit`).
- **D11 closed.** `find_sector_far` `FUN_0814dbbc` and the player's and the opponents' push-back loops (fuzz: 79 far
  searches, 47 player and 28 opponent push-backs; the opponent's loop has no pull-back).
- **D12 closed.** Traffic spawn kinds 0 and 2 (1,500 direct oracle cases; spawner entities no race creates).
- **D13 closed.** Manual gearbox, nitro (drain and torque), neutral revs and side-hit flags (control fuzz, mutation
  checked); breakable walls and side route segments (`shortcut` trace); hunter wall hits (`hunter`); the career
  and wingman branches of the car init (init fuzz; `wingman` trace); the wingman command `FUN_0814078c` (`wingman`
  trace plus 1,500 direct cases).
- **D5 (lap tail):** `route::lap` now runs `career::RacingLine::lap_crossing` (4,500 direct cases, 493 crossings), so
  the player and AI cars finish sprints and laps; `EXPECTED_STOPS` in `tests/ai_trace.rs` is empty (12,472 calls
  exact, 0 stopped).
- **Left for others:** the six `Unported` stops in `ai.rs`/`traffic_ai.rs` (AI hunter mode `FUN_0813fdb0`, the
  player's car on the opponent handler, `FUN_0814dd24` outside every sector, and three more): AI scope. The
  `hunter`, `tipped`, `stuck` traces can feed `ai_trace` once AI hunter mode is ported.

**Notes CSVs:** `docs/engine/notes/symbols.physics-paths.csv` (5 rows) and `addresses.physics-paths.csv` (17 rows).
The `driver` rows (`+0x08`, `+0x48`, `+0x4C`, `+0x5C`, `+0x6C`) are new rows of the address map's Driver table:
`notes_merge.py` reports them as edits because it matches struct offsets across tables. `0x030061DC`: the AI notes
call it the gap setting (`0x7F4284`); `wingman_command` spends one per command, so it is the command count loaded
from that table.

**Tools** (for TOOLS.md): `tools/trace_autopilot.lua` (deterministic autopilot: racing line, ram a car, hunt the
nearest, take a shortcut, raise the tipped counter), `tools/trace_probe.lua` (call counts of chosen functions, the
player's split out), `tools/trace_fuzz.py`, `tools/trace_calls.py`, `tools/trace_suspension.py` (oracle cases), and
the remote's `lua FILE` / `luax STATEMENT` commands in `tools/mgba_remote.lua`.

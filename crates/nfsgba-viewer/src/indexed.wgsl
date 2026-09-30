// GBA-style indexed colour: 8-bit palette indices looked up in a 256-colour palette, one texel per pixel.
// The texel is picked by integer coordinates and wrapped like the game's power-of-two masks (`u & (w - 1)`);
// Euclidean modulo gives the same result for those sizes. The window is the GBA's 240×160 screen scaled up.
//
// Index 0 is the backdrop: palette entry 0, which the VCount IRQ sets to a sky gradient colour per pair of screen
// lines (`backdrop`: one colour per GBA line, `nfsgba_formats::sky`). OPAQUE surfaces show it (the opaque wall
// drawer 0x03004db0 writes index 0 like any index); the others skip the pixel. PAIRS surfaces are transparent walls
// (0x03004d48): the game draws them in pixel pairs of the 240-column screen and skips a pair unless both of its
// texels are non-zero, each pixel sampling its own texture column at its left edge, the pair sharing its row.
// Here the pair's two texels are found by stepping u along the screen from this fragment (`dpdx`).
// NOT 1:1 (R14): at high resolution the fragment keeps its own texel; the pair test uses the fragment's row.
//
// City fragments carry their sector (`uv_b.x`) and, for walls, 1 + the wall's index in the sector (`uv_b.y`; 0 for
// floors and ceilings). With a portal list (texel (0, 0) of `portals` = entry count, -1 = none) a fragment is drawn
// only inside the screen span of an entry of its sector, pixels `left ..= right` and rows `top .. bottom` (so row
// 159 is never drawn), and a wall only if the entry's wall mask (row 1) has it. CULL surfaces drop back faces.
// NOT 1:1 (R27): the game's spans are its projected portal ends rounded down, and it draws walls in 2-pixel
// column pairs clipped to pairs `left >> 1 .. right >> 1`, so its surfaces meet on whole pixels. Here the geometry
// is continuous: a surface behind a portal ends where the portal's continuous end lies, up to a pixel past the
// rounded `right`, so the span keeps that pixel (clipping any tighter opens a hairline where the backdrop shows).
//
// Car models: index 0 shows the backdrop (OPAQUE), as the game's span drawer writes every texel and palette entry 0 is
// the backdrop; the cars are clipped to the rectangle of the portal entry they were drawn through.
//
// SCREEN surfaces read `indices` as the 240×160 GBA screen at the pixel's screen position (the sky layer).
//
// Hidden surfaces (R10): the game paints the list from its last entry to its first (`draw_visible_sectors`), then the
// cars of every entry over all of it. A city fragment writes the depth `0.5 (count - entry + z) / (count + 1)` (z: the
// real reversed depth, 0..1), so a nearer entry covers a farther one as in the painter's order and, within an entry, the
// real depth decides; a clipped car writes `0.5 + 0.5 z`, above all of the city, as pass 1 overwrites the sectors.

#import bevy_pbr::{forward_io::VertexOutput, mesh_view_bindings::view}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var indices: texture_2d<u32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var palette: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var backdrop: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var<uniform> mode: vec4<u32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var portals: texture_2d<i32>;

const OPAQUE: u32 = 1u;
const SCREEN: u32 = 2u;
const CULL: u32 = 4u;
const PAIRS: u32 = 8u;
// The sky layer writes this alpha, everything else 1.0: nothing blends (the window ignores it), and the visual check
// (`shots.rs`) tells sky from geometry by it.
const SKY_ALPHA: f32 = 0.98;

struct Out {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
}

fn texel(uv: vec2<f32>) -> u32 {
    let size = vec2<i32>(textureDimensions(indices));
    let t = vec2<i32>(floor(uv * vec2<f32>(size)));
    return textureLoad(indices, ((t % size) + size) % size, 0).r;
}

// The number (1..) of the first list entry that draws GBA pixel `p` of `sector` (wall `wall` of it, or a flat when `wall`
// < 0) - the game draws the list from its last entry to its first, so the lowest number is drawn last - 0 when there is
// no list (draw everything), -1 when no entry draws it.
fn listed(sector: i32, wall: i32, p: vec2<i32>) -> i32 {
    let count = textureLoad(portals, vec2<i32>(0, 0), 0).x;
    if count < 0 {
        return 0;
    }
    for (var k = 1; k <= count; k++) {
        let e = textureLoad(portals, vec2<i32>(k, 0), 0);
        if e.x != sector || p.y < (e.w & 0xFFFF) || (e.w >> 16u) <= p.y {
            continue;
        }
        if wall >= 0 {
            let mask = textureLoad(portals, vec2<i32>(k, 1), 0);
            let bits = select(select(select(mask.x, mask.y, wall >= 32), mask.z, wall >= 64), mask.w, wall >= 96);
            if ((bits >> u32(wall & 31)) & 1) == 0 {
                continue;
            }
        }
        if e.y <= p.x && p.x <= e.z {
            return k;
        }
    }
    return -1;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) front: bool) -> Out {
    var depth = in.position.z;
    // The window scaled onto the GBA's 240×160 screen.
    let column = view.viewport.z / 240.0;
    let at = (in.position.xy - view.viewport.xy) / view.viewport.zw * vec2<f32>(240.0, 160.0);
    let gba = clamp(vec2<i32>(floor(at)), vec2<i32>(0, 0), vec2<i32>(239, 159));
    // Derivatives first, in uniform control flow: the texture's change per window pixel across the screen.
    let du = dpdx(in.uv);
    if (mode.x & CULL) != 0u && !front {
        discard;
    }
#ifdef VERTEX_UVS_B
    let entry = listed(i32(round(in.uv_b.x)), i32(round(in.uv_b.y)) - 1, gba);
    if entry < 0 {
        discard;
    }
    if entry > 0 {
        // The list is drawn from its last entry to its first: a later entry's surfaces cover an earlier one's; within
        // an entry the real depth decides. The city takes the lower half of the depth range, cars (pass 1) the upper.
        let count = textureLoad(portals, vec2<i32>(0, 0), 0).x;
        depth = 0.5 * (f32(count - entry) + in.position.z) / f32(count + 1);
    }
#else
    if mode.w != 0u {
        depth = 0.5 + 0.5 * in.position.z;
    }
    // A car (R29): only inside the clip rectangle of the portal entry the game drew it through (`mode.yz`, biased by
    // 1024: left | right << 16, top | bottom << 16; right and bottom exclusive).
    if mode.w != 0u {
        let x = vec2<i32>(i32(mode.y & 0xFFFFu), i32(mode.y >> 16u)) - 1024;
        let y = vec2<i32>(i32(mode.z & 0xFFFFu), i32(mode.z >> 16u)) - 1024;
        if gba.x < x.x || gba.x >= x.y || gba.y < y.x || gba.y >= y.y {
            discard;
        }
    }
#endif
    var index = texel(in.uv);
    let alpha = select(1.0, SKY_ALPHA, (mode.x & SCREEN) != 0u);
    if (mode.x & SCREEN) != 0u {
        index = textureLoad(indices, gba, 0).r;
    }
    if (mode.x & PAIRS) != 0u && index != 0u {
        // The pair's two pixels at their left edges, on this fragment's row.
        let left = view.viewport.x + f32(gba.x & ~1) * column;
        // Only u steps to the neighbour: the pair shares this fragment's row, and stepping v as well would read
        // past the wall's foot (a wrapped, often transparent row) along sloping edges.
        let uv = vec2<f32>(in.uv.x + du.x * (left - in.position.x), in.uv.y);
        if texel(uv) == 0u || texel(vec2<f32>(uv.x + du.x * column, uv.y)) == 0u {
            discard;
        }
    }
    if index == 0u {
        if (mode.x & OPAQUE) == 0u {
            discard;
        }
        return Out(vec4<f32>(textureLoad(backdrop, vec2<i32>(gba.y, 0), 0).rgb, alpha), depth);
    }
    return Out(vec4<f32>(textureLoad(palette, vec2<i32>(i32(index), 0), 0).rgb, alpha), depth);
}

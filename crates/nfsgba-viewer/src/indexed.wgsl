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
// City fragments carry their sector (`uv_b.x`) and whether they belong to a wall (`uv_b.y`). With a portal list
// (texel 0 of `portals` = entry count, -1 = none) a fragment is drawn only inside the screen span of an entry of
// its sector: walls in whole pixel pairs (`raster_wall_columns` clips to columns `left >> 1 .. right >> 1`), flats
// in pixels `left .. right`, both in rows `top .. bottom` (so row 159 is never drawn). CULL surfaces drop back faces.
//
// SCREEN surfaces read `indices` as the 240×160 GBA screen at the pixel's screen position (the sky layer).

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

fn texel(uv: vec2<f32>) -> u32 {
    let size = vec2<i32>(textureDimensions(indices));
    let t = vec2<i32>(floor(uv * vec2<f32>(size)));
    return textureLoad(indices, ((t % size) + size) % size, 0).r;
}

// Whether GBA pixel `p` of `sector` lies in the span of one of the portal list's entries for that sector.
fn listed(sector: i32, wall: bool, p: vec2<i32>) -> bool {
    let count = textureLoad(portals, vec2<i32>(0, 0), 0).x;
    if count < 0 {
        return true;
    }
    for (var k = 1; k <= count; k++) {
        let e = textureLoad(portals, vec2<i32>(k, 0), 0);
        var span = vec2<i32>(e.y, e.z);
        if wall {
            span = (span >> vec2<u32>(1u)) * 2;
        }
        if e.x == sector && span.x <= p.x && p.x < span.y && (e.w & 0xFFFF) <= p.y && p.y < (e.w >> 16u) {
            return true;
        }
    }
    return false;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
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
    if !listed(i32(round(in.uv_b.x)), in.uv_b.y > 0.5, gba) {
        discard;
    }
#endif
    var index = texel(in.uv);
    if (mode.x & SCREEN) != 0u {
        index = textureLoad(indices, gba, 0).r;
    }
    if (mode.x & PAIRS) != 0u && index != 0u {
        // The pair's two pixels at their left edges, on this fragment's row.
        let left = view.viewport.x + f32(gba.x & ~1) * column;
        let uv = in.uv + du * (left - in.position.x);
        if texel(uv) == 0u || texel(uv + du * column) == 0u {
            discard;
        }
    }
    if index == 0u {
        if (mode.x & OPAQUE) == 0u {
            discard;
        }
        return vec4<f32>(textureLoad(backdrop, vec2<i32>(gba.y, 0), 0).rgb, 1.0);
    }
    return vec4<f32>(textureLoad(palette, vec2<i32>(i32(index), 0), 0).rgb, 1.0);
}

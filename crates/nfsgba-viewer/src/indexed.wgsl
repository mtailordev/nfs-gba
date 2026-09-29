// GBA-style indexed colour: 8-bit palette indices looked up in a 256-colour palette, one texel per pixel.
// The texel is picked by integer coordinates and wrapped like the game's power-of-two masks (`u & (w - 1)`);
// Euclidean modulo gives the same result for those sizes.
//
// Index 0 is the backdrop: palette entry 0, which the VCount IRQ sets to a sky gradient colour per pair of screen
// lines (`backdrop`: one colour per GBA line, `nfsgba_formats::sky`). OPAQUE surfaces show it (the opaque wall
// drawer 0x03004db0 writes index 0 like any index); the others skip the pixel.
// NOT 1:1 (R14): the game's transparent wall drawer (0x03004d48) skips a whole pixel pair unless both texels are
// non-zero; here each pixel is skipped on its own.
//
// SCREEN surfaces read `indices` as the 240×160 GBA screen at the pixel's screen position (the sky layer).

#import bevy_pbr::{forward_io::VertexOutput, mesh_view_bindings::view}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var indices: texture_2d<u32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var palette: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var backdrop: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var<uniform> mode: vec4<u32>;

const OPAQUE: u32 = 1u;
const SCREEN: u32 = 2u;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // The window scaled onto the GBA's 240×160 screen.
    let at = (in.position.xy - view.viewport.xy) / view.viewport.zw * vec2<f32>(240.0, 160.0);
    let gba = clamp(vec2<i32>(floor(at)), vec2<i32>(0, 0), vec2<i32>(239, 159));
    let size = vec2<i32>(textureDimensions(indices));
    var texel = vec2<i32>(floor(in.uv * vec2<f32>(size)));
    if (mode.x & SCREEN) != 0u {
        texel = gba;
    }
    let index = textureLoad(indices, ((texel % size) + size) % size, 0).r;
    if index == 0u {
        if (mode.x & OPAQUE) == 0u {
            discard;
        }
        return vec4<f32>(textureLoad(backdrop, vec2<i32>(gba.y, 0), 0).rgb, 1.0);
    }
    return vec4<f32>(textureLoad(palette, vec2<i32>(i32(index), 0), 0).rgb, 1.0);
}

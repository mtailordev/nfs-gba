// GBA-style indexed colour: 8-bit palette indices looked up in a 256-colour palette, one texel per pixel.
// The texel is picked by integer coordinates and wrapped like the game's power-of-two masks (`u & (w - 1)`);
// Euclidean modulo gives the same result and also covers the 240-wide skyline. Index 0 is not drawn.
// NOT 1:1 (docs/engine/viewer-rendering.md, R14): the game's transparent wall drawer (0x03004d48) skips a whole
// pixel pair unless both texels are non-zero, and opaque walls (0x03004db0) write index 0 as the backdrop.

#import bevy_pbr::forward_io::VertexOutput

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var indices: texture_2d<u32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var palette: texture_2d<f32>;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(indices));
    let texel = vec2<i32>(floor(in.uv * vec2<f32>(size)));
    let index = textureLoad(indices, ((texel % size) + size) % size, 0).r;
    if index == 0u {
        discard;
    }
    return vec4<f32>(textureLoad(palette, vec2<i32>(i32(index), 0), 0).rgb, 1.0);
}

// The last pass (G2): the 3D scene (an sRGB image at window size) and the HUD layer (the game's sprites on the 240×160
// screen, raw gamma bytes: alpha 255 = an opaque sprite pixel, 128 = a semi-transparent one, 0 = none).
// The GBA blends in 5-bit gamma space, `min(31, (obj·EVA + bg·EVB) >> 4)` per channel (BLDALPHA: EVA = `blend.x`,
// EVB = `blend.y`), and then shows the 5-bit colour as `c << 3 | c >> 2`. The scene is read back as its sRGB bytes
// (the image is sRGB, so a read gives linear values; they are encoded again and rounded), both are cut to 5 bits and
// mixed with the game's integer formula. Pixels without a semi-transparent sprite pass through unchanged.

#import bevy_sprite::mesh2d_vertex_output::VertexOutput

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var scene: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var hud: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var<uniform> blend: vec4<u32>;

fn encode(c: f32) -> f32 {
    return select(1.055 * pow(c, 1.0 / 2.4) - 0.055, 12.92 * c, c <= 0.0031308);
}

fn decode(c: f32) -> f32 {
    return select(pow((c + 0.055) / 1.055, 2.4), c / 12.92, c <= 0.04045);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(scene));
    let s = textureLoad(scene, vec2<i32>(in.position.xy), 0);
    let gba = clamp(vec2<i32>(floor(in.position.xy / size * vec2<f32>(240.0, 160.0))), vec2<i32>(0), vec2<i32>(239, 159));
    let h = textureLoad(hud, gba, 0);
    // The scene's gamma bytes.
    var bytes = vec3<u32>(round(vec3<f32>(encode(s.r), encode(s.g), encode(s.b)) * 255.0));
    if h.a > 0.99 {
        bytes = vec3<u32>(round(h.rgb * 255.0));
    } else if h.a > 0.01 {
        let obj = vec3<u32>(round(h.rgb * 255.0)) >> vec3<u32>(3u);
        let back = bytes >> vec3<u32>(3u);
        let c = min(vec3<u32>(31u), (obj * blend.x + back * blend.y) >> vec3<u32>(4u));
        bytes = (c << vec3<u32>(3u)) | (c >> vec3<u32>(2u));
    }
    let g = vec3<f32>(bytes) / 255.0;
    return vec4<f32>(decode(g.r), decode(g.g), decode(g.b), s.a);
}

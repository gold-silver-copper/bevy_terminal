@group(0) @binding(0) var glyph_atlas: texture_2d<f32>;
@group(0) @binding(1) var glyph_sampler: sampler;

struct VertexInput {
    @location(0) rect: vec4<f32>,
    @location(1) uv: vec4<f32>,
    @location(2) color: vec4<f32>,
    @location(3) background: f32,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) solid: u32,
    @location(3) @interpolate(flat) background: f32,
}

@vertex
fn vertex(input: VertexInput, @builtin(vertex_index) index: u32) -> VertexOutput {
    let corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 1.0),
    );
    let corner = corners[index % 6u];
    var output: VertexOutput;
    output.position = vec4<f32>(mix(input.rect.xy, input.rect.zw, corner), 0.0, 1.0);
    output.uv = mix(input.uv.xy, input.uv.zw, corner);
    output.color = input.color;
    output.solid = select(0u, 1u, input.uv.w < 0.0);
    output.background = input.background;
    return output;
}

fn linearize(v: f32) -> f32 {
    return select(pow((v + 0.055) / 1.055, 2.4), v / 12.92, v <= 0.04045);
}

fn unlinearize(v: f32) -> f32 {
    return select(pow(v, 1.0 / 2.4) * 1.055 - 0.055, v * 12.92, v <= 0.0031308);
}

fn luminance(color: vec3<f32>) -> f32 {
    return dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
}

@fragment
fn fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    if input.solid != 0u {
        return input.color;
    }
    let sample = textureSample(glyph_atlas, glyph_sampler, input.uv);
    if input.color.a < 0.0 {
        return sample;
    }
    // Ghostty's linear-corrected blending: blend in linear light, with the
    // coverage remapped so the result has the luminance a gamma-space blend
    // of the text and cell background would have.
    var coverage = sample.a;
    let bg_l = input.background;
    let fg_l = luminance(input.color.rgb);
    if bg_l >= 0.0 && abs(fg_l - bg_l) > 0.001 {
        let blend_l = linearize(unlinearize(fg_l) * coverage + unlinearize(bg_l) * (1.0 - coverage));
        coverage = clamp((blend_l - bg_l) / (fg_l - bg_l), 0.0, 1.0);
    }
    return vec4<f32>(input.color.rgb, input.color.a * coverage);
}

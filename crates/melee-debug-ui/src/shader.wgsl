struct Globals {
    screen_size: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>, @location(2) color: vec4<f32>) -> VsOut {
    var out: VsOut;
    out.pos = vec4<f32>(
        2.0 * pos.x / globals.screen_size.x - 1.0,
        1.0 - 2.0 * pos.y / globals.screen_size.y,
        0.0,
        1.0,
    );
    out.uv = uv;
    out.color = color;
    return out;
}

fn linear_from_gamma(c: vec3<f32>) -> vec3<f32> {
    let lower = c / 12.92;
    let higher = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(higher, lower, c < vec3<f32>(0.04045));
}

@fragment
fn fs_gamma(in: VsOut) -> @location(0) vec4<f32> {
    return in.color * textureSample(tex, samp, in.uv);
}

@fragment
fn fs_linear(in: VsOut) -> @location(0) vec4<f32> {
    let c = in.color * textureSample(tex, samp, in.uv);
    return vec4<f32>(linear_from_gamma(c.rgb), c.a);
}

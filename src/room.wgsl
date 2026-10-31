// The room: one shadow-mapped spotlight at (or near) the viewer's head.
// Kept to features WebGL2 has: uniforms, a depth texture with a comparison sampler.

struct Globals {
    view_proj: mat4x4<f32>,
    light_view_proj: mat4x4<f32>,
    eye: vec4<f32>,
    // xyz position, w intensity.
    light: vec4<f32>,
    // x grid spacing, y ambient, z shadow-map texel size, w normal offset.
    params: vec4<f32>,
    // x half width, y half height, z depth, w light distance to the window plane.
    room: vec4<f32>,
    // xyz spotlight direction, w 1 when the light is a spotlight.
    spot: vec4<f32>,
    // x cos of the outer half-angle, y cos of the inner one.
    cone: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var shadow_map: texture_depth_2d;
@group(1) @binding(1) var shadow_sampler: sampler_comparison;

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
};

@vertex
fn vs_shadow(v: VsIn) -> @builtin(position) vec4<f32> {
    return g.light_view_proj * vec4<f32>(v.pos, 1.0);
}

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
};

@vertex
fn vs_main(v: VsIn) -> VsOut {
    var out: VsOut;
    out.clip = g.view_proj * vec4<f32>(v.pos, 1.0);
    out.world = v.pos;
    out.normal = v.normal;
    out.color = v.color;
    return out;
}

// 1 where the light reaches `world`, 0 in shadow; 3x3 PCF.
fn lit_fraction(world: vec3<f32>, n: vec3<f32>) -> f32 {
    let clip = g.light_view_proj * vec4<f32>(world + n * g.params.w, 1.0);
    if clip.w <= 0.0 {
        return 0.0;
    }
    let ndc = clip.xyz / clip.w;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    // Outside the shadow frustum nothing can occlude; the spot cone darkens it anyway.
    if uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 || ndc.z > 1.0 {
        return 1.0;
    }
    var sum = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let o = vec2<f32>(f32(x), f32(y)) * g.params.z;
            sum += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + o, ndc.z);
        }
    }
    return sum / 9.0;
}

// Thin dark lines every `spacing` meters, anti-aliased by screen-space derivatives.
fn grid(world: vec3<f32>, n: vec3<f32>, spacing: f32) -> f32 {
    let p = world / spacing;
    let w = fwidth(p) * 1.5;
    let d = abs(fract(p - 0.5) - 0.5);
    let l = smoothstep(vec3<f32>(0.0), w, d);
    // Ignore the axis along the normal: it is constant across the face.
    let an = abs(n);
    let lx = select(l.x, 1.0, an.x > 0.5);
    let ly = select(l.y, 1.0, an.y > 0.5);
    let lz = select(l.z, 1.0, an.z > 0.5);
    return min(min(lx, ly), lz);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let n = normalize(in.normal);
    let to_light = g.light.xyz - in.world;
    let dist = length(to_light);
    let l = to_light / dist;

    // Inverse-square falloff, normalized to 1 at the window plane so brightness does not
    // depend on how far the viewer sits; capped for pop-out objects close to the light.
    let d0 = g.room.w;
    let atten = min((d0 * d0) / (dist * dist), 1.5);
    let diffuse = max(dot(n, l), 0.0);

    let v = normalize(g.eye.xyz - in.world);
    let h = normalize(l + v);
    let spec = pow(max(dot(n, h), 0.0), 48.0) * 0.35 * step(0.5, 1.0 - in.color.a);

    let shadow = select(0.0, lit_fraction(in.world, n), diffuse > 0.0);
    // Derivatives need uniform control flow, so the grid is computed everywhere and masked.
    let lines = mix(0.72, 1.0, grid(in.world, n, g.params.x));
    let albedo = in.color.rgb * mix(1.0, lines, in.color.a);
    let cone = smoothstep(g.cone.x, g.cone.y, dot(-l, g.spot.xyz));
    let spot = mix(1.0, cone, g.spot.w);
    let direct = g.light.w * atten * shadow * spot;
    let rgb = albedo * (g.params.y + diffuse * direct) + vec3<f32>(spec * direct);
    return vec4<f32>(rgb, 1.0);
}

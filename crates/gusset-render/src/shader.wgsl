// Gusset viewport shaders. Downlevel-safe: no storage buffers, no compute, no
// `@interpolate(linear)`, only uniform buffers and vertex attributes (WebGL2 limits).
// The lighting in `shade()` mirrors `Lighting::shade` in theme.rs.

struct Globals {
    proj: mat4x4<f32>,
    bg_top: vec4<f32>,
    bg_bottom: vec4<f32>,
    // x, y: viewport size in px; z: half edge width in px; w: 1 = encode sRGB in shader
    viewport: vec4<f32>,
    key_dir: vec4<f32>,  // xyz direction, w strength
    fill_dir: vec4<f32>, // xyz direction, w strength
    light: vec4<f32>,    // sky, ground, specular, shininess
    // x: rim, y: 1 = orthographic, z: edge pull (perspective, fraction), w: edge pull (ortho)
    misc: vec4<f32>,
};

struct Draw {
    model_view: mat4x4<f32>,
    normal0: vec4<f32>,
    normal1: vec4<f32>,
    normal2: vec4<f32>,
    color: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var<uniform> d: Draw;

fn encode(c: vec3<f32>) -> vec4<f32> {
    let x = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    if (g.viewport.w > 0.5) {
        let lo = x * 12.92;
        let hi = 1.055 * pow(x, vec3<f32>(1.0 / 2.4)) - 0.055;
        return vec4<f32>(select(hi, lo, x <= vec3<f32>(0.0031308)), 1.0);
    }
    return vec4<f32>(x, 1.0);
}

// ---------------------------------------------------------------- background

struct BgOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) t: f32,
};

@vertex
fn vs_background(@builtin(vertex_index) i: u32) -> BgOut {
    // One oversized triangle covering the viewport.
    let x = f32(i32(i & 1u) * 4 - 1);
    let y = f32(i32(i >> 1u) * 4 - 1);
    var o: BgOut;
    o.pos = vec4<f32>(x, y, 1.0, 1.0);
    o.t = 0.5 - 0.5 * y; // 0 at the top edge, 1 at the bottom edge
    return o;
}

@fragment
fn fs_background(in: BgOut) -> @location(0) vec4<f32> {
    return encode(mix(g.bg_top.rgb, g.bg_bottom.rgb, clamp(in.t, 0.0, 1.0)));
}

// ---------------------------------------------------------------- shaded faces

struct MeshOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) view_pos: vec3<f32>,
    @location(1) view_normal: vec3<f32>,
};

@vertex
fn vs_mesh(@location(0) position: vec3<f32>, @location(1) normal: vec4<f32>) -> MeshOut {
    let vp = d.model_view * vec4<f32>(position, 1.0);
    let n = mat3x3<f32>(d.normal0.xyz, d.normal1.xyz, d.normal2.xyz) * normal.xyz;
    var o: MeshOut;
    o.pos = g.proj * vp;
    o.view_pos = vp.xyz;
    o.view_normal = n;
    return o;
}

fn shade(base: vec3<f32>, n: vec3<f32>, v: vec3<f32>) -> vec3<f32> {
    let hemi = 0.5 + 0.5 * n.y;
    let ambient = mix(g.light.y, g.light.x, hemi);
    let key = max(dot(n, g.key_dir.xyz), 0.0) * g.key_dir.w;
    let fill = max(dot(n, g.fill_dir.xyz), 0.0) * g.fill_dir.w;
    let h = normalize(g.key_dir.xyz + v);
    let spec = pow(max(dot(n, h), 0.0), g.light.w) * g.light.z;
    let rim = pow(1.0 - max(dot(n, v), 0.0), 3.0) * g.misc.x;
    return base * (ambient + key + fill) + vec3<f32>(spec + rim);
}

@fragment
fn fs_mesh(in: MeshOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    var n = normalize(in.view_normal);
    if (!front) {
        n = -n;
    }
    var v = vec3<f32>(0.0, 0.0, 1.0);
    if (g.misc.y < 0.5) {
        v = normalize(-in.view_pos);
    }
    return encode(shade(d.color.rgb, n, v));
}

// ---------------------------------------------------------------- edges

fn pull(vp: vec4<f32>) -> vec4<f32> {
    // Move edge vertices slightly towards the eye so they win the depth test against
    // the faces they bound (depth bias is not allowed on line pipelines).
    if (g.misc.y > 0.5) {
        return vec4<f32>(vp.xy, vp.z + g.misc.w, 1.0);
    }
    return vec4<f32>(vp.xyz * (1.0 - g.misc.z), 1.0);
}

@vertex
fn vs_edge_line(@location(0) position: vec3<f32>) -> @builtin(position) vec4<f32> {
    return g.proj * pull(d.model_view * vec4<f32>(position, 1.0));
}

@fragment
fn fs_edge() -> @location(0) vec4<f32> {
    return encode(d.color.rgb);
}

// Wide edges: one instance per segment, six vertices forming a screen-aligned quad.
@vertex
fn vs_edge_quad(@builtin(vertex_index) i: u32, @location(0) a: vec3<f32>, @location(1) b: vec3<f32>)
    -> @builtin(position) vec4<f32> {
    var ca = g.proj * pull(d.model_view * vec4<f32>(a, 1.0));
    var cb = g.proj * pull(d.model_view * vec4<f32>(b, 1.0));
    // Clip the segment against w = eps so both ends are in front of the eye.
    let eps = 1e-5;
    if (ca.w < eps && cb.w < eps) {
        return vec4<f32>(2.0, 2.0, 2.0, 1.0); // outside clip space: discarded
    }
    if (ca.w < eps) {
        ca = mix(ca, cb, (eps - ca.w) / (cb.w - ca.w));
    } else if (cb.w < eps) {
        cb = mix(cb, ca, (eps - cb.w) / (ca.w - cb.w));
    }
    let sa = ca.xy / ca.w * g.viewport.xy * 0.5;
    let sb = cb.xy / cb.w * g.viewport.xy * 0.5;
    var dir = sb - sa;
    let len = length(dir);
    if (len < 1e-6) {
        dir = vec2<f32>(1.0, 0.0);
    } else {
        dir = dir / len;
    }
    let nrm = vec2<f32>(-dir.y, dir.x);
    // Corner table: (end, side) for two triangles.
    var ends = array<f32, 6>(0.0, 1.0, 1.0, 0.0, 1.0, 0.0);
    var sides = array<f32, 6>(-1.0, -1.0, 1.0, -1.0, 1.0, 1.0);
    let e = ends[i];
    let s = sides[i];
    let hw = g.viewport.z;
    var c = select(ca, cb, e > 0.5);
    let along = select(-dir, dir, e > 0.5) * hw; // extend caps by half a width
    let off_px = nrm * s * hw + along;
    let off = off_px / (g.viewport.xy * 0.5) * c.w;
    return vec4<f32>(c.xy + off, c.z, c.w);
}

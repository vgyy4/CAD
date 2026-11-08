// Gusset viewport shaders. Downlevel-safe: no storage buffers, no compute, no
// `@interpolate(linear)`, only uniform buffers and vertex attributes (WebGL2 limits).
// The lighting in `shade()` mirrors `Lighting::shade_finish` in theme.rs.

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
    // Section plane in view space: n.p + w >= 0 is kept; xyz = 0 when there is no section.
    clip: vec4<f32>,
    cap: vec4<f32>, // rgb: cut face tint (linear)
};

const CAP_MIX: f32 = 0.6; // cut face tint amount (software.rs CAP_MIX)

fn sectioned() -> bool {
    return dot(g.clip.xyz, g.clip.xyz) > 0.5;
}

fn removed(view_pos: vec3<f32>) -> bool {
    return dot(g.clip.xyz, view_pos) + g.clip.w < 0.0;
}

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

// `d.color.w` = closed (1) + 2 * finish (0 plastic, 1 metal, 2 glass, 3 rubber).
fn closed_mesh() -> bool {
    return (u32(d.color.w + 0.5) & 1u) == 1u;
}

fn finish_code() -> u32 {
    return u32(d.color.w + 0.5) >> 1u;
}

// Mirrors `Lighting::shade_finish` in theme.rs.
fn shade(base_in: vec3<f32>, n: vec3<f32>, v: vec3<f32>, finish: u32) -> vec3<f32> {
    var dscale = 1.0;
    var sscale = 1.0;
    var sharp = 1.0;
    var lighten = 0.0;
    var rscale = 1.0;
    var tint = false;
    if (finish == 1u) {
        dscale = 0.45; sscale = 2.2; sharp = 3.0; tint = true;
    } else if (finish == 2u) {
        dscale = 0.7; sscale = 2.0; sharp = 4.0; lighten = 0.35; rscale = 2.0;
    } else if (finish == 3u) {
        sscale = 0.0; rscale = 0.3;
    }
    let base = base_in + (vec3<f32>(1.0) - base_in) * lighten;
    let hemi = 0.5 + 0.5 * n.y;
    let ambient = mix(g.light.y, g.light.x, hemi);
    let key = max(dot(n, g.key_dir.xyz), 0.0) * g.key_dir.w;
    let fill = max(dot(n, g.fill_dir.xyz), 0.0) * g.fill_dir.w;
    let h = normalize(g.key_dir.xyz + v);
    let spec = pow(max(dot(n, h), 0.0), g.light.w * sharp) * g.light.z * sscale;
    let rim = pow(1.0 - max(dot(n, v), 0.0), 3.0) * g.misc.x * rscale;
    var extra = vec3<f32>(spec + rim);
    var env = 0.0;
    if (tint) {
        extra = base * (spec + rim);
        // Metal mirrors the sky above and the ground below (the reflected view direction).
        let ry = 2.0 * dot(n, v) * n.y - v.y;
        env = mix(g.light.y, g.light.x * 1.4, clamp(0.5 + 0.5 * ry, 0.0, 1.0)) * 0.9;
    }
    return base * ((ambient + key + fill) * dscale + env) + extra;
}

@fragment
fn fs_mesh(in: MeshOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    if (removed(in.view_pos)) {
        discard;
    }
    // Through a section, a closed body's back faces are its cut: the cap pass draws them.
    if (!front && sectioned() && closed_mesh()) {
        discard;
    }
    var n = normalize(in.view_normal);
    if (!front) {
        n = -n;
    }
    return encode(shade(d.color.rgb, n, view_dir(in.view_pos), finish_code()));
}

fn view_dir(view_pos: vec3<f32>) -> vec3<f32> {
    if (g.misc.y < 0.5) {
        return normalize(-view_pos);
    }
    return vec3<f32>(0.0, 0.0, 1.0);
}

struct CapFrag {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

// Where the view ray through `view_pos` meets the section plane (view space); `w` = 0
// when the plane is seen edge-on.
fn on_section(view_pos: vec3<f32>) -> vec4<f32> {
    if (g.misc.y > 0.5) {
        // Orthographic: the ray runs along -z through (x, y).
        if (abs(g.clip.z) < 1e-6) {
            return vec4<f32>(0.0);
        }
        let z = -(g.clip.x * view_pos.x + g.clip.y * view_pos.y + g.clip.w) / g.clip.z;
        return vec4<f32>(view_pos.xy, z, 1.0);
    }
    let k = dot(g.clip.xyz, view_pos);
    if (abs(k) < 1e-9) {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(view_pos * (-g.clip.w / k), 1.0);
}

// The cut face of a closed body: its back faces seen through the section, drawn flat in the
// cap tint at the depth of the cut plane (not of the far wall, so what lies behind the cut
// stays hidden). Mirrors `raster_triangle`'s cap branch in software.rs.
@fragment
fn fs_cap(in: MeshOut, @builtin(front_facing) front: bool) -> CapFrag {
    if (front || !sectioned() || !closed_mesh() || removed(in.view_pos)) {
        discard;
    }
    var o: CapFrag;
    o.color = encode(shade(mix(d.color.rgb, g.cap.rgb, CAP_MIX), -g.clip.xyz, view_dir(in.view_pos), 0u));
    o.depth = in.pos.z;
    let p = on_section(in.view_pos);
    if (p.w > 0.5) {
        let c = g.proj * vec4<f32>(p.xyz, 1.0);
        o.depth = clamp(c.z / c.w, 0.0, 1.0);
    }
    return o;
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

struct EdgeOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) view_pos: vec3<f32>,
};

@vertex
fn vs_edge_line(@location(0) position: vec3<f32>) -> EdgeOut {
    let vp = d.model_view * vec4<f32>(position, 1.0);
    var o: EdgeOut;
    o.pos = g.proj * pull(vp);
    o.view_pos = vp.xyz;
    return o;
}

@fragment
fn fs_edge(in: EdgeOut) -> @location(0) vec4<f32> {
    if (removed(in.view_pos)) {
        discard;
    }
    return encode(d.color.rgb);
}

// Wide edges: one instance per segment, six vertices forming a screen-aligned quad.
@vertex
fn vs_edge_quad(@builtin(vertex_index) i: u32, @location(0) a: vec3<f32>, @location(1) b: vec3<f32>)
    -> EdgeOut {
    let va = d.model_view * vec4<f32>(a, 1.0);
    let vb = d.model_view * vec4<f32>(b, 1.0);
    var ca = g.proj * pull(va);
    var cb = g.proj * pull(vb);
    var o: EdgeOut;
    // Clip the segment against w = eps so both ends are in front of the eye.
    let eps = 1e-5;
    if (ca.w < eps && cb.w < eps) {
        o.pos = vec4<f32>(2.0, 2.0, 2.0, 1.0); // outside clip space: discarded
        o.view_pos = va.xyz;
        return o;
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
    o.pos = vec4<f32>(c.xy + off, c.z, c.w);
    o.view_pos = select(va.xyz, vb.xyz, e > 0.5);
    return o;
}

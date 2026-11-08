//! A small CPU path tracer for still images ("rendering", DESIGN.md later tier): diffuse
//! materials in the parts' colours on a ground plane, lit by a sky and a soft sun, with
//! shadows and colour bleeding. Deterministic: the noise depends only on the pixel and the
//! sample number, so the same scene and settings give the same image.
//!
//! Rays are cast through [`Scene::pick`] (the picking BVH), so whatever the viewport shows is
//! what is traced.

use crate::camera::{Camera, Ray};
use crate::headless::RenderError;
use crate::image::RgbaImage;
use crate::math::{Vec3, add, cross, dot, mat_vec, normalize_or, scale, sub};
use crate::scene::{Finish, Scene};
use crate::theme::{linear_to_srgb, srgb_to_linear};

/// How long to trace.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TraceSettings {
    /// Paths per pixel, 1..=1024.
    pub samples: u32,
    /// Bounces after the first hit, 0..=6.
    pub bounces: u32,
}

impl Default for TraceSettings {
    fn default() -> Self {
        TraceSettings { samples: 64, bounces: 3 }
    }
}

/// Largest side of a traced image, and the most paths one image may trace.
pub const TRACE_MAX_SIDE: u32 = 4096;
const MAX_PATHS: u64 = 400_000_000;

/// Direction towards the sun (unit), its irradiance and its angular radius (soft shadows).
const SUN: Vec3 = [0.408_248_29, 0.408_248_29, 0.816_496_58];
const SUN_POWER: f64 = 2.2;
const SUN_SPREAD: f64 = 0.05;
const GROUND_ALBEDO: f64 = 0.7;
/// Clear coat: reflectance at normal incidence (a lacquer's 4 %).
const COAT_F0: f64 = 0.04;
/// Metal roughness and glass's index of refraction.
const METAL_ROUGH: f64 = 0.12;
const GLASS_IOR: f64 = 1.5;
/// The sun's disc as specular paths see it: within this angle of [`SUN`], with the radiance
/// that gives [`SUN_POWER`] over the disc.
const SUN_DISC: f64 = 0.028;
/// Most radiance one path adds to its pixel (removes fireflies, darkens the sun's mirror
/// image a little).
const MAX_PATH_RADIANCE: f64 = 12.0;

/// A little xorshift generator, seeded per pixel and sample.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03);
        r.next();
        r
    }
    fn next(&mut self) -> f64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// An orthonormal basis around `n`.
fn basis(n: Vec3) -> (Vec3, Vec3) {
    let a = if n[0].abs() > 0.9 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
    let t = normalize_or(cross(a, n), [0.0, 0.0, 1.0]);
    (t, cross(n, t))
}

/// A cosine-weighted direction about `n`.
fn cosine_dir(n: Vec3, rng: &mut Rng) -> Vec3 {
    let (u, v) = (rng.next(), rng.next());
    let (r, phi) = (u.sqrt(), 2.0 * std::f64::consts::PI * v);
    let (t, b) = basis(n);
    let z = (1.0 - u).max(0.0).sqrt();
    normalize_or(add(add(scale(t, r * phi.cos()), scale(b, r * phi.sin())), scale(n, z)), n)
}

/// Sky radiance seen along `d`: blue above, white at the horizon, grey below.
fn sky(d: Vec3) -> [f64; 3] {
    let up = d[2].clamp(-1.0, 1.0);
    if up < 0.0 {
        return [0.35, 0.35, 0.36];
    }
    let t = up.powf(0.5);
    [1.0 - 0.45 * t, 1.0 - 0.3 * t, 1.0]
}

/// What a ray meets: the hit point, the unit normal facing the ray, the albedo (linear).
struct Hit {
    point: Vec3,
    normal: Vec3,
    albedo: [f64; 3],
    /// Parts have a clear coat; the ground does not.
    finish: Finish,
    /// The ray met the outside of the surface (the mesh's outward side).
    front: bool,
    /// The mesh is open (a sheet): glass on it is a thin pane, not a solid that bends light.
    thin: bool,
}

struct World<'a> {
    scene: &'a Scene,
    ground: f64,
    eps: f64,
}

impl World<'_> {
    fn trace(&self, ray: &Ray) -> Option<Hit> {
        let part = self.scene.pick(ray).and_then(|h| {
            let m = self.scene.mesh(h.mesh)?;
            let d = m.data();
            let i = 3 * h.triangle as usize;
            let tri = d.indices.get(i..i + 3)?;
            let world = |k: u32| {
                let p = d.positions.get(k as usize).copied().unwrap_or([0.0; 3]);
                let w = mat_vec(m.transform(), [f64::from(p[0]), f64::from(p[1]), f64::from(p[2]), 1.0]);
                [w[0], w[1], w[2]]
            };
            let ids = [*tri.first()?, *tri.get(1)?, *tri.get(2)?];
            let [a, b, c] = ids.map(world);
            let geo = normalize_or(cross(sub(b, a), sub(c, a)), [0.0, 0.0, 1.0]);
            // Smooth shading: the vertex normals interpolated at the hit (barycentric), on the
            // geometric normal's side.
            let (e1, e2, ep) = (sub(b, a), sub(c, a), sub(h.point, a));
            let (d00, d01, d11, d20, d21) = (dot(e1, e1), dot(e1, e2), dot(e2, e2), dot(ep, e1), dot(ep, e2));
            let den = d00 * d11 - d01 * d01;
            let normal_of = |k: u32| {
                let q = crate::mesh::unpack_normal(d.normals.get(k as usize).copied().unwrap_or([0; 4]));
                let w = mat_vec(m.transform(), [f64::from(q[0]), f64::from(q[1]), f64::from(q[2]), 0.0]);
                [w[0], w[1], w[2]]
            };
            let den_ok = (den.abs() > 1e-30).then(|| ((d11 * d20 - d01 * d21) / den, (d00 * d21 - d01 * d20) / den));
            let smooth = den_ok.map(|(v, w)| {
                let [na, nb, nc] = ids.map(normal_of);
                add(add(scale(na, 1.0 - v - w), scale(nb, v)), scale(nc, w))
            });
            let mut n = smooth.map_or(geo, |sn| normalize_or(sn, geo));
            if dot(n, geo) < 0.0 {
                n = scale(n, -1.0);
            }
            let front = dot(geo, ray.dir) <= 0.0;
            if !front {
                n = scale(n, -1.0);
            }
            // Highlighted faces in the viewport's selection colours (dogfooding F12-10).
            let face = d.face_ranges.iter().position(|r| r.contains(&h.triangle));
            let color = match face.map(|f| m.face_highlight(f as u32)) {
                Some(crate::Highlight::Select) => crate::Theme::Light.colors().select,
                Some(crate::Highlight::Preselect) => crate::Theme::Light.colors().preselect,
                _ => face.map_or(m.material().color, |f| m.face_color(f as u32)),
            };
            let base = m.material().color;
            let mut albedo = color.bytes().map(|v| f64::from(srgb_to_linear(f32::from(v) / 255.0)));
            // A texture, in the body's own coordinates at the hit (not on highlighted faces).
            if let (Some(t), Some(den)) = (m.material().texture.filter(|_| color == base), den_ok) {
                let model = |k: u32| {
                    let p = d.positions.get(k as usize).copied().unwrap_or([0.0; 3]);
                    [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])]
                };
                let [ma, mb, mc] = ids.map(model);
                let (v, w) = den;
                let p = add(add(scale(ma, 1.0 - v - w), scale(mb, v)), scale(mc, w));
                let mn = normalize_or(cross(sub(mb, ma), sub(mc, ma)), [0.0, 0.0, 1.0]);
                for (a, f) in albedo.iter_mut().zip(t.factor(p, mn)) {
                    *a = (*a * f).min(0.98);
                }
            }
            let [r, g, bl] = albedo;
            Some((
                h.distance,
                Hit {
                    point: h.point,
                    normal: n,
                    albedo: [r, g, bl],
                    finish: m.material().finish,
                    front,
                    thin: !d.closed,
                },
            ))
        });
        // The ground plane under the parts.
        let ground = (ray.dir[2] < -1e-9).then(|| (self.ground - ray.origin[2]) / ray.dir[2]).filter(|t| *t > self.eps);
        match (part, ground) {
            (Some((t, h)), Some(g)) if t <= g => Some(h),
            (Some((_, h)), None) => Some(h),
            (_, Some(g)) => Some(Hit {
                point: add(ray.origin, scale(ray.dir, g)),
                normal: [0.0, 0.0, 1.0],
                albedo: [GROUND_ALBEDO; 3],
                finish: Finish::Rubber,
                front: true,
                thin: false,
            }),
            (None, None) => None,
        }
    }

    /// Whether the sun is seen from `p` (a direction jittered over the sun's disc).
    /// The sun's light reaching `p`: `cos` times what glass on the way lets through (a
    /// simplification: glass dims and tints the light but does not bend it).
    fn sun_seen(&self, p: Vec3, n: Vec3, rng: &mut Rng) -> Option<[f64; 3]> {
        let (t, b) = basis(SUN);
        let (u, v) = (rng.next() - 0.5, rng.next() - 0.5);
        let l = normalize_or(add(SUN, add(scale(t, u * SUN_SPREAD), scale(b, v * SUN_SPREAD))), SUN);
        let cos = dot(n, l);
        if cos <= 0.0 {
            return None;
        }
        let mut light = [cos; 3];
        let mut origin = add(p, scale(n, self.eps));
        for _ in 0..16 {
            let Some(h) = self.scene.pick(&Ray { origin, dir: l }) else { return Some(light) };
            let m = self.scene.mesh(h.mesh)?.material();
            if m.finish != Finish::Glass {
                return None;
            }
            for (x, c) in light.iter_mut().zip(m.color.bytes()) {
                // A fourth root per surface: a pass through a solid (two surfaces) tints by the
                // square root, as camera paths do (review R13); 4 % reflected at each.
                *x *= f64::from(srgb_to_linear(f32::from(c) / 255.0)).sqrt().sqrt() * 0.96;
            }
            origin = add(origin, scale(l, h.distance + self.eps));
        }
        None
    }

    fn radiance(&self, mut ray: Ray, bounces: u32, rng: &mut Rng) -> [f64; 3] {
        let mut out = [0.0; 3];
        let mut through = [1.0; 3];
        // The camera ray and mirror-like bounces see the sun's disc; diffuse bounces get the
        // sun from `sun_seen` instead, so it is not counted twice.
        let mut specular = true;
        // Passes through glass do not use up bounces (review R5), up to this many.
        let mut glass_free = 12u32;
        let mut level = 0u32;
        while level <= bounces {
            let depth = level;
            level += 1;
            let Some(h) = self.trace(&ray) else {
                let disc = if specular && dot(ray.dir, SUN) > SUN_DISC.cos() {
                    SUN_POWER / (std::f64::consts::PI * SUN_DISC * SUN_DISC)
                } else {
                    0.0
                };
                for ((o, t), s) in out.iter_mut().zip(through).zip(sky(ray.dir)) {
                    *o += t * (s + disc);
                }
                break;
            };
            let cos_i = (-dot(ray.dir, h.normal)).clamp(0.0, 1.0);
            let off = |p: Vec3, n: Vec3, d: f64| add(p, scale(n, d));
            match h.finish {
                Finish::Metal => {
                    // Schlick's Fresnel with the colour as reflectance at normal incidence.
                    let k = (1.0 - cos_i).powi(5);
                    for (t, a) in through.iter_mut().zip(h.albedo) {
                        *t *= a + (1.0 - a) * k;
                    }
                    if depth == bounces || through.iter().all(|t| *t < 1e-3) {
                        break;
                    }
                    ray = Ray {
                        origin: off(h.point, h.normal, self.eps),
                        dir: glossy(ray.dir, h.normal, METAL_ROUGH, rng),
                    };
                    specular = true;
                    continue;
                }
                Finish::Glass => {
                    if glass_free > 0 {
                        glass_free -= 1;
                        level -= 1;
                    } else if depth == bounces {
                        break;
                    }
                    let eta = if h.front && !h.thin { 1.0 / GLASS_IOR } else { GLASS_IOR };
                    let f0 = ((1.0 - GLASS_IOR) / (1.0 + GLASS_IOR)).powi(2);
                    let sin2 = eta * eta * (1.0 - cos_i * cos_i);
                    let cos_t = (1.0 - sin2).max(0.0).sqrt();
                    // Schlick with the angle on the thinner side: the transmitted one when
                    // leaving the glass (review R6).
                    let c = if h.front || h.thin { cos_i } else { cos_t };
                    let fresnel = f0 + (1.0 - f0) * (1.0 - c).powi(5);
                    let mirror = sub(ray.dir, scale(h.normal, 2.0 * dot(ray.dir, h.normal)));
                    ray = if (!h.thin && sin2 > 1.0) || rng.next() < fresnel {
                        Ray { origin: off(h.point, h.normal, self.eps), dir: mirror }
                    } else if h.thin {
                        // A pane: straight through, tinted once.
                        for (t, a) in through.iter_mut().zip(h.albedo) {
                            *t *= a.sqrt();
                        }
                        Ray { origin: off(h.point, h.normal, -self.eps), dir: ray.dir }
                    } else {
                        // Refracted: through the surface, tinted by the colour once per pass.
                        let dir = normalize_or(add(scale(ray.dir, eta), scale(h.normal, eta * cos_i - cos_t)), ray.dir);
                        if h.front {
                            for (t, a) in through.iter_mut().zip(h.albedo) {
                                *t *= a.sqrt();
                            }
                        }
                        Ray { origin: off(h.point, h.normal, -self.eps), dir }
                    };
                    specular = true;
                    continue;
                }
                Finish::Plastic => {
                    // A thin clear coat, added rather than sampled (dogfooding F14-8: random
                    // coat bounces left grain): its Fresnel share of the sky seen along the
                    // mirror direction, unless the model is in the way; the base below gets
                    // the rest.
                    let fresnel = COAT_F0 + (1.0 - COAT_F0) * (1.0 - cos_i).powi(5);
                    let mirror = sub(ray.dir, scale(h.normal, 2.0 * dot(ray.dir, h.normal)));
                    let probe = Ray { origin: off(h.point, h.normal, self.eps), dir: mirror };
                    if self.scene.pick(&probe).is_none() {
                        let disc = if dot(mirror, SUN) > SUN_DISC.cos() {
                            SUN_POWER / (std::f64::consts::PI * SUN_DISC * SUN_DISC)
                        } else {
                            0.0
                        };
                        for ((o, t), s) in out.iter_mut().zip(through).zip(sky(mirror)) {
                            *o += t * fresnel * (s + disc);
                        }
                    }
                    for t in &mut through {
                        *t *= 1.0 - fresnel;
                    }
                }
                Finish::Rubber => {}
            }
            for (t, a) in through.iter_mut().zip(h.albedo) {
                *t *= a;
            }
            if let Some(light) = self.sun_seen(h.point, h.normal, rng) {
                for ((o, t), l) in out.iter_mut().zip(through).zip(light) {
                    *o += t * SUN_POWER * l;
                }
            }
            if depth == bounces || through.iter().all(|t| *t < 1e-3) {
                break;
            }
            ray = Ray { origin: off(h.point, h.normal, self.eps), dir: cosine_dir(h.normal, rng) };
            specular = false;
        }
        out
    }
}

/// The mirror direction of `dir` about `n`, jittered by `rough`, kept above the surface.
fn glossy(dir: Vec3, n: Vec3, rough: f64, rng: &mut Rng) -> Vec3 {
    let mirror = sub(dir, scale(n, 2.0 * dot(dir, n)));
    let (t, b) = basis(mirror);
    let (u, v) = (rng.next() - 0.5, rng.next() - 0.5);
    let d = normalize_or(add(mirror, add(scale(t, u * rough), scale(b, v * rough))), mirror);
    if dot(d, n) <= 0.0 { mirror } else { d }
}

/// Traces `scene` seen by `camera` into an image of `size` pixels.
///
/// # Errors
/// A size of zero or above [`TRACE_MAX_SIDE`], settings out of range, or too much work.
pub fn render_traced(
    scene: &Scene,
    camera: &Camera,
    size: [u32; 2],
    settings: TraceSettings,
) -> Result<RgbaImage, RenderError> {
    let [w, h] = size;
    if w == 0 || h == 0 || w > TRACE_MAX_SIDE || h > TRACE_MAX_SIDE {
        return Err(RenderError::InvalidSize { size, max: TRACE_MAX_SIDE });
    }
    if !(1..=1024).contains(&settings.samples) || settings.bounces > 6 {
        return Err(RenderError::Settings("samples must be 1..=1024 and bounces 0..=6".into()));
    }
    if u64::from(w) * u64::from(h) * u64::from(settings.samples) > MAX_PATHS {
        return Err(RenderError::Gpu("too much work: fewer samples or a smaller image".into()));
    }
    let b = scene.bounds();
    let extent = b.max.iter().zip(b.min).map(|(hi, lo)| hi - lo).fold(0.0, f64::max).max(1e-6);
    let world = World { scene, ground: b.min[2], eps: extent * 1e-4 };
    let viewport = [f64::from(w), f64::from(h)];
    let threads = std::thread::available_parallelism().map_or(1, std::num::NonZero::get).min(64);
    let mut img = RgbaImage::new(w, h, [0, 0, 0, 255]);
    let row = 4 * w as usize;
    let rows_per = (h as usize).div_ceil(threads).max(1);
    std::thread::scope(|sc| {
        for (chunk_no, chunk) in img.data.chunks_mut(row * rows_per).enumerate() {
            let world = &world;
            sc.spawn(move || {
                for (r, line) in chunk.chunks_mut(row).enumerate() {
                    let y = (chunk_no * rows_per + r) as u32;
                    for (x, px) in line.chunks_exact_mut(4).enumerate() {
                        let mut sum = [0.0; 3];
                        for s in 0..settings.samples {
                            let seed = (u64::from(y) << 40) ^ ((x as u64) << 20) ^ u64::from(s);
                            let mut rng = Rng::new(seed);
                            let p = [x as f64 + rng.next(), f64::from(y) + rng.next()];
                            let l = world.radiance(camera.ray(p, viewport), settings.bounces, &mut rng);
                            // A rare path that finds the sun through a mirror or glass would
                            // leave a bright speck: each path counts at most MAX_PATH_RADIANCE.
                            for (a, v) in sum.iter_mut().zip(l) {
                                *a += v.min(MAX_PATH_RADIANCE);
                            }
                        }
                        for (c, v) in sum.iter().enumerate() {
                            // Exposure, then a soft shoulder before sRGB.
                            let lin = v / f64::from(settings.samples) * 0.75;
                            let mapped = (lin / (1.0 + lin * 0.25)) as f32;
                            if let Some(o) = px.get_mut(c) {
                                *o = (linear_to_srgb(mapped) * 255.0).round() as u8;
                            }
                        }
                    }
                }
            });
        }
    });
    Ok(img)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;
    use crate::{MeshId, NamedView, demo};

    fn scene() -> (Scene, Camera) {
        let mut scene = Scene::new();
        scene.insert_mesh(MeshId(1), &demo::box_mesh([10.0, 10.0, 10.0])).unwrap();
        let mut camera = Camera::default();
        camera.set_view(NamedView::Iso);
        camera.fit(&scene.bounds(), 4.0 / 3.0);
        (scene, camera)
    }

    #[test]
    fn a_box_on_the_ground_is_lit_shadowed_and_deterministic() {
        let (s, c) = scene();
        let set = TraceSettings { samples: 4, bounces: 2 };
        let img = render_traced(&s, &c, [64, 48], set).unwrap();
        assert_eq!(img, render_traced(&s, &c, [64, 48], set).unwrap(), "same input, same image");
        let lum = |x: u32, y: u32| {
            let i = 4 * (y * 64 + x) as usize;
            img.data[i..i + 3].iter().map(|v| u32::from(*v)).sum::<u32>()
        };
        // The picture is not flat: lit faces, shadow and sky differ.
        let all: Vec<u32> = (0..48).flat_map(|y| (0..64).map(move |x| (x, y))).map(|(x, y)| lum(x, y)).collect();
        let (lo, hi) = (all.iter().min().unwrap(), all.iter().max().unwrap());
        assert!(hi - lo > 150, "{lo}..{hi}");
        assert!(img.to_png().unwrap().starts_with(b"\x89PNG"));
        // A look at a bigger picture on request (not in CI).
        if let Ok(out) = std::env::var("GUSSET_TRACE_OUT") {
            let big = render_traced(&s, &c, [480, 360], TraceSettings { samples: 64, bounces: 3 }).unwrap();
            std::fs::write(out, big.to_png().unwrap()).unwrap();
        }
    }

    #[test]
    fn finishes_change_the_picture_and_glass_shows_what_is_behind() {
        let set = TraceSettings { samples: 8, bounces: 4 };
        let shot = |finish: Finish| {
            let (mut s, c) = scene();
            s.set_material(MeshId(1), crate::Material { color: crate::Srgb(0xC0_40_30), finish, texture: None });
            render_traced(&s, &c, [48, 36], set).unwrap()
        };
        let all = Finish::ALL_FOR_TESTS.map(shot);
        for (i, a) in all.iter().enumerate() {
            for b in all.get(i + 1..).unwrap() {
                assert_ne!(a, b, "two finishes render alike");
            }
        }
        // Through the glass box the light ground shows: its centre is brighter and less red
        // than the rubber box's.
        let centre = |img: &RgbaImage| {
            let i = 4 * (18 * 48 + 24) as usize;
            [img.data[i], img.data[i + 1], img.data[i + 2]].map(i32::from)
        };
        let (rubber, glass) = (centre(&all[3]), centre(&all[2]));
        assert!(glass[1] > rubber[1], "glass {glass:?} rubber {rubber:?}");
        if let Ok(dir) = std::env::var("GUSSET_TRACE_OUT_DIR") {
            for (f, name) in Finish::ALL_FOR_TESTS.into_iter().zip(["plastic", "metal", "glass", "rubber"]) {
                let (mut s, c) = scene();
                s.set_material(MeshId(1), crate::Material { color: crate::Srgb(0xC0_40_30), finish: f, texture: None });
                let big = render_traced(&s, &c, [320, 240], TraceSettings { samples: 64, bounces: 5 }).unwrap();
                std::fs::write(format!("{dir}/{name}.png"), big.to_png().unwrap()).unwrap();
            }
        }
    }

    #[test]
    fn bad_settings_are_refused() {
        let (s, c) = scene();
        assert!(render_traced(&s, &c, [0, 10], TraceSettings::default()).is_err());
        assert!(render_traced(&s, &c, [10, 10], TraceSettings { samples: 0, bounces: 1 }).is_err());
        assert!(render_traced(&s, &c, [10, 10], TraceSettings { samples: 4, bounces: 9 }).is_err());
        assert!(render_traced(&s, &c, [4096, 4096], TraceSettings { samples: 1024, bounces: 1 }).is_err());
    }
}

//! Frame-time measurements for docs/PERFORMANCE.md (spec: 60 fps orbit for typical parts,
//! 30 fps for large assemblies, 15 fps under the software renderer). Opt-in:
//! `cargo test --release -p gusset-render --test perf -- --ignored --nocapture`.

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::print_stdout)]

use gusset_render::{Camera, HeadlessRenderer, MeshId, NamedView, Scene, SoftwareRenderer, demo};
use std::time::Instant;

/// A typical part: a cylinder (2k triangles) and a box.
fn part() -> Scene {
    let mut s = Scene::new();
    s.insert_mesh(MeshId(1), &demo::cylinder_mesh(20.0, 40.0, 512)).unwrap();
    s.insert_mesh(MeshId(2), &demo::box_mesh([60.0, 40.0, 8.0])).unwrap();
    s
}

/// A large assembly: 400 bodies, 1.6 M triangles.
fn assembly() -> Scene {
    let mut s = Scene::new();
    for i in 0..400u64 {
        let mut m = demo::cylinder_mesh(3.0, 8.0, 1000);
        let (x, y) = ((i % 20) as f32 * 10.0, (i / 20) as f32 * 10.0);
        for p in &mut m.positions {
            p[0] += x;
            p[1] += y;
        }
        s.insert_mesh(MeshId(i), &m).unwrap();
    }
    s
}

/// Mean milliseconds per frame over an orbit of `n` frames.
fn orbit(n: u32, mut frame: impl FnMut(&Camera)) -> f64 {
    let mut cam = Camera::default();
    cam.set_view(NamedView::Iso);
    frame(&cam); // warm-up (uploads)
    let t = Instant::now();
    for _ in 0..n {
        cam.orbit(0.05, 0.0);
        frame(&cam);
    }
    t.elapsed().as_secs_f64() * 1000.0 / f64::from(n)
}

#[test]
#[ignore = "measurement, run on demand"]
fn frame_times() {
    let size = [1280, 800];
    for (name, scene) in [("typical part", part()), ("large assembly (400 bodies)", assembly())] {
        let tris: usize = scene.meshes().map(|(_, m)| m.data().triangle_count()).sum();
        let mut fit = Camera::default();
        fit.fit(&scene.bounds(), 1.6);
        let sw = SoftwareRenderer::new();
        let soft = orbit(10, |c| {
            let mut c = c.clone();
            c.fit(&scene.bounds(), 1.6);
            sw.render_rgba(&scene, &c, size).unwrap();
        });
        println!("{name}: {tris} triangles; software {soft:.1} ms/frame ({:.1} fps)", 1000.0 / soft);
        match HeadlessRenderer::new() {
            Ok(mut gpu) => {
                let ms = orbit(30, |c| {
                    let mut c = c.clone();
                    c.fit(&scene.bounds(), 1.6);
                    gpu.render_rgba(&scene, &c, size).unwrap();
                });
                println!(
                    "{name}: GPU ({}) {ms:.1} ms/frame incl. readback ({:.1} fps)",
                    gpu.adapter().name,
                    1000.0 / ms
                );
            }
            Err(e) => println!("{name}: no GPU adapter ({e})"),
        }
    }
}

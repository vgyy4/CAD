//! Render the demo scene to a PNG: `cargo run -p gusset-render --example render_view --
//! out.png [width height] [dark|light] [view] [ppp] [software]`.

use gusset_render::math::translation;
use gusset_render::{
    Camera, Highlight, Material, MeshId, NamedView, Scene, SoftwareRenderer, Srgb, Theme, ViewOptions, ViewRenderer,
    demo,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize| args.get(i).map(String::as_str);
    let out = arg(0).unwrap_or("render_view.png");
    let w: u32 = arg(1).and_then(|s| s.parse().ok()).unwrap_or(640);
    let h: u32 = arg(2).and_then(|s| s.parse().ok()).unwrap_or(480);
    let theme = if arg(3) == Some("light") { Theme::Light } else { Theme::Dark };
    let view = arg(4).and_then(NamedView::parse).unwrap_or(NamedView::Iso);
    let ppp: f32 = arg(5).and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let software = arg(6) == Some("software");

    let mut scene = Scene::new();
    scene.insert_mesh(MeshId(1), &demo::box_mesh([20.0, 20.0, 20.0]))?;
    scene.set_transform(MeshId(1), translation([-14.0, 0.0, 10.0]));
    scene.insert_mesh(MeshId(2), &demo::cylinder_mesh(9.0, 24.0, 96))?;
    scene.set_transform(MeshId(2), translation([14.0, 0.0, 0.0]));
    scene.set_material(MeshId(2), Material { color: Srgb(0xC9B79C) });
    scene.set_face_highlight(MeshId(1), 1, Highlight::Select);
    scene.set_face_highlight(MeshId(2), 0, Highlight::Preselect);

    let mut camera = Camera::default();
    camera.set_view(view);
    camera.fit(&scene.bounds(), f64::from(w) / f64::from(h));
    let options = ViewOptions { theme, pixels_per_point: ppp, ..ViewOptions::default() };

    let png = if software {
        let mut r = SoftwareRenderer::new();
        r.set_options(options);
        r.render_png(&scene, &camera, [w, h])?
    } else {
        let mut r = ViewRenderer::new();
        eprintln!("{}", r.describe());
        r.set_options(options);
        r.render_png(&scene, &camera, [w, h])?
    };
    std::fs::write(out, png)?;
    Ok(())
}

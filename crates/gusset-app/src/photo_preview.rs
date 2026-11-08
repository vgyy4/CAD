//! The Render workspace's photo preview: the viewport path-traced in place at half
//! resolution, in passes of more and more samples while the view stays still. A camera, model
//! or size change abandons the passes and starts over; nothing runs once the last pass is
//! in (spec: idle means idle).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, channel};

use gusset_render::{Camera, RgbaImage, Scene};

/// Samples a pixel of each pass.
const PASSES: [u32; 4] = [1, 4, 16, 64];

/// A preview pass: its generation and the picture with its samples, or why it failed.
type Pass = (u64, Result<(RgbaImage, u32), String>);

#[derive(Default)]
pub struct PhotoPreview {
    /// Shown (the Render tab's toggle).
    pub on: bool,
    generation: Arc<AtomicU64>,
    key: Option<String>,
    rx: Option<Receiver<Pass>>,
    texture: Option<egui::TextureHandle>,
    /// Samples a pixel of the picture shown.
    pub samples: u32,
    /// Why the last pass failed (shown over the viewport).
    pub error: Option<String>,
}

impl std::fmt::Debug for PhotoPreview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PhotoPreview").field("on", &self.on).field("samples", &self.samples).finish()
    }
}

impl PhotoPreview {
    /// Turns the preview on or off (off stops the passes).
    pub fn set(&mut self, on: bool) {
        self.on = on;
        if !on {
            self.stop();
        }
    }

    fn stop(&mut self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        (self.key, self.rx, self.texture, self.samples, self.error) = (None, None, None, 0, None);
    }

    /// Starts passes when the view changed, takes finished ones and paints the latest over
    /// `rect`. `moving`: the camera is moving (the preview waits for it to stop).
    /// `generation`: the viewport's scene generation (changes with the model, not with
    /// highlights, which the preview leaves out).
    pub fn frame(
        &mut self,
        ui: &egui::Ui,
        scene: Arc<Scene>,
        generation: u64,
        camera: Camera,
        rect: egui::Rect,
        moving: bool,
    ) {
        if !self.on || rect.width() < 2.0 || rect.height() < 2.0 {
            return;
        }
        let ppp = ui.ctx().pixels_per_point();
        let size = [((rect.width() * ppp / 2.0) as u32).max(1), ((rect.height() * ppp / 2.0) as u32).max(1)];
        // The model by generation and what is shown (hiding a body edits the scene in place).
        let hidden: Vec<u64> = scene.meshes().filter(|(_, m)| !m.visible()).map(|(id, _)| id.0).collect();
        let key = format!("{generation} {hidden:?} {camera:?} {size:?}");
        if moving {
            if self.key.is_some() {
                self.stop();
            }
            return;
        }
        if self.key.as_deref() != Some(key.as_str()) {
            self.stop();
            self.key = Some(key);
            let generation = self.generation.load(Ordering::Relaxed);
            let current = self.generation.clone();
            let (tx, rx) = channel();
            self.rx = Some(rx);
            let ctx = ui.ctx().clone();
            std::thread::spawn(move || {
                // Hover and selection colours stay out of the photo.
                let mut scene = Scene::clone(&scene);
                scene.clear_highlights(gusset_render::Highlight::Preselect);
                scene.clear_highlights(gusset_render::Highlight::Select);
                // An abandoned pass stops within a row, not at the end of the pass.
                let stop = || current.load(Ordering::Relaxed) != generation;
                for samples in PASSES {
                    if stop() {
                        return;
                    }
                    let set = gusset_render::TraceSettings { samples, ..Default::default() };
                    let pass = match gusset_render::render_traced_until(&scene, &camera, size, set, &stop) {
                        Ok(img) => Ok((img, samples)),
                        Err(gusset_render::RenderError::Cancelled) => return,
                        Err(e) => Err(e.to_string()),
                    };
                    let failed = pass.is_err();
                    if tx.send((generation, pass)).is_err() || failed {
                        ctx.request_repaint();
                        return;
                    }
                    ctx.request_repaint();
                }
            });
        }
        while let Some((generation, pass)) = self.rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            if generation != self.generation.load(Ordering::Relaxed) {
                continue;
            }
            let (img, samples) = match pass {
                Ok(p) => p,
                Err(e) => {
                    self.error = Some(e);
                    continue;
                }
            };
            let image = egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.data);
            match &mut self.texture {
                Some(t) => t.set(image, egui::TextureOptions::LINEAR),
                None => {
                    self.texture = Some(ui.ctx().load_texture("photo-preview", image, egui::TextureOptions::LINEAR))
                }
            }
            self.samples = samples;
        }
        if let Some(t) = &self.texture {
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            ui.painter().image(t.id(), rect, uv, egui::Color32::WHITE);
        }
        if let Some(e) = &self.error {
            ui.painter().text(
                rect.left_top() + egui::vec2(12.0, 12.0),
                egui::Align2::LEFT_TOP,
                format!("Photo preview: {e}"),
                egui::FontId::proportional(13.0),
                ui.visuals().error_fg_color,
            );
        }
    }
}

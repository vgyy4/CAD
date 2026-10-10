//! `render_view` entry point: a GPU headless renderer when any adapter works, the CPU
//! rasterizer otherwise, so a PNG is always produced.

use crate::camera::Camera;
use crate::frame::ViewOptions;
use crate::headless::{AdapterPreference, HeadlessRenderer, RenderError};
use crate::image::RgbaImage;
use crate::scene::Scene;
use crate::software::SoftwareRenderer;

/// GPU headless renderer or CPU fallback.
#[derive(Debug)]
pub enum ViewRenderer {
    /// A wgpu adapter is available.
    Gpu(Box<HeadlessRenderer>),
    /// No adapter at all: the CPU rasterizer, with the reason the GPU chain failed.
    Software(SoftwareRenderer, String),
}

impl ViewRenderer {
    /// Try the GPU chain, then fall back to the CPU rasterizer. Never fails.
    #[must_use]
    pub fn new() -> ViewRenderer {
        Self::with_preference(AdapterPreference::Auto)
    }

    /// Like [`ViewRenderer::new`] with a restricted adapter chain.
    #[must_use]
    pub fn with_preference(pref: AdapterPreference) -> ViewRenderer {
        match HeadlessRenderer::with_preference(pref) {
            Ok(r) => ViewRenderer::Gpu(Box::new(r)),
            Err(e) => ViewRenderer::Software(SoftwareRenderer::new(), e.to_string()),
        }
    }

    /// Human-readable description of the path in use.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Gpu(r) => format!("gpu: {}", r.adapter()),
            Self::Software(_, why) => format!("software rasterizer (no GPU adapter: {why})"),
        }
    }

    /// Set view options on whichever path is active.
    pub fn set_options(&mut self, options: ViewOptions) {
        match self {
            Self::Gpu(r) => r.renderer_mut().set_options(options),
            Self::Software(r, _) => r.set_options(options),
        }
    }

    /// Render to an RGBA image. A GPU error falls back to the CPU rasterizer for this
    /// frame (and is reported only if that fails too).
    ///
    /// # Errors
    /// Invalid size.
    pub fn render_rgba(&mut self, scene: &Scene, camera: &Camera, size: [u32; 2]) -> Result<RgbaImage, RenderError> {
        match self {
            Self::Gpu(r) => match r.render_rgba(scene, camera, size) {
                Ok((img, _)) => Ok(img),
                Err(_) => {
                    let mut sw = SoftwareRenderer::new();
                    sw.set_options(*r.renderer().options());
                    sw.render_rgba(scene, camera, size)
                }
            },
            Self::Software(r, _) => r.render_rgba(scene, camera, size),
        }
    }

    /// Render to PNG bytes.
    ///
    /// # Errors
    /// See [`ViewRenderer::render_rgba`].
    pub fn render_png(&mut self, scene: &Scene, camera: &Camera, size: [u32; 2]) -> Result<Vec<u8>, RenderError> {
        Ok(self.render_rgba(scene, camera, size)?.to_png()?)
    }
}

impl Default for ViewRenderer {
    fn default() -> Self {
        Self::new()
    }
}

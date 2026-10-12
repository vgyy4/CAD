//! Windowless rendering for `render_view`, screenshot tests and the GPU self-test.
//!
//! Adapter fallback chain (ADR 0009): hardware adapter (Vulkan/DX12/Metal) →
//! `force_fallback_adapter` (WARP on Windows, lavapipe on Linux) → the GL backend
//! (llvmpipe through EGL on Linux). Only downlevel (WebGL2-class) limits and no optional
//! features are requested. If every step fails, use [`crate::ViewRenderer`], which
//! falls back to [`crate::SoftwareRenderer`].

use std::fmt;
use std::future::Future;
use std::pin::pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};

use gusset_hw::{GpuClass, GpuDiagnostics, QualityTier, tier};

use crate::camera::Camera;
use crate::gpu::{FrameStats, Renderer};
use crate::image::{ImageError, RgbaImage};
use crate::scene::Scene;

/// Errors from GPU setup and headless rendering.
#[derive(Debug)]
pub enum RenderError {
    /// No adapter could be created; the string lists what was tried.
    NoAdapter(String),
    /// The device reported an error (validation, out of memory, device lost).
    Gpu(String),
    /// The requested image size is zero or exceeds the device's texture limit.
    InvalidSize {
        /// Requested size.
        size: [u32; 2],
        /// Largest supported dimension.
        max: u32,
    },
    /// Reading the image back failed.
    Readback(String),
    /// PNG encoding failed.
    Image(ImageError),
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoAdapter(s) => write!(f, "no usable GPU adapter ({s})"),
            Self::Gpu(s) => write!(f, "GPU error: {s}"),
            Self::InvalidSize { size, max } => {
                write!(f, "image size {}x{} is invalid (each side must be 1..={max})", size[0], size[1])
            }
            Self::Readback(s) => write!(f, "image readback failed: {s}"),
            Self::Image(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for RenderError {}

impl From<ImageError> for RenderError {
    fn from(e: ImageError) -> Self {
        Self::Image(e)
    }
}

/// Which adapters [`HeadlessRenderer::with_preference`] may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AdapterPreference {
    /// The full fallback chain.
    #[default]
    Auto,
    /// Only the primary backends (Vulkan/DX12/Metal), without forcing a fallback
    /// adapter (may still return a software adapter if it is the only one).
    Primary,
    /// Only `force_fallback_adapter` on the primary backends (WARP / lavapipe).
    SoftwareFallback,
    /// Only the GL backend (never on Windows, where DX12/WARP always exist).
    Gl,
}

/// What was chosen, for the diagnostics panel and test logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterReport {
    /// Adapter name.
    pub name: String,
    /// Backend name ("vulkan", "dx12", "metal", "gl").
    pub backend: String,
    /// wgpu device type, as text.
    pub device_type: String,
    /// Driver name.
    pub driver: String,
    /// Driver info / version.
    pub driver_info: String,
    /// Class used for the automatic tier.
    pub class: GpuClass,
    /// Steps of the fallback chain that failed before this one.
    pub failed: Vec<String>,
}

impl fmt::Display for AdapterReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({}, {}, driver {} {})",
            self.name,
            self.backend,
            self.class.label(),
            self.driver,
            self.driver_info
        )
    }
}

/// Map wgpu adapter info to a [`GpuClass`]. Software rasterizers that report
/// `Other` (e.g. llvmpipe on GL) are recognized by name.
#[must_use]
pub fn classify_adapter(info: &wgpu::AdapterInfo) -> GpuClass {
    let name = info.name.to_ascii_lowercase();
    let driver = info.driver.to_ascii_lowercase();
    let software = ["llvmpipe", "lavapipe", "softpipe", "swiftshader", "microsoft basic render", "warp"];
    if software.iter().any(|s| name.contains(s) || driver.contains(s)) {
        return GpuClass::Software;
    }
    match info.device_type {
        wgpu::DeviceType::Cpu => GpuClass::Software,
        wgpu::DeviceType::IntegratedGpu => GpuClass::Integrated,
        wgpu::DeviceType::DiscreteGpu => GpuClass::Discrete,
        wgpu::DeviceType::VirtualGpu => GpuClass::Virtual,
        wgpu::DeviceType::Other => GpuClass::Unknown,
    }
}

/// The automatic tier for an adapter (gusset-hw's registry decides).
#[must_use]
pub fn tier_for(info: &wgpu::AdapterInfo) -> QualityTier {
    tier::auto_tier(classify_adapter(info))
}

fn backend_name(b: wgpu::Backend) -> String {
    match b {
        wgpu::Backend::Vulkan => "vulkan".into(),
        wgpu::Backend::Metal => "metal".into(),
        wgpu::Backend::Dx12 => "dx12".into(),
        wgpu::Backend::Gl => "gl".into(),
        other => format!("{other:?}").to_ascii_lowercase(),
    }
}

struct ThreadWaker(std::thread::Thread);

impl Wake for ThreadWaker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

/// Minimal blocking executor for wgpu's setup futures (native backends resolve them
/// immediately or after a device poll).
pub(crate) fn block_on<F: Future>(fut: F) -> F::Output {
    let waker = Waker::from(Arc::new(ThreadWaker(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut fut = pin!(fut);
    loop {
        if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::park_timeout(std::time::Duration::from_millis(5));
    }
}

/// Limits we request: WebGL2-class downlevel limits, with texture sizes raised to what
/// the adapter supports (screenshots larger than 2048 px).
#[must_use]
pub fn required_limits(adapter_limits: &wgpu::Limits) -> wgpu::Limits {
    wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter_limits.clone())
}

/// An offscreen renderer with its own device.
pub struct HeadlessRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Renderer,
    report: AdapterReport,
    errors: Arc<Mutex<Vec<String>>>,
    target: Option<([u32; 2], wgpu::Texture, wgpu::TextureView, wgpu::Buffer)>,
}

impl fmt::Debug for HeadlessRenderer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HeadlessRenderer").field("adapter", &self.report).finish_non_exhaustive()
    }
}

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

impl HeadlessRenderer {
    /// Pick an adapter with the full fallback chain.
    ///
    /// # Errors
    /// [`RenderError::NoAdapter`] when no step of the chain works.
    pub fn new() -> Result<HeadlessRenderer, RenderError> {
        Self::with_preference(AdapterPreference::Auto)
    }

    /// Pick an adapter, restricted by `pref`.
    ///
    /// # Errors
    /// [`RenderError::NoAdapter`] when no allowed step works.
    pub fn with_preference(pref: AdapterPreference) -> Result<HeadlessRenderer, RenderError> {
        let primary = wgpu::Backends::PRIMARY;
        // GL is the last resort for machines without Vulkan/Metal/DX12. Windows always
        // has DX12 with WARP, so GL adds nothing there, and its drivers are the least
        // reliable (an access violation on Windows ARM64 runners): not used (ADR 0009).
        let gl: Vec<(&str, wgpu::Backends, bool)> = if cfg!(windows) {
            Vec::new()
        } else {
            vec![("gl", wgpu::Backends::GL, false), ("gl-fallback", wgpu::Backends::GL, true)]
        };
        let steps: Vec<(&str, wgpu::Backends, bool)> = match pref {
            AdapterPreference::Auto => {
                let mut v = vec![("primary", primary, false), ("primary-fallback", primary, true)];
                v.extend(gl);
                v
            }
            AdapterPreference::Primary => vec![("primary", primary, false)],
            AdapterPreference::SoftwareFallback => vec![("primary-fallback", primary, true)],
            AdapterPreference::Gl => gl,
        };
        let mut failed = Vec::new();
        for (label, backends, force_fallback) in steps {
            match Self::try_step(backends, force_fallback) {
                Ok((adapter, device, queue)) => {
                    let info = adapter.get_info();
                    let report = AdapterReport {
                        name: info.name.clone(),
                        backend: backend_name(info.backend),
                        device_type: format!("{:?}", info.device_type),
                        driver: info.driver.clone(),
                        driver_info: info.driver_info.clone(),
                        class: classify_adapter(&info),
                        failed,
                    };
                    let mut renderer = Renderer::new(&device, &queue, FORMAT);
                    renderer.set_quality_tier(tier::auto_tier(report.class));
                    let color = adapter.get_texture_format_features(FORMAT).flags;
                    let depth = adapter.get_texture_format_features(wgpu::TextureFormat::Depth32Float).flags;
                    let counts: Vec<u32> = [2, 4, 8, 16]
                        .into_iter()
                        .filter(|&c| color.sample_count_supported(c) && depth.sample_count_supported(c))
                        // Counts other than 4 need TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
                        // an optional feature we do not request.
                        .filter(|&c| c == 4)
                        .collect();
                    renderer.set_supported_sample_counts(&counts);
                    let errors = Arc::new(Mutex::new(Vec::new()));
                    let sink = Arc::clone(&errors);
                    device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
                        if let Ok(mut v) = sink.lock() {
                            v.push(e.to_string());
                        }
                    }));
                    return Ok(HeadlessRenderer { device, queue, renderer, report, errors, target: None });
                }
                Err(e) => failed.push(format!("{label}: {e}")),
            }
        }
        if failed.is_empty() {
            failed.push("the GL backend is not used on Windows".to_string());
        }
        Err(RenderError::NoAdapter(failed.join("; ")))
    }

    fn try_step(
        backends: wgpu::Backends,
        force_fallback_adapter: bool,
    ) -> Result<(wgpu::Adapter, wgpu::Device, wgpu::Queue), String> {
        // One device creation at a time in this process: concurrent creation crashed the
        // WARP software adapter on Windows ARM64 (render_view requests can overlap).
        static CREATE: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _one = CREATE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        // Release builds never take behavior from environment variables (spec), so the
        // descriptor is built explicitly rather than with `from_env`.
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = backends;
        let instance = wgpu::Instance::new(desc);
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::None,
            force_fallback_adapter,
            compatible_surface: None,
            apply_limit_buckets: false,
        }))
        .map_err(|e| e.to_string())?;
        let limits = required_limits(&adapter.limits());
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("gusset-render headless"),
            required_features: wgpu::Features::empty(),
            required_limits: limits,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::Off,
        }))
        .map_err(|e| e.to_string())?;
        Ok((adapter, device, queue))
    }

    /// The chosen adapter.
    #[must_use]
    pub fn adapter(&self) -> &AdapterReport {
        &self.report
    }

    /// GPU facts for `gusset_hw::Diagnostics::set_gpu`.
    #[must_use]
    pub fn gpu_diagnostics(&self, user_override: Option<QualityTier>) -> GpuDiagnostics {
        GpuDiagnostics {
            adapter_name: self.report.name.clone(),
            backend: self.report.backend.clone(),
            class: self.report.class,
            driver: self.report.driver.clone(),
            driver_info: self.report.driver_info.clone(),
            tier: tier::select_tier(self.report.class, user_override),
            tier_overridden: user_override.is_some(),
            failed_backends: self.report.failed.clone(),
        }
    }

    /// The device (e.g. to share with a UI).
    #[must_use]
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// The queue.
    #[must_use]
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// The inner renderer (options, quality).
    #[must_use]
    pub fn renderer(&self) -> &Renderer {
        &self.renderer
    }

    /// The inner renderer, mutable.
    pub fn renderer_mut(&mut self) -> &mut Renderer {
        &mut self.renderer
    }

    /// Render to an RGBA8 sRGB image.
    ///
    /// # Errors
    /// Invalid size, a device error or a failed readback.
    pub fn render_rgba(
        &mut self,
        scene: &Scene,
        camera: &Camera,
        size: [u32; 2],
    ) -> Result<(RgbaImage, FrameStats), RenderError> {
        let max = self.device.limits().max_texture_dimension_2d;
        if size[0] == 0 || size[1] == 0 || size[0] > max || size[1] > max {
            return Err(RenderError::InvalidSize { size, max });
        }
        let row = u64::from(size[0]) * 4;
        let padded =
            row.div_ceil(u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)) * u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        if self.target.as_ref().is_none_or(|t| t.0 != size) {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("gusset-render headless target"),
                size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("gusset-render readback"),
                size: padded * u64::from(size[1]),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            self.target = Some((size, texture, view, buffer));
        }
        let Some((_, texture, view, buffer)) = self.target.as_ref() else {
            return Err(RenderError::Readback("no target".into()));
        };
        let stats = self.renderer.render_to(scene, camera, view, size);
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("readback") });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded as u32),
                    rows_per_image: Some(size[1]),
                },
            },
            wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
        );
        self.queue.submit([enc.finish()]);
        let done: Arc<Mutex<Option<Result<(), String>>>> = Arc::new(Mutex::new(None));
        let d2 = Arc::clone(&done);
        buffer.map_async(wgpu::MapMode::Read, .., move |r| {
            if let Ok(mut g) = d2.lock() {
                *g = Some(r.map_err(|e| e.to_string()));
            }
        });
        self.device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| RenderError::Gpu(e.to_string()))?;
        let mapped = done.lock().map_err(|_| RenderError::Readback("lock poisoned".into()))?.take();
        match mapped {
            Some(Ok(())) => {}
            Some(Err(e)) => return Err(RenderError::Readback(e)),
            None => return Err(RenderError::Readback("buffer was not mapped".into())),
        }
        let mut data = Vec::with_capacity(row as usize * size[1] as usize);
        {
            let view = buffer.get_mapped_range(..).map_err(|e| RenderError::Readback(e.to_string()))?;
            for chunk in view.chunks(padded as usize).take(size[1] as usize) {
                data.extend_from_slice(chunk.get(..row as usize).unwrap_or(&[]));
            }
        }
        buffer.unmap();
        let errors = self.errors.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default();
        if !errors.is_empty() {
            return Err(RenderError::Gpu(errors.join("; ")));
        }
        if data.len() != row as usize * size[1] as usize {
            return Err(RenderError::Readback("short read".into()));
        }
        Ok((RgbaImage { width: size[0], height: size[1], data }, stats))
    }

    /// Render to PNG bytes.
    ///
    /// # Errors
    /// See [`HeadlessRenderer::render_rgba`].
    pub fn render_png(&mut self, scene: &Scene, camera: &Camera, size: [u32; 2]) -> Result<Vec<u8>, RenderError> {
        Ok(self.render_rgba(scene, camera, size)?.0.to_png()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(name: &str, t: wgpu::DeviceType) -> wgpu::AdapterInfo {
        wgpu::AdapterInfo {
            name: name.into(),
            vendor: 0,
            device: 0,
            device_type: t,
            device_pci_bus_id: String::new(),
            driver: String::new(),
            driver_info: String::new(),
            backend: wgpu::Backend::Vulkan,
            subgroup_min_size: 0,
            subgroup_max_size: 0,
            transient_saves_memory: None,
            limit_bucket: None,
        }
    }

    #[test]
    fn classification_and_tiers() {
        assert_eq!(
            classify_adapter(&info("llvmpipe (LLVM 19.1.1, 256 bits)", wgpu::DeviceType::Other)),
            GpuClass::Software
        );
        assert_eq!(classify_adapter(&info("Microsoft Basic Render Driver", wgpu::DeviceType::Cpu)), GpuClass::Software);
        assert_eq!(
            classify_adapter(&info("Intel(R) HD Graphics 520", wgpu::DeviceType::IntegratedGpu)),
            GpuClass::Integrated
        );
        assert_eq!(classify_adapter(&info("Radeon RX 7600", wgpu::DeviceType::DiscreteGpu)), GpuClass::Discrete);
        assert_eq!(tier_for(&info("Radeon RX 7600", wgpu::DeviceType::DiscreteGpu)), QualityTier::High);
        assert_eq!(tier_for(&info("Intel(R) HD Graphics 520", wgpu::DeviceType::IntegratedGpu)), QualityTier::Low);
        assert_eq!(tier_for(&info("virgl", wgpu::DeviceType::VirtualGpu)), QualityTier::Low);
    }

    #[test]
    fn requested_limits_are_downlevel() {
        let adapter = wgpu::Limits::default();
        let l = required_limits(&adapter);
        assert_eq!(l.max_storage_buffers_per_shader_stage, 0);
        assert_eq!(l.max_compute_workgroups_per_dimension, 0);
        assert_eq!(l.max_texture_dimension_2d, adapter.max_texture_dimension_2d);
    }
}

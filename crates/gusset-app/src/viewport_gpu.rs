//! GPU viewport: `gusset-render`'s [`Renderer`] on eframe's wgpu device, through an
//! `egui_wgpu` paint callback (ADR 0009).
//!
//! `prepare` renders the scene into an offscreen texture (depth-tested, hidden edges
//! removed, MSAA by quality tier) only when the frame key changed; `paint` copies that
//! texture into egui's render pass with a full-screen triangle. The renderer and its
//! GPU buffers live in egui_wgpu's callback resources, created on first use.

use crate::viewport::FrameKey;
use eframe::egui_wgpu::{self, CallbackResources, CallbackTrait, ScreenDescriptor};
use eframe::wgpu;
use gusset_hw::QualityTier;
use gusset_render::{Renderer, Scene};
use std::sync::Arc;

/// What the viewport needs to know about eframe's wgpu setup.
#[derive(Debug, Clone, PartialEq)]
pub struct GpuTarget {
    /// Format of egui's render target.
    pub format: wgpu::TextureFormat,
    /// Quality tier for the adapter.
    pub tier: QualityTier,
    /// MSAA sample counts the adapter supports for `format`.
    pub sample_counts: Vec<u32>,
}

impl GpuTarget {
    /// From eframe's (or the test harness's) render state.
    pub fn from_render_state(rs: &egui_wgpu::RenderState) -> GpuTarget {
        let info = rs.adapter.get_info();
        let flags = rs.adapter.get_texture_format_features(rs.target_format).flags;
        let sample_counts = [1, 2, 4, 8, 16].into_iter().filter(|&n| flags.sample_count_supported(n)).collect();
        GpuTarget { format: rs.target_format, tier: gusset_render::tier_for(&info), sample_counts }
    }
}

/// The paint callback for one frame.
pub(crate) fn callback(rect: egui::Rect, scene: Arc<Scene>, key: FrameKey, target: GpuTarget) -> egui::Shape {
    egui::Shape::Callback(egui_wgpu::Callback::new_paint_callback(rect, ViewportPaint { scene, key, target }))
}

struct ViewportPaint {
    scene: Arc<Scene>,
    key: FrameKey,
    target: GpuTarget,
}

/// GPU resources kept across frames.
struct GpuViewport {
    renderer: Renderer,
    target: GpuTarget,
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    /// Offscreen color texture view, its bind group and size.
    image: Option<(wgpu::TextureView, wgpu::BindGroup, [u32; 2])>,
    /// Key of the image in `image`.
    last: Option<FrameKey>,
    /// Scene generation the renderer's GPU buffers belong to.
    generation: Option<u64>,
}

const COPY_SHADER: &str = r"
@group(0) @binding(0) var image: texture_2d<f32>;
@group(0) @binding(1) var image_sampler: sampler;

struct Out {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> Out {
    // One triangle that covers the viewport.
    let x = f32(i32(i & 1u) * 4 - 1);
    let y = f32(i32(i >> 1u) * 4 - 1);
    var o: Out;
    o.pos = vec4<f32>(x, y, 0.0, 1.0);
    o.uv = vec2<f32>(0.5 * x + 0.5, 0.5 - 0.5 * y);
    return o;
}

@fragment
fn fs_main(v: Out) -> @location(0) vec4<f32> {
    return vec4<f32>(textureSample(image, image_sampler, v.uv).rgb, 1.0);
}
";

impl GpuViewport {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, target: &GpuTarget) -> GpuViewport {
        let mut renderer = Renderer::new(device, queue, target.format);
        renderer.set_quality_tier(target.tier);
        renderer.set_supported_sample_counts(&target.sample_counts);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gusset viewport copy"),
            source: wgpu::ShaderSource::Wgsl(COPY_SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gusset viewport copy"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gusset viewport copy"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        // egui's main pass: one sample, no depth attachment (eframe's defaults).
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("gusset viewport copy"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target.format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("gusset viewport copy"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        GpuViewport {
            renderer,
            target: target.clone(),
            layout,
            pipeline,
            sampler,
            image: None,
            last: None,
            generation: None,
        }
    }

    fn ensure_image(&mut self, device: &wgpu::Device, size: [u32; 2]) {
        if self.image.as_ref().is_some_and(|(_, _, s)| *s == size) {
            return;
        }
        let view = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("gusset viewport image"),
                size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.target.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gusset viewport image"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        self.image = Some((view, bind, size));
        self.last = None;
    }
}

impl CallbackTrait for ViewportPaint {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if resources.get::<GpuViewport>().is_none_or(|r| r.target != self.target) {
            resources.insert(GpuViewport::new(device, queue, &self.target));
        }
        let Some(res) = resources.get_mut::<GpuViewport>() else { return Vec::new() };
        let size = self.key.size.map(|s| s.clamp(1, device.limits().max_texture_dimension_2d));
        res.ensure_image(device, size);
        if res.last.as_ref() == Some(&self.key) {
            return Vec::new();
        }
        if res.generation.is_some_and(|g| g != self.key.generation) {
            // A new model: its mesh ids and revisions restart, so drop the old buffers.
            res.renderer.release_buffers();
        }
        res.generation = Some(self.key.generation);
        res.renderer.set_options(self.key.options);
        res.renderer.set_interacting(self.key.interacting);
        if let Some((view, _, size)) = &res.image {
            res.renderer.render_to(&self.scene, &self.key.camera, view, *size);
        }
        res.last = Some(self.key.clone());
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &CallbackResources,
    ) {
        let Some(res) = resources.get::<GpuViewport>() else { return };
        let Some((_, bind, _)) = &res.image else { return };
        pass.set_pipeline(&res.pipeline);
        pass.set_bind_group(0, bind, &[]);
        pass.draw(0..3, 0..1);
    }
}

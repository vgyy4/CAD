//! The wgpu renderer. Works on a caller-provided `wgpu::Device` (shared with egui) and
//! draws into any texture view of the configured format. No internal render loop:
//! work happens only inside [`Renderer::render_to`].

use std::collections::HashMap;

use gusset_hw::QualityTier;
use wgpu::util::DeviceExt as _;

use crate::camera::Camera;
use crate::frame::{FramePlan, ViewOptions};
use crate::math;
use crate::scene::{MeshId, Scene};

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Size of the `Globals` uniform (see `shader.wgsl`).
const GLOBALS_SIZE: u64 = 64 + 16 * 7;
/// Size of the `Draw` uniform.
const DRAW_SIZE: u64 = 64 + 16 * 4;
/// Vertex stride: position `f32x3` + normal `snorm16x4`.
const VERTEX_STRIDE: u64 = 20;
/// Edge buffer stride: one point (`f32x3`).
const EDGE_POINT_STRIDE: u64 = 12;
/// Edge width (physical px) from which edges are drawn as quads instead of lines.
const QUAD_EDGE_MIN_PX: f32 = 1.5;

/// Statistics about one rendered frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameStats {
    /// Draw calls issued.
    pub draw_calls: u32,
    /// Triangles submitted.
    pub triangles: u64,
    /// Edge segments submitted.
    pub edge_segments: u64,
    /// Meshes drawn (after culling).
    pub meshes: u32,
    /// Internal render size (after render scale).
    pub render_size: [u32; 2],
    /// MSAA sample count used.
    pub samples: u32,
    /// Whether edges were drawn as quads (wide lines) rather than line primitives.
    pub quad_edges: bool,
}

/// Quality settings derived from the tier and the interaction state.
#[derive(Debug, Clone, PartialEq)]
pub struct Quality {
    tier: QualityTier,
    interacting: bool,
    supported_samples: Vec<u32>,
}

impl Quality {
    fn new(tier: QualityTier) -> Quality {
        // 1 and 4 samples are guaranteed for renderable formats by WebGPU and WebGL2.
        Quality { tier, interacting: false, supported_samples: vec![1, 4] }
    }

    /// The tier in use.
    #[must_use]
    pub fn tier(&self) -> QualityTier {
        self.tier
    }

    /// MSAA samples for the next frame: the tier's count, lowered to a supported count,
    /// and 1 while interacting.
    #[must_use]
    pub fn samples(&self) -> u32 {
        if self.interacting {
            return 1;
        }
        let want = self.tier.spec().msaa_samples.max(1);
        self.supported_samples.iter().copied().filter(|&s| s <= want).max().unwrap_or(1)
    }

    /// Render scale for the next frame (1.0 when still).
    #[must_use]
    pub fn render_scale(&self) -> f32 {
        if self.interacting { self.tier.spec().interactive_render_scale.clamp(0.25, 1.0) } else { 1.0 }
    }
}

struct Pipelines {
    background: wgpu::RenderPipeline,
    mesh: wgpu::RenderPipeline,
    edge_line: wgpu::RenderPipeline,
    edge_quad: wgpu::RenderPipeline,
}

struct GpuMesh {
    rev: u64,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    edges: Option<wgpu::Buffer>,
}

struct Targets {
    size: [u32; 2],
    samples: u32,
    scaled: bool,
    msaa: Option<wgpu::TextureView>,
    depth: wgpu::TextureView,
    /// Intermediate render-scale texture and its blit bind group.
    intermediate: Option<(wgpu::TextureView, wgpu::BindGroup)>,
}

struct Blit {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
}

/// The viewport renderer on a shared device.
pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    options: ViewOptions,
    quality: Quality,
    module: wgpu::ShaderModule,
    pipeline_layout: wgpu::PipelineLayout,
    globals_buf: wgpu::Buffer,
    globals_bg: wgpu::BindGroup,
    draw_layout: wgpu::BindGroupLayout,
    draw_buf: wgpu::Buffer,
    draw_bg: wgpu::BindGroup,
    draw_capacity: u64,
    draw_stride: u64,
    pipelines: Vec<(u32, Pipelines)>,
    blit: Blit,
    meshes: HashMap<MeshId, GpuMesh>,
    targets: Option<Targets>,
}

impl std::fmt::Debug for Renderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Renderer")
            .field("format", &self.format)
            .field("options", &self.options)
            .field("quality", &self.quality)
            .field("gpu_meshes", &self.meshes.len())
            .finish_non_exhaustive()
    }
}

impl Renderer {
    /// Create a renderer drawing into textures of `format` on `device`. The device only
    /// needs downlevel (WebGL2-class) limits and no optional features. The quality tier
    /// starts at `Low`; call [`Renderer::set_quality_tier`] with the detected tier.
    #[must_use]
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Renderer {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gusset-render viewport"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let uniform_entry = |dynamic: bool, size: u64| wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: dynamic,
                min_binding_size: wgpu::BufferSize::new(size),
            },
            count: None,
        };
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gusset-render globals"),
            entries: &[uniform_entry(false, GLOBALS_SIZE)],
        });
        let draw_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gusset-render draw"),
            entries: &[uniform_entry(true, DRAW_SIZE)],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gusset-render"),
            bind_group_layouts: &[Some(&globals_layout), Some(&draw_layout)],
            immediate_size: 0,
        });
        let globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gusset-render globals"),
            size: GLOBALS_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gusset-render globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals_buf.as_entire_binding() }],
        });
        let align = u64::from(device.limits().min_uniform_buffer_offset_alignment.max(16));
        let draw_stride = DRAW_SIZE.div_ceil(align) * align;
        let draw_capacity = 64;
        let (draw_buf, draw_bg) = Self::create_draw_buffer(device, &draw_layout, draw_capacity * draw_stride);
        let blit = Self::create_blit(device, format);
        Renderer {
            device: device.clone(),
            queue: queue.clone(),
            format,
            options: ViewOptions::default(),
            quality: Quality::new(QualityTier::Low),
            module,
            pipeline_layout,
            globals_buf,
            globals_bg,
            draw_layout,
            draw_buf,
            draw_bg,
            draw_capacity,
            draw_stride,
            pipelines: Vec::new(),
            blit,
            meshes: HashMap::new(),
            targets: None,
        }
    }

    /// The target format.
    #[must_use]
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// View options (theme, DPI scale, edges, lighting).
    #[must_use]
    pub fn options(&self) -> &ViewOptions {
        &self.options
    }

    /// Replace the view options.
    pub fn set_options(&mut self, options: ViewOptions) {
        self.options = options;
    }

    /// Current quality settings.
    #[must_use]
    pub fn quality(&self) -> &Quality {
        &self.quality
    }

    /// Set the quality tier (automatic or user override).
    pub fn set_quality_tier(&mut self, tier: QualityTier) {
        self.quality.tier = tier;
    }

    /// Adaptive quality: while `true` (orbit/pan/zoom in progress) frames use 1x MSAA
    /// and the tier's reduced render scale. The caller must request one more frame
    /// after setting it back to `false`, to refine the still image; the renderer never
    /// schedules frames on its own.
    pub fn set_interacting(&mut self, interacting: bool) {
        self.quality.interacting = interacting;
    }

    /// Restrict MSAA to these sample counts (from the adapter's format features). `1` is
    /// always allowed.
    pub fn set_supported_sample_counts(&mut self, counts: &[u32]) {
        let mut v: Vec<u32> = counts.iter().copied().filter(|c| c.is_power_of_two() && *c <= 16).collect();
        v.push(1);
        v.sort_unstable();
        v.dedup();
        self.quality.supported_samples = v;
    }

    /// Number of meshes with GPU buffers.
    #[must_use]
    pub fn gpu_mesh_count(&self) -> usize {
        self.meshes.len()
    }

    /// Drop all GPU resources except pipelines (e.g. when the viewport is hidden).
    pub fn release_buffers(&mut self) {
        self.meshes.clear();
        self.targets = None;
    }

    fn create_draw_buffer(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        size: u64,
    ) -> (wgpu::Buffer, wgpu::BindGroup) {
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gusset-render draws"),
            size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gusset-render draws"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buf,
                    offset: 0,
                    size: wgpu::BufferSize::new(DRAW_SIZE),
                }),
            }],
        });
        (buf, bg)
    }

    fn create_blit(device: &wgpu::Device, format: wgpu::TextureFormat) -> Blit {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gusset-render blit"),
            source: wgpu::ShaderSource::Wgsl(include_str!("blit.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gusset-render blit"),
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
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gusset-render blit"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("gusset-render blit"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_blit"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_blit"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("gusset-render blit"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Blit { layout, pipeline, sampler }
    }

    fn pipelines(&mut self, samples: u32) -> usize {
        if let Some(i) = self.pipelines.iter().position(|(s, _)| *s == samples) {
            return i;
        }
        let device = &self.device;
        let module = &self.module;
        let format = self.format;
        let layout = &self.pipeline_layout;
        let make = |label: &str,
                    vs: &str,
                    fs: &str,
                    buffers: &[Option<wgpu::VertexBufferLayout<'_>>],
                    topology: wgpu::PrimitiveTopology,
                    depth: (bool, wgpu::CompareFunction),
                    bias: wgpu::DepthBiasState| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some(vs),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState { topology, cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(depth.0),
                    depth_compare: Some(depth.1),
                    stencil: wgpu::StencilState::default(),
                    bias,
                }),
                multisample: wgpu::MultisampleState { count: samples, mask: !0, alpha_to_coverage_enabled: false },
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some(fs),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let mesh_attrs = [
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Snorm16x4, offset: 12, shader_location: 1 },
        ];
        let line_attrs =
            [wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 }];
        let quad_attrs = [
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 12, shader_location: 1 },
        ];
        let no_bias = wgpu::DepthBiasState::default();
        let p = Pipelines {
            background: make(
                "gusset-render background",
                "vs_background",
                "fs_background",
                &[],
                wgpu::PrimitiveTopology::TriangleList,
                (false, wgpu::CompareFunction::Always),
                no_bias,
            ),
            mesh: make(
                "gusset-render mesh",
                "vs_mesh",
                "fs_mesh",
                &[Some(wgpu::VertexBufferLayout {
                    array_stride: VERTEX_STRIDE,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &mesh_attrs,
                })],
                wgpu::PrimitiveTopology::TriangleList,
                (true, wgpu::CompareFunction::Less),
                // Push faces back a little at grazing angles so edges stay visible.
                wgpu::DepthBiasState { constant: 0, slope_scale: 1.0, clamp: 0.0 },
            ),
            edge_line: make(
                "gusset-render edge lines",
                "vs_edge_line",
                "fs_edge",
                &[Some(wgpu::VertexBufferLayout {
                    array_stride: EDGE_POINT_STRIDE,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &line_attrs,
                })],
                wgpu::PrimitiveTopology::LineList,
                (true, wgpu::CompareFunction::LessEqual),
                no_bias,
            ),
            edge_quad: make(
                "gusset-render edge quads",
                "vs_edge_quad",
                "fs_edge",
                &[Some(wgpu::VertexBufferLayout {
                    array_stride: EDGE_POINT_STRIDE * 2,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &quad_attrs,
                })],
                wgpu::PrimitiveTopology::TriangleList,
                (true, wgpu::CompareFunction::LessEqual),
                no_bias,
            ),
        };
        self.pipelines.push((samples, p));
        self.pipelines.len() - 1
    }

    fn sync_mesh(&mut self, scene: &Scene, id: MeshId, rev: u64) {
        if self.meshes.get(&id).is_some_and(|m| m.rev == rev) {
            return;
        }
        let Some(m) = scene.mesh(id) else { return };
        let data = m.data();
        let mut vbytes = Vec::with_capacity(data.positions.len() * VERTEX_STRIDE as usize);
        for (p, n) in data.positions.iter().zip(&data.normals) {
            for c in p {
                vbytes.extend_from_slice(&c.to_le_bytes());
            }
            for c in n {
                vbytes.extend_from_slice(&c.to_le_bytes());
            }
        }
        let ibytes: Vec<u8> = data.indices.iter().flat_map(|i| i.to_le_bytes()).collect();
        let init = |label: &str, contents: &[u8], usage| {
            // wgpu rejects zero-sized mapped buffers on some backends; pad to 4 bytes.
            let pad = [0_u8; 4];
            let contents = if contents.is_empty() { &pad[..] } else { contents };
            self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents, usage })
        };
        let vertices = init("gusset-render vertices", &vbytes, wgpu::BufferUsages::VERTEX);
        let indices = init("gusset-render indices", &ibytes, wgpu::BufferUsages::INDEX);
        let edges = (!data.edge_points.is_empty()).then(|| {
            let e: Vec<u8> = data.edge_points.iter().flatten().flat_map(|c| c.to_le_bytes()).collect();
            init("gusset-render edges", &e, wgpu::BufferUsages::VERTEX)
        });
        self.meshes.insert(id, GpuMesh { rev, vertices, indices, edges });
    }

    fn ensure_targets(&mut self, size: [u32; 2], samples: u32, scaled: bool) {
        if self.targets.as_ref().is_some_and(|t| t.size == size && t.samples == samples && t.scaled == scaled) {
            return;
        }
        let extent = wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 };
        let tex = |label: &str, format, samples, usage| {
            self.device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: extent,
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        let msaa = (samples > 1)
            .then(|| tex("gusset-render msaa", self.format, samples, wgpu::TextureUsages::RENDER_ATTACHMENT));
        let depth = tex("gusset-render depth", DEPTH_FORMAT, samples, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let intermediate = scaled.then(|| {
            let view = tex(
                "gusset-render scaled",
                self.format,
                1,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            );
            let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("gusset-render blit"),
                layout: &self.blit.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.blit.sampler) },
                ],
            });
            (view, bg)
        });
        self.targets = Some(Targets { size, samples, scaled, msaa, depth, intermediate });
    }

    /// Render `scene` from `camera` into `target` (a view of a texture with this
    /// renderer's format and `RENDER_ATTACHMENT` usage, `size` = its pixel size), and
    /// submit the work. A zero-sized target draws nothing.
    pub fn render_to(
        &mut self,
        scene: &Scene,
        camera: &Camera,
        target: &wgpu::TextureView,
        size: [u32; 2],
    ) -> FrameStats {
        let samples = self.quality.samples();
        let scale = self.quality.render_scale();
        let internal = if scale < 1.0 { size.map(|s| ((s as f32 * scale).round() as u32).max(1)) } else { size };
        let mut stats = FrameStats {
            draw_calls: 0,
            triangles: 0,
            edge_segments: 0,
            meshes: 0,
            render_size: internal,
            samples,
            quad_edges: false,
        };
        if size[0] == 0 || size[1] == 0 {
            return stats;
        }
        let scaled = internal != size;
        let plan = FramePlan::build(scene, camera, internal, &self.options);

        // GPU mesh cache: drop meshes that left the scene, upload new revisions.
        self.meshes.retain(|id, g| scene.mesh(*id).is_some_and(|m| m.geometry_rev == g.rev));
        for d in &plan.draws {
            self.sync_mesh(scene, d.id, d.geometry_rev);
        }

        // Uniforms.
        let encode_srgb = !self.format.is_srgb();
        let quad_edges = plan.edge_width_px >= QUAD_EDGE_MIN_PX;
        stats.quad_edges = quad_edges;
        self.queue.write_buffer(&self.globals_buf, 0, &globals_bytes(&plan, internal, encode_srgb));
        let n_draws: u64 = 1 + plan.draws.iter().map(|d| (d.tri_runs.len() + d.edge_runs.len()) as u64).sum::<u64>();
        if n_draws > self.draw_capacity {
            self.draw_capacity = n_draws.next_power_of_two();
            let (b, g) =
                Self::create_draw_buffer(&self.device, &self.draw_layout, self.draw_capacity * self.draw_stride);
            self.draw_buf = b;
            self.draw_bg = g;
        }
        let stride = self.draw_stride as usize;
        let mut draw_bytes = vec![0_u8; n_draws as usize * stride];
        let mut slot = 1_usize; // slot 0: background (unused contents)
        let mut put = |bytes: &mut Vec<u8>, mv: &math::Mat4, n: &[math::Vec3; 3], color: [f32; 3]| -> u32 {
            let off = slot * stride;
            let mut e = Vec::with_capacity(DRAW_SIZE as usize);
            push_mat(&mut e, mv);
            for c in n {
                push_vec4(&mut e, [c[0] as f32, c[1] as f32, c[2] as f32, 0.0]);
            }
            push_vec4(&mut e, [color[0], color[1], color[2], 1.0]);
            if let Some(dst) = bytes.get_mut(off..off + e.len()) {
                dst.copy_from_slice(&e);
            }
            slot += 1;
            off as u32
        };
        let mut offsets: Vec<(Vec<u32>, Vec<u32>)> = Vec::with_capacity(plan.draws.len());
        for d in &plan.draws {
            let tri = d.tri_runs.iter().map(|(_, c)| put(&mut draw_bytes, &d.model_view, &d.normal, *c)).collect();
            let edge = d.edge_runs.iter().map(|(_, c)| put(&mut draw_bytes, &d.model_view, &d.normal, *c)).collect();
            offsets.push((tri, edge));
        }
        self.queue.write_buffer(&self.draw_buf, 0, &draw_bytes);

        self.ensure_targets(internal, samples, scaled);
        let pi = self.pipelines(samples);
        let (Some(targets), Some((_, pipes))) = (self.targets.as_ref(), self.pipelines.get(pi)) else {
            return stats;
        };
        let dest = targets.intermediate.as_ref().map_or(target, |(v, _)| v);
        let mut encoder =
            self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("gusset-render frame") });
        {
            let (view, resolve) = match &targets.msaa {
                Some(m) => (m, Some(dest)),
                None => (dest, None),
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("gusset-render main"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: resolve,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: if resolve.is_some() { wgpu::StoreOp::Discard } else { wgpu::StoreOp::Store },
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &targets.depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.globals_bg, &[]);
            pass.set_bind_group(1, &self.draw_bg, &[0]);
            pass.set_pipeline(&pipes.background);
            pass.draw(0..3, 0..1);
            stats.draw_calls += 1;

            pass.set_pipeline(&pipes.mesh);
            for (d, (tri_offs, _)) in plan.draws.iter().zip(&offsets) {
                let Some(g) = self.meshes.get(&d.id) else { continue };
                stats.meshes += 1;
                pass.set_vertex_buffer(0, g.vertices.slice(..));
                pass.set_index_buffer(g.indices.slice(..), wgpu::IndexFormat::Uint32);
                for ((r, _), off) in d.tri_runs.iter().zip(tri_offs) {
                    pass.set_bind_group(1, &self.draw_bg, &[*off]);
                    pass.draw_indexed(r.start * 3..r.end * 3, 0, 0..1);
                    stats.draw_calls += 1;
                    stats.triangles += u64::from(r.end - r.start);
                }
            }
            pass.set_pipeline(if quad_edges { &pipes.edge_quad } else { &pipes.edge_line });
            for (d, (_, edge_offs)) in plan.draws.iter().zip(&offsets) {
                let Some(eb) = self.meshes.get(&d.id).and_then(|g| g.edges.as_ref()) else { continue };
                for ((r, _), off) in d.edge_runs.iter().zip(edge_offs) {
                    if r.end <= r.start {
                        continue;
                    }
                    let seg_bytes = EDGE_POINT_STRIDE * 2;
                    pass.set_bind_group(1, &self.draw_bg, &[*off]);
                    pass.set_vertex_buffer(0, eb.slice(u64::from(r.start) * seg_bytes..u64::from(r.end) * seg_bytes));
                    let n = r.end - r.start;
                    if quad_edges {
                        pass.draw(0..6, 0..n);
                    } else {
                        pass.draw(0..n * 2, 0..1);
                    }
                    stats.draw_calls += 1;
                    stats.edge_segments += u64::from(n);
                }
            }
        }
        if let Some((_, bg)) = &targets.intermediate {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("gusset-render upscale"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.blit.pipeline);
            pass.set_bind_group(0, bg, &[]);
            pass.draw(0..3, 0..1);
            stats.draw_calls += 1;
        }
        self.queue.submit([encoder.finish()]);
        stats
    }
}

fn push_vec4(out: &mut Vec<u8>, v: [f32; 4]) {
    for c in v {
        out.extend_from_slice(&c.to_le_bytes());
    }
}

fn push_mat(out: &mut Vec<u8>, m: &math::Mat4) {
    for col in math::to_f32(m) {
        push_vec4(out, col);
    }
}

fn globals_bytes(plan: &FramePlan, size: [u32; 2], encode_srgb: bool) -> Vec<u8> {
    let l = &plan.lighting;
    let mut b = Vec::with_capacity(GLOBALS_SIZE as usize);
    push_mat(&mut b, &plan.proj);
    push_vec4(&mut b, [plan.bg_top[0], plan.bg_top[1], plan.bg_top[2], 1.0]);
    push_vec4(&mut b, [plan.bg_bottom[0], plan.bg_bottom[1], plan.bg_bottom[2], 1.0]);
    push_vec4(&mut b, [size[0] as f32, size[1] as f32, plan.edge_width_px * 0.5, if encode_srgb { 1.0 } else { 0.0 }]);
    push_vec4(&mut b, [l.key_dir[0], l.key_dir[1], l.key_dir[2], l.key]);
    push_vec4(&mut b, [l.fill_dir[0], l.fill_dir[1], l.fill_dir[2], l.fill]);
    push_vec4(&mut b, [l.sky, l.ground, l.specular, l.shininess]);
    push_vec4(&mut b, [l.rim, if plan.ortho { 1.0 } else { 0.0 }, plan.edge_bias_persp, plan.edge_bias_ortho]);
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_samples_and_scale() {
        let mut q = Quality::new(QualityTier::High);
        assert_eq!(q.samples(), 4); // 8 requested, 4 supported by default
        q.supported_samples = vec![1, 2, 4, 8];
        assert_eq!(q.samples(), 8);
        q.interacting = true;
        assert_eq!(q.samples(), 1);
        let mut q = Quality::new(QualityTier::Low);
        assert_eq!(q.samples(), 1);
        assert!((q.render_scale() - 1.0).abs() < 1e-6);
        q.interacting = true;
        assert!(q.render_scale() < 1.0);
        let q = Quality::new(QualityTier::Medium);
        assert_eq!(q.samples(), 4);
    }

    #[test]
    fn globals_layout_size() {
        let s = crate::Scene::new();
        let plan = FramePlan::build(&s, &Camera::default(), [10, 10], &ViewOptions::default());
        assert_eq!(globals_bytes(&plan, [10, 10], true).len() as u64, GLOBALS_SIZE);
    }
}

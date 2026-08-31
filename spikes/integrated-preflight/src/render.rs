use std::ptr::NonNull;

use anyhow::{Context, Result, anyhow};
use bytemuck::{Pod, Zeroable};
use glyphon::{
    Attrs, Buffer as TextBuffer, Cache, Color as TextColor, Family, FontSystem, Metrics,
    Resolution, Shaping, SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport,
};
use raw_window_handle::{
    AppKitDisplayHandle, AppKitWindowHandle, RawDisplayHandle, RawWindowHandle,
};
use wgpu::util::DeviceExt;

use crate::layout::{CanvasGeometry, DIVIDER_WIDTH, PaneId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VisualState {
    Normal,
    Attention,
    Remote,
}

/// One palette for every native surface. Keep the hierarchy in code, not in
/// per-rectangle literals, so future panes cannot drift from the shell chrome.
struct DesignTokens;

impl DesignTokens {
    // These values are product-facing sRGB tokens. The swapchain is an sRGB
    // target, so every surface goes through `srgb_color` before it reaches
    // the linear shader input. Keeping the readable values here prevents a
    // future rectangle from accidentally mixing color spaces.
    const BACKGROUND_SRGB: [u8; 3] = [10, 12, 16];
    const NAVIGATOR_SRGB: [u8; 3] = [16, 19, 25];
    const CHROME_SRGB: [u8; 3] = [21, 24, 31];
    const PANE_SURFACE_SRGB: [u8; 3] = [25, 29, 38];
    const PANE_ACTIVE_SRGB: [u8; 3] = [28, 32, 41];
    const SELECTION_SURFACE_SRGB: [u8; 3] = [35, 43, 56];
    const DIVIDER_SRGB: [u8; 3] = [53, 63, 78];
    const ACCENT_SRGB: [u8; 3] = [76, 160, 240];
    const ATTENTION_SRGB: [u8; 3] = [236, 150, 45];

    fn background() -> [f32; 4] {
        srgb_color(Self::BACKGROUND_SRGB)
    }

    fn navigator() -> [f32; 4] {
        srgb_color(Self::NAVIGATOR_SRGB)
    }

    fn chrome() -> [f32; 4] {
        srgb_color(Self::CHROME_SRGB)
    }

    fn surface() -> [f32; 4] {
        srgb_color(Self::PANE_SURFACE_SRGB)
    }

    fn pane_active() -> [f32; 4] {
        srgb_color(Self::PANE_ACTIVE_SRGB)
    }

    fn selection_surface() -> [f32; 4] {
        srgb_color(Self::SELECTION_SURFACE_SRGB)
    }

    fn divider() -> [f32; 4] {
        srgb_color(Self::DIVIDER_SRGB)
    }

    fn accent() -> [f32; 4] {
        srgb_color(Self::ACCENT_SRGB)
    }

    fn attention() -> [f32; 4] {
        srgb_color(Self::ATTENTION_SRGB)
    }

    fn text_primary() -> TextColor {
        TextColor::rgb(231, 238, 247)
    }

    fn text_secondary() -> TextColor {
        TextColor::rgb(158, 174, 193)
    }

    fn text_muted() -> TextColor {
        TextColor::rgb(112, 129, 151)
    }
}

#[derive(Clone, Debug)]
pub struct FrameModel {
    pub zoomed: Option<PaneId>,
    pub focused: PaneId,
    pub terminal_status: String,
    pub terminal_text: String,
    pub editor_text: String,
    pub input_generation: u64,
    pub browser_enabled: bool,
    pub visual_state: VisualState,
}

#[derive(Clone, Debug)]
pub struct PresentObservation {
    pub width: u32,
    pub height: u32,
    pub input_generation: u64,
    pub adapter_name: String,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 2],
    color: [f32; 4],
}

/// Maps one logical AppKit canvas to the physical WGPU surface. Layout is
/// always derived in logical points; this object is the only boundary that
/// projects rectangles and text areas to backing pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
struct RenderGeometry {
    logical: CanvasGeometry,
    physical_width: f32,
    physical_height: f32,
    scale_factor: f32,
}

impl RenderGeometry {
    fn from_physical(width: u32, height: u32, scale_factor: f32) -> Self {
        let scale_factor = normalize_scale(scale_factor as f64);
        let physical_width = width.max(1) as f32;
        let physical_height = height.max(1) as f32;
        let logical = CanvasGeometry::for_size(
            f64::from(physical_width) / f64::from(scale_factor),
            f64::from(physical_height) / f64::from(scale_factor),
        );
        Self {
            logical,
            physical_width,
            physical_height,
            scale_factor,
        }
    }

    fn project(&self, value: f32) -> f32 {
        value * self.scale_factor
    }

    fn push_rect(
        &self,
        vertices: &mut Vec<Vertex>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: [f32; 4],
    ) {
        push_rect(
            vertices,
            self.physical_width,
            self.physical_height,
            self.project(x),
            self.project(y),
            self.project(width),
            self.project(height),
            color,
        );
    }
}

fn normalize_scale(scale_factor: f64) -> f32 {
    if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor as f32
    } else {
        1.0
    }
}

pub struct Renderer {
    _instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    scale_factor: f32,
    adapter_name: String,
    shape_pipeline: wgpu::RenderPipeline,
    font_system: FontSystem,
    swash_cache: SwashCache,
    viewport: Viewport,
    atlas: TextAtlas,
    text_renderer: TextRenderer,
    navigator_text: TextBuffer,
    tab_text: TextBuffer,
    primary_text: TextBuffer,
    secondary_text: TextBuffer,
    status_text: TextBuffer,
}

impl Renderer {
    /// The caller keeps the NSView alive until after this renderer is dropped.
    pub unsafe fn new(
        ns_view: NonNull<std::ffi::c_void>,
        width: u32,
        height: u32,
        scale_factor: f64,
    ) -> Result<Self> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::METAL;
        let instance = wgpu::Instance::new(descriptor);
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: Some(RawDisplayHandle::AppKit(AppKitDisplayHandle::new())),
                raw_window_handle: RawWindowHandle::AppKit(AppKitWindowHandle::new(ns_view)),
            })
        }
        .context("stage=wgpu.surface target=NSView backend=Metal retryable=false")?;

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
            apply_limit_buckets: false,
        }))
        .context("stage=wgpu.adapter target=Metal retryable=false")?;
        let info = adapter.get_info();
        if info.backend != wgpu::Backend::Metal {
            return Err(anyhow!(
                "stage=wgpu.adapter target=Metal cause=unexpected-backend actual={:?}",
                info.backend
            ));
        }
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("Herdr IDE native spike device"),
            ..wgpu::DeviceDescriptor::default()
        }))
        .context("stage=wgpu.device target=Metal retryable=false")?;

        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(|candidate| *candidate == wgpu::TextureFormat::Bgra8UnormSrgb)
            .or_else(|| capabilities.formats.first().copied())
            .ok_or_else(|| anyhow!("stage=wgpu.surface target=Metal cause=no-supported-format"))?;
        let alpha_mode = capabilities
            .alpha_modes
            .first()
            .copied()
            .unwrap_or(wgpu::CompositeAlphaMode::Opaque);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::include_wgsl!("shape.wgsl"));
        let shape_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Herdr IDE native spike shape pipeline"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 0,
                            shader_location: 0,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x4,
                            offset: 8,
                            shader_location: 1,
                        },
                    ],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let mut font_system = FontSystem::new();
        let swash_cache = SwashCache::new();
        let cache = Cache::new(&device);
        let viewport = Viewport::new(&device, &cache);
        let mut atlas = TextAtlas::new(&device, &queue, &cache, format);
        let text_renderer =
            TextRenderer::new(&mut atlas, &device, wgpu::MultisampleState::default(), None);
        let navigator_text = TextBuffer::new(&mut font_system, Metrics::new(13.0, 20.0));
        let tab_text = TextBuffer::new(&mut font_system, Metrics::new(12.0, 18.0));
        let primary_text = TextBuffer::new(&mut font_system, Metrics::new(14.0, 21.0));
        let secondary_text = TextBuffer::new(&mut font_system, Metrics::new(14.0, 21.0));
        let status_text = TextBuffer::new(&mut font_system, Metrics::new(12.0, 18.0));

        Ok(Self {
            _instance: instance,
            surface,
            device,
            queue,
            config,
            scale_factor: normalize_scale(scale_factor),
            adapter_name: format!("{} ({:?})", info.name, info.backend),
            shape_pipeline,
            font_system,
            swash_cache,
            viewport,
            atlas,
            text_renderer,
            navigator_text,
            tab_text,
            primary_text,
            secondary_text,
            status_text,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32, scale_factor: f64) {
        let width = width.max(1);
        let height = height.max(1);
        self.scale_factor = normalize_scale(scale_factor);
        if self.config.width == width && self.config.height == height {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    pub fn render(&mut self, model: &FrameModel) -> Result<PresentObservation> {
        let geometry =
            RenderGeometry::from_physical(self.config.width, self.config.height, self.scale_factor);
        let width = geometry.logical.width as f32;
        let height = geometry.logical.height as f32;
        let navigator_width = geometry.logical.navigator_width as f32;
        let tab_height = geometry.logical.tab_height as f32;
        let status_height = geometry.logical.status_height as f32;
        let canvas_left = geometry.logical.canvas_left as f32;
        let canvas_top = geometry.logical.canvas_top as f32;
        let canvas_bottom = geometry.logical.canvas_bottom as f32;
        let canvas_width = geometry.logical.canvas_width as f32;
        let canvas_height = geometry.logical.canvas_height as f32;
        let split = geometry.logical.split_x as f32;
        let terminal_attention = model.visual_state == VisualState::Attention
            || model.terminal_status.contains("failed")
            || model.terminal_status.contains("unavailable")
            || model.terminal_status.contains("error");

        let mut vertices = Vec::with_capacity(48);
        geometry.push_rect(
            &mut vertices,
            0.0,
            0.0,
            width,
            height,
            DesignTokens::background(),
        );
        geometry.push_rect(
            &mut vertices,
            0.0,
            0.0,
            navigator_width,
            height,
            DesignTokens::navigator(),
        );
        geometry.push_rect(
            &mut vertices,
            canvas_left,
            0.0,
            canvas_width,
            tab_height,
            DesignTokens::chrome(),
        );
        geometry.push_rect(
            &mut vertices,
            canvas_left,
            canvas_bottom,
            canvas_width,
            status_height,
            DesignTokens::chrome(),
        );

        // One-pixel dividers keep the low-chrome hierarchy legible without
        // turning every pane into a card.
        geometry.push_rect(
            &mut vertices,
            navigator_width,
            0.0,
            1.0,
            height,
            DesignTokens::divider(),
        );
        geometry.push_rect(
            &mut vertices,
            canvas_left,
            tab_height - 1.0,
            canvas_width,
            1.0,
            DesignTokens::divider(),
        );
        geometry.push_rect(
            &mut vertices,
            canvas_left,
            canvas_bottom,
            canvas_width,
            1.0,
            DesignTokens::divider(),
        );

        // The selected workspace and active tab have a surface, not a bold
        // border. This mirrors Orca's compact two-line navigator rhythm.
        geometry.push_rect(
            &mut vertices,
            10.0,
            76.0,
            navigator_width - 20.0,
            66.0,
            DesignTokens::selection_surface(),
        );
        geometry.push_rect(
            &mut vertices,
            canvas_left + 8.0,
            6.0,
            214.0,
            tab_height - 12.0,
            DesignTokens::selection_surface(),
        );
        geometry.push_rect(
            &mut vertices,
            canvas_left + 226.0,
            6.0,
            130.0,
            tab_height - 12.0,
            DesignTokens::chrome(),
        );

        if model.zoomed.is_none() {
            let terminal_x = canvas_left + 8.0;
            let terminal_width = split - canvas_left - 12.0;
            let editor_x = split + 4.0;
            let editor_width = width - split - 12.0;
            geometry.push_rect(
                &mut vertices,
                terminal_x,
                canvas_top + 8.0,
                terminal_width,
                canvas_height - 16.0,
                pane_color(model.focused == PaneId::TerminalA),
            );
            geometry.push_rect(
                &mut vertices,
                editor_x,
                canvas_top + 8.0,
                editor_width,
                canvas_height - 16.0,
                pane_color(model.focused == PaneId::Editor),
            );
            geometry.push_rect(
                &mut vertices,
                terminal_x,
                canvas_top + 8.0,
                terminal_width,
                DIVIDER_WIDTH as f32 * 2.0,
                if terminal_attention {
                    DesignTokens::attention()
                } else if model.focused == PaneId::TerminalA {
                    DesignTokens::accent()
                } else {
                    DesignTokens::divider()
                },
            );
            geometry.push_rect(
                &mut vertices,
                editor_x,
                canvas_top + 8.0,
                editor_width,
                DIVIDER_WIDTH as f32 * 2.0,
                if model.focused == PaneId::Editor {
                    DesignTokens::accent()
                } else {
                    DesignTokens::divider()
                },
            );
        } else if model.zoomed != Some(PaneId::Browser) {
            geometry.push_rect(
                &mut vertices,
                canvas_left + 8.0,
                canvas_top + 8.0,
                canvas_width - 16.0,
                canvas_height - 16.0,
                pane_color(true),
            );
            geometry.push_rect(
                &mut vertices,
                canvas_left + 8.0,
                canvas_top + 8.0,
                canvas_width - 16.0,
                2.0,
                DesignTokens::accent(),
            );
        }

        let vertex_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Herdr IDE native spike UI rectangles"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });

        let zoom_label = model
            .zoomed
            .map(|pane| format!("⌘⇧↵  {pane:?} zoomed  ·  Esc restore"))
            .unwrap_or_else(|| "⌘⇧↵  Zoom".to_owned());
        let navigator_text = if model.visual_state == VisualState::Remote {
            "HERDR IDE\n\nWORKSPACES\n● local                 7\n  native-spike\n    11 panes · active\n\nAGENTS\n◉ terminal / PTY\n  Working · 1 pane\n\nWORKTREES\n↗ rust-native\n  connected · mini"
        } else {
            "HERDR IDE\n\nWORKSPACES\n● local                 7\n  native-spike\n    11 panes · active\n\nAGENTS\n◉ terminal / PTY\n  Working · 1 pane\n\nWORKTREES\n● rust-native\n  clean · local"
        };
        set_text(
            &mut self.navigator_text,
            &mut self.font_system,
            navigator_text,
            navigator_width - 28.0,
            height - 28.0,
        );
        set_text(
            &mut self.tab_text,
            &mut self.font_system,
            &format!(
                "● integrated-preflight   ×    {}   +    {zoom_label}",
                if model.visual_state == VisualState::Remote {
                    "↗ Remote mini"
                } else if model.browser_enabled {
                    "○ Browser live"
                } else {
                    "○ Browser closed"
                },
            ),
            canvas_width - 28.0,
            tab_height - 8.0,
        );
        let terminal_state = if terminal_attention {
            "⚠ Attention"
        } else {
            "● Ready"
        };
        let status_lead = match model.visual_state {
            VisualState::Normal => "● Ready",
            VisualState::Attention => "⚠ Attention",
            VisualState::Remote => "↗ Remote mini",
        };
        let browser_status = if model.visual_state == VisualState::Remote {
            "remote"
        } else if model.browser_enabled {
            "live"
        } else {
            "closed"
        };
        let terminal = format!(
            "TERMINAL A   {terminal_state}\n{}\n\nInput ready · UTF-8 · scrollback",
            model.terminal_text,
        );
        set_text(
            &mut self.primary_text,
            &mut self.font_system,
            &terminal,
            if model.zoomed.is_some() {
                canvas_width - 44.0
            } else {
                split - canvas_left - 36.0
            },
            canvas_height - 34.0,
        );
        let editor = format!(
            "EDITOR B   {}\n\n{}\n\nWGPU text · UTF-8\n⌘⇧↵  Zoom / Esc restore",
            if model.focused == PaneId::Editor {
                "● Focus"
            } else {
                "○ Inactive"
            },
            model.editor_text,
        );
        set_text(
            &mut self.secondary_text,
            &mut self.font_system,
            &editor,
            width - split - 36.0,
            canvas_height - 34.0,
        );
        set_text(
            &mut self.status_text,
            &mut self.font_system,
            &format!(
                "{status_lead}   ·   PTY 1   ·   Agents 1   ·   Browser {browser_status}   ·   {}×{} pt",
                geometry.logical.width.round(),
                geometry.logical.height.round(),
            ),
            canvas_width - 24.0,
            status_height,
        );

        self.viewport.update(
            &self.queue,
            Resolution {
                width: self.config.width,
                height: self.config.height,
            },
        );
        let mut areas = vec![
            text_area(
                &self.navigator_text,
                geometry.scale_factor,
                16.0,
                22.0,
                0,
                0,
                navigator_width as i32,
                height as i32,
                DesignTokens::text_secondary(),
            ),
            text_area(
                &self.tab_text,
                geometry.scale_factor,
                canvas_left + 16.0,
                13.0,
                canvas_left as i32,
                0,
                width as i32,
                tab_height as i32,
                DesignTokens::text_primary(),
            ),
            text_area(
                &self.status_text,
                geometry.scale_factor,
                canvas_left + 12.0,
                canvas_bottom + 6.0,
                canvas_left as i32,
                canvas_bottom as i32,
                width as i32,
                height as i32,
                DesignTokens::text_muted(),
            ),
        ];
        match model.zoomed {
            None => {
                areas.push(text_area(
                    &self.primary_text,
                    geometry.scale_factor,
                    canvas_left + 22.0,
                    canvas_top + 22.0,
                    canvas_left as i32,
                    canvas_top as i32,
                    split as i32,
                    canvas_bottom as i32,
                    DesignTokens::text_primary(),
                ));
                areas.push(text_area(
                    &self.secondary_text,
                    geometry.scale_factor,
                    split + 18.0,
                    canvas_top + 22.0,
                    split as i32,
                    canvas_top as i32,
                    width as i32,
                    canvas_bottom as i32,
                    DesignTokens::text_primary(),
                ));
            }
            Some(PaneId::Editor) => {
                areas.push(text_area(
                    &self.secondary_text,
                    geometry.scale_factor,
                    canvas_left + 22.0,
                    canvas_top + 22.0,
                    canvas_left as i32,
                    canvas_top as i32,
                    width as i32,
                    canvas_bottom as i32,
                    DesignTokens::text_primary(),
                ));
            }
            Some(PaneId::Browser) => {}
            Some(_) => {
                areas.push(text_area(
                    &self.primary_text,
                    geometry.scale_factor,
                    canvas_left + 22.0,
                    canvas_top + 22.0,
                    canvas_left as i32,
                    canvas_top as i32,
                    width as i32,
                    canvas_bottom as i32,
                    DesignTokens::text_primary(),
                ));
            }
        }
        self.text_renderer
            .prepare(
                &self.device,
                &self.queue,
                &mut self.font_system,
                &mut self.atlas,
                &self.viewport,
                areas,
                &mut self.swash_cache,
            )
            .context("stage=wgpu.text.prepare target=glyphon retryable=true")?;

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Err(anyhow!(
                    "stage=wgpu.present target=surface cause=temporarily-unavailable retryable=true"
                ));
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Suboptimal(_) => {
                self.surface.configure(&self.device, &self.config);
                return Err(anyhow!(
                    "stage=wgpu.present target=surface cause=outdated retryable=true"
                ));
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return Err(anyhow!(
                    "stage=wgpu.present target=surface cause=lost retryable=true"
                ));
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err(anyhow!(
                    "stage=wgpu.present target=surface cause=validation retryable=false"
                ));
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Herdr IDE native spike frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Herdr IDE native spike render pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(DesignTokens::background()[0]),
                            g: f64::from(DesignTokens::background()[1]),
                            b: f64::from(DesignTokens::background()[2]),
                            a: f64::from(DesignTokens::background()[3]),
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.shape_pipeline);
            pass.set_vertex_buffer(0, vertex_buffer.slice(..));
            pass.draw(0..vertices.len() as u32, 0..1);
            self.text_renderer
                .render(&self.atlas, &self.viewport, &mut pass)
                .context("stage=wgpu.text.render target=glyphon retryable=true")?;
        }
        self.queue.submit(Some(encoder.finish()));
        self.queue.present(frame);
        self.atlas.trim();

        Ok(PresentObservation {
            width: self.config.width,
            height: self.config.height,
            input_generation: model.input_generation,
            adapter_name: self.adapter_name.clone(),
        })
    }
}

fn set_text(
    buffer: &mut TextBuffer,
    font_system: &mut FontSystem,
    text: &str,
    width: f32,
    height: f32,
) {
    buffer.set_size(Some(width.max(1.0)), Some(height.max(1.0)));
    buffer.set_text(
        text,
        &Attrs::new().family(Family::Monospace),
        Shaping::Advanced,
        None,
    );
    buffer.shape_until_scroll(font_system, false);
}

#[allow(clippy::too_many_arguments)]
fn text_area<'a>(
    buffer: &'a TextBuffer,
    scale: f32,
    left: f32,
    top: f32,
    bound_left: i32,
    bound_top: i32,
    bound_right: i32,
    bound_bottom: i32,
    color: TextColor,
) -> TextArea<'a> {
    let scale = normalize_scale(f64::from(scale));
    TextArea {
        buffer,
        left: left * scale,
        top: top * scale,
        scale,
        bounds: TextBounds {
            left: (bound_left as f32 * scale).round() as i32,
            top: (bound_top as f32 * scale).round() as i32,
            right: (bound_right as f32 * scale).round() as i32,
            bottom: (bound_bottom as f32 * scale).round() as i32,
        },
        default_color: color,
        custom_glyphs: &[],
    }
}

fn pane_color(focused: bool) -> [f32; 4] {
    if focused {
        DesignTokens::pane_active()
    } else {
        DesignTokens::surface()
    }
}

fn srgb_color(rgb: [u8; 3]) -> [f32; 4] {
    [
        srgb_to_linear(rgb[0]),
        srgb_to_linear(rgb[1]),
        srgb_to_linear(rgb[2]),
        1.0,
    ]
}

fn srgb_to_linear(channel: u8) -> f32 {
    let value = f32::from(channel) / 255.0;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

#[allow(clippy::too_many_arguments)]
fn push_rect(
    vertices: &mut Vec<Vertex>,
    surface_width: f32,
    surface_height: f32,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color: [f32; 4],
) {
    let left = x / surface_width * 2.0 - 1.0;
    let right = (x + width) / surface_width * 2.0 - 1.0;
    let top = 1.0 - y / surface_height * 2.0;
    let bottom = 1.0 - (y + height) / surface_height * 2.0;
    vertices.extend_from_slice(&[
        Vertex {
            position: [left, top],
            color,
        },
        Vertex {
            position: [left, bottom],
            color,
        },
        Vertex {
            position: [right, bottom],
            color,
        },
        Vertex {
            position: [left, top],
            color,
        },
        Vertex {
            position: [right, bottom],
            color,
        },
        Vertex {
            position: [right, top],
            color,
        },
    ]);
}

#[cfg(test)]
mod tests {
    use glyphon::{Buffer as TextBuffer, FontSystem, Metrics};

    use super::{DesignTokens, RenderGeometry, srgb_to_linear, text_area};

    #[test]
    fn srgb_tokens_are_converted_to_linear_shader_values() {
        assert!((srgb_to_linear(0) - 0.0).abs() < f32::EPSILON);
        assert!((srgb_to_linear(10) - 0.003_035_27).abs() < 0.000_001);
        assert!((srgb_to_linear(128) - 0.215_860_5).abs() < 0.000_001);
        assert!(srgb_to_linear(25) < 0.01);
        assert!(srgb_to_linear(56) < 0.05);
    }

    #[test]
    fn design_surfaces_keep_alpha_and_ordered_darkness() {
        let background = DesignTokens::background();
        let navigator = DesignTokens::navigator();
        let surface = DesignTokens::surface();
        let pane_active = DesignTokens::pane_active();
        let selection = DesignTokens::selection_surface();
        assert_eq!(background[3], 1.0);
        assert_eq!(navigator[3], 1.0);
        assert_eq!(surface[3], 1.0);
        assert_eq!(pane_active[3], 1.0);
        assert_eq!(selection[3], 1.0);
        assert!(background[0] < navigator[0]);
        assert!(navigator[0] < surface[0]);
        assert!(surface[0] < pane_active[0]);
        assert!(pane_active[0] < selection[0]);
    }

    #[test]
    fn retina_projection_preserves_logical_geometry_and_text_metrics() {
        let scale_one = RenderGeometry::from_physical(1180, 720, 1.0);
        let scale_two = RenderGeometry::from_physical(2360, 1440, 2.0);

        assert_eq!(scale_one.logical, scale_two.logical);
        assert_eq!(
            scale_one.logical.navigator_width,
            scale_two.logical.navigator_width
        );
        assert_eq!(scale_one.logical.tab_height, scale_two.logical.tab_height);
        assert_eq!(
            scale_one.logical.status_height,
            scale_two.logical.status_height
        );
        assert_eq!(scale_one.logical.split_x, scale_two.logical.split_x);

        let split_one = scale_one.project(scale_one.logical.split_x as f32);
        let split_two = scale_two.project(scale_two.logical.split_x as f32);
        assert!((split_one - split_two / 2.0).abs() < f32::EPSILON);
        assert!(
            (scale_one.project(scale_one.logical.tab_height as f32)
                - scale_two.project(scale_two.logical.tab_height as f32) / 2.0)
                .abs()
                < f32::EPSILON
        );

        let mut font_system = FontSystem::new();
        let buffer = TextBuffer::new(&mut font_system, Metrics::new(14.0, 21.0));
        let one = text_area(
            &buffer,
            scale_one.scale_factor,
            22.0,
            22.0,
            0,
            0,
            400,
            300,
            DesignTokens::text_primary(),
        );
        let two = text_area(
            &buffer,
            scale_two.scale_factor,
            22.0,
            22.0,
            0,
            0,
            400,
            300,
            DesignTokens::text_primary(),
        );
        assert_eq!(one.left, two.left / 2.0);
        assert_eq!(one.top, two.top / 2.0);
        assert_eq!(one.bounds.right, two.bounds.right / 2);
        assert_eq!(one.bounds.bottom, two.bounds.bottom / 2);
        assert_eq!(one.scale, 1.0);
        assert_eq!(two.scale, 2.0);
    }
}

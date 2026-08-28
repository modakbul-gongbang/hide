use std::ptr::NonNull;

use anyhow::{Context, Result, anyhow};
use bytemuck::{Pod, Zeroable};
use glyphon::{
    Attrs, Buffer as TextBuffer, Cache, Color as TextColor, Family, FontSystem, Metrics,
    Resolution, Shaping, SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport,
};
use raw_window_handle::{AppKitWindowHandle, RawWindowHandle};
use wgpu::util::DeviceExt;

use crate::layout::PaneId;

#[derive(Clone, Debug)]
pub struct FrameModel {
    pub zoomed: Option<PaneId>,
    pub focused: PaneId,
    pub terminal_status: String,
    pub terminal_text: String,
    pub editor_text: String,
    pub ime_marked: String,
    pub ime_committed: String,
    pub input_generation: u64,
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

pub struct Renderer {
    _instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
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
    pub unsafe fn new(ns_view: NonNull<std::ffi::c_void>, width: u32, height: u32) -> Result<Self> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::METAL;
        let instance = wgpu::Instance::new(descriptor);
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: None,
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
        let navigator_text = TextBuffer::new(&mut font_system, Metrics::new(14.0, 22.0));
        let tab_text = TextBuffer::new(&mut font_system, Metrics::new(13.0, 20.0));
        let primary_text = TextBuffer::new(&mut font_system, Metrics::new(14.0, 21.0));
        let secondary_text = TextBuffer::new(&mut font_system, Metrics::new(14.0, 21.0));
        let status_text = TextBuffer::new(&mut font_system, Metrics::new(12.0, 18.0));

        Ok(Self {
            _instance: instance,
            surface,
            device,
            queue,
            config,
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

    pub fn resize(&mut self, width: u32, height: u32) {
        let width = width.max(1);
        let height = height.max(1);
        if self.config.width == width && self.config.height == height {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    pub fn render(&mut self, model: &FrameModel) -> Result<PresentObservation> {
        let width = self.config.width as f32;
        let height = self.config.height as f32;
        let navigator_width = (width * 0.22).clamp(190.0, 260.0);
        let tab_height = 46.0;
        let status_height = 30.0;
        let canvas_left = navigator_width + 1.0;
        let canvas_top = tab_height;
        let canvas_bottom = height - status_height;
        let canvas_width = width - canvas_left;
        let canvas_height = canvas_bottom - canvas_top;

        let mut vertices = Vec::with_capacity(48);
        push_rect(
            &mut vertices,
            width,
            height,
            0.0,
            0.0,
            width,
            height,
            [0.035, 0.043, 0.055, 1.0],
        );
        push_rect(
            &mut vertices,
            width,
            height,
            0.0,
            0.0,
            navigator_width,
            height,
            [0.055, 0.066, 0.082, 1.0],
        );
        push_rect(
            &mut vertices,
            width,
            height,
            canvas_left,
            0.0,
            canvas_width,
            tab_height,
            [0.072, 0.082, 0.102, 1.0],
        );
        push_rect(
            &mut vertices,
            width,
            height,
            canvas_left,
            canvas_bottom,
            canvas_width,
            status_height,
            [0.048, 0.058, 0.072, 1.0],
        );

        let split = canvas_left + canvas_width * 0.55;
        if model.zoomed.is_none() {
            push_rect(
                &mut vertices,
                width,
                height,
                canvas_left + 8.0,
                canvas_top + 8.0,
                split - canvas_left - 12.0,
                canvas_height - 16.0,
                pane_color(model.focused == PaneId::TerminalA),
            );
            push_rect(
                &mut vertices,
                width,
                height,
                split + 4.0,
                canvas_top + 8.0,
                width - split - 12.0,
                canvas_height - 16.0,
                pane_color(model.focused == PaneId::Editor),
            );
        } else {
            push_rect(
                &mut vertices,
                width,
                height,
                canvas_left + 8.0,
                canvas_top + 8.0,
                canvas_width - 16.0,
                canvas_height - 16.0,
                pane_color(true),
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
            .map(|pane| format!("ZOOMED: {pane:?}"))
            .unwrap_or_else(|| "Split layout".to_owned());
        set_text(
            &mut self.navigator_text,
            &mut self.font_system,
            "HERDR IDE\n\nWORKSPACES\n● local\n  native-spike\n\nAGENTS\n  terminal / PTY\n\nWORKTREES\n  rust-native",
            navigator_width - 28.0,
            height - 28.0,
        );
        set_text(
            &mut self.tab_text,
            &mut self.font_system,
            &format!("native-spike    {zoom_label}    Cmd+Shift+Enter"),
            canvas_width - 28.0,
            tab_height - 8.0,
        );
        let terminal = format!(
            "TERMINAL A  [{}]\n{}\n\nIME marked: {}\nIME committed: {}",
            model.terminal_status,
            model.terminal_text,
            empty_marker(&model.ime_marked),
            empty_marker(&model.ime_committed),
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
            "EDITOR B  [WGPU text]\n\n{}\n\nClick this pane, then type Korean text.\nCmd+Shift+Enter zooms without changing topology.",
            model.editor_text
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
                "AppKit lifecycle  •  WGPU Metal pixels  •  real PTY  •  Retina {}x{}  •  AX tree",
                self.config.width, self.config.height
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
                16.0,
                22.0,
                0,
                0,
                navigator_width as i32,
                height as i32,
                TextColor::rgb(203, 213, 225),
            ),
            text_area(
                &self.tab_text,
                canvas_left + 16.0,
                13.0,
                canvas_left as i32,
                0,
                width as i32,
                tab_height as i32,
                TextColor::rgb(226, 232, 240),
            ),
            text_area(
                &self.status_text,
                canvas_left + 12.0,
                canvas_bottom + 6.0,
                canvas_left as i32,
                canvas_bottom as i32,
                width as i32,
                height as i32,
                TextColor::rgb(148, 163, 184),
            ),
        ];
        match model.zoomed {
            None => {
                areas.push(text_area(
                    &self.primary_text,
                    canvas_left + 22.0,
                    canvas_top + 22.0,
                    canvas_left as i32,
                    canvas_top as i32,
                    split as i32,
                    canvas_bottom as i32,
                    TextColor::rgb(226, 232, 240),
                ));
                areas.push(text_area(
                    &self.secondary_text,
                    split + 18.0,
                    canvas_top + 22.0,
                    split as i32,
                    canvas_top as i32,
                    width as i32,
                    canvas_bottom as i32,
                    TextColor::rgb(226, 232, 240),
                ));
            }
            Some(PaneId::Editor) => {
                areas.push(text_area(
                    &self.secondary_text,
                    canvas_left + 22.0,
                    canvas_top + 22.0,
                    canvas_left as i32,
                    canvas_top as i32,
                    width as i32,
                    canvas_bottom as i32,
                    TextColor::rgb(226, 232, 240),
                ));
            }
            Some(_) => {
                areas.push(text_area(
                    &self.primary_text,
                    canvas_left + 22.0,
                    canvas_top + 22.0,
                    canvas_left as i32,
                    canvas_top as i32,
                    width as i32,
                    canvas_bottom as i32,
                    TextColor::rgb(226, 232, 240),
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
                            r: 0.035,
                            g: 0.043,
                            b: 0.055,
                            a: 1.0,
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
    left: f32,
    top: f32,
    bound_left: i32,
    bound_top: i32,
    bound_right: i32,
    bound_bottom: i32,
    color: TextColor,
) -> TextArea<'a> {
    TextArea {
        buffer,
        left,
        top,
        scale: 1.0,
        bounds: TextBounds {
            left: bound_left,
            top: bound_top,
            right: bound_right,
            bottom: bound_bottom,
        },
        default_color: color,
        custom_glyphs: &[],
    }
}

fn pane_color(focused: bool) -> [f32; 4] {
    if focused {
        [0.075, 0.091, 0.116, 1.0]
    } else {
        [0.056, 0.067, 0.086, 1.0]
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

fn empty_marker(value: &str) -> &str {
    if value.is_empty() { "none" } else { value }
}

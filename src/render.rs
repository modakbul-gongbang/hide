use std::ptr::NonNull;

use alacritty_terminal::vte::ansi::CursorShape;
use anyhow::{Context, Result, anyhow};
use bytemuck::{Pod, Zeroable};
use glyphon::{
    Attrs, Buffer as TextBuffer, Cache, Color as TextColor, Family, FontSystem, Metrics,
    Resolution, Shaping, Style, SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer,
    Viewport, Weight,
};
use raw_window_handle::{
    AppKitDisplayHandle, AppKitWindowHandle, RawDisplayHandle, RawWindowHandle,
};
use wgpu::util::DeviceExt;

use crate::layout::{CanvasGeometry, PaneId};
use crate::terminal::TerminalView;

#[derive(Clone, Debug)]
pub struct FrameModel {
    pub navigator_text: String,
    pub overlay_text: String,
    pub tab_text: String,
    pub connection_text: String,
    pub zoomed: Option<PaneId>,
    pub focused: PaneId,
    pub terminal: TerminalView,
    pub editor_text: String,
    pub ime_marked: String,
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
    overlay_text: TextBuffer,
    tab_text: TextBuffer,
    primary_text: TextBuffer,
    secondary_text: TextBuffer,
    status_text: TextBuffer,
    terminal_alert_text: TextBuffer,
}

impl Renderer {
    /// The caller keeps the NSView alive until after this renderer is dropped.
    pub unsafe fn new(ns_view: NonNull<std::ffi::c_void>, width: u32, height: u32) -> Result<Self> {
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
            label: Some("Herdr IDE WGPU device"),
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
            label: Some("Herdr IDE shape pipeline"),
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
        let overlay_text = TextBuffer::new(&mut font_system, Metrics::new(13.0, 20.0));
        let tab_text = TextBuffer::new(&mut font_system, Metrics::new(13.0, 20.0));
        let primary_text = TextBuffer::new(&mut font_system, Metrics::new(14.0, 21.0));
        let secondary_text = TextBuffer::new(&mut font_system, Metrics::new(14.0, 21.0));
        let status_text = TextBuffer::new(&mut font_system, Metrics::new(12.0, 18.0));
        let terminal_alert_text = TextBuffer::new(&mut font_system, Metrics::new(12.0, 18.0));

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
            overlay_text,
            tab_text,
            primary_text,
            secondary_text,
            status_text,
            terminal_alert_text,
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
        let geometry = CanvasGeometry::for_size(width as f64, height as f64);
        let navigator_width = geometry.navigator_width as f32;
        let tab_height = geometry.tab_height as f32;
        let status_height = geometry.status_height as f32;
        let canvas_left = geometry.canvas_left as f32;
        let canvas_top = geometry.canvas_top as f32;
        let canvas_bottom = geometry.canvas_bottom as f32;
        let canvas_width = geometry.canvas_width as f32;
        let canvas_height = geometry.canvas_height as f32;
        let terminal_surface = geometry
            .pane_rect(PaneId::TerminalA, model.zoomed)
            .unwrap_or_else(|| geometry.canvas_rect());
        let editor_surface = geometry
            .pane_rect(PaneId::Editor, model.zoomed)
            .unwrap_or_else(|| geometry.canvas_rect());

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

        for node in geometry.layout_nodes(model.zoomed) {
            let focused = model.focused == node.pane_id
                || (matches!(node.pane_id, PaneId::TerminalA | PaneId::TerminalB)
                    && matches!(model.focused, PaneId::TerminalA | PaneId::TerminalB));
            push_rect(
                &mut vertices,
                width,
                height,
                node.rect.x as f32 + 8.0,
                node.rect.y as f32 + 8.0,
                (node.rect.width - 16.0).max(0.0) as f32,
                (node.rect.height - 16.0).max(0.0) as f32,
                pane_color(focused),
            );
        }

        if !model.overlay_text.is_empty() {
            let overlay_width = (canvas_width - 48.0).max(260.0).min(560.0);
            let overlay_height = (canvas_height - 48.0).max(160.0).min(420.0);
            push_rect(
                &mut vertices,
                width,
                height,
                canvas_left + (canvas_width - overlay_width) / 2.0,
                canvas_top + (canvas_height - overlay_height) / 2.0,
                overlay_width,
                overlay_height,
                [0.095, 0.11, 0.15, 0.98],
            );
        }

        let terminal_rect = geometry.terminal_rect(model.zoomed).map(|rect| {
            (
                rect.x as f32,
                rect.y as f32,
                rect.width as f32,
                rect.height as f32,
            )
        });
        if let Some((terminal_x, terminal_y, terminal_width, terminal_height)) = terminal_rect {
            push_terminal_backgrounds(
                &mut vertices,
                width,
                height,
                &model.terminal,
                terminal_x,
                terminal_y,
                terminal_width,
                terminal_height,
            );
        }

        let vertex_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Herdr IDE UI rectangles"),
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
            &model.navigator_text,
            navigator_width - 28.0,
            height - 28.0,
        );
        let overlay_width = (canvas_width - 48.0).max(260.0).min(560.0);
        let overlay_height = (canvas_height - 48.0).max(160.0).min(420.0);
        set_text(
            &mut self.overlay_text,
            &mut self.font_system,
            &model.overlay_text,
            overlay_width - 28.0,
            overlay_height - 24.0,
        );
        set_text(
            &mut self.tab_text,
            &mut self.font_system,
            &format!("{}    {zoom_label}", model.tab_text),
            canvas_width - 28.0,
            tab_height - 8.0,
        );
        set_terminal_text(
            &mut self.primary_text,
            &mut self.font_system,
            &model.terminal,
            &model.ime_marked,
            terminal_rect.map(|rect| rect.2).unwrap_or(1.0),
            terminal_rect.map(|rect| rect.3).unwrap_or(1.0),
        );
        set_text(
            &mut self.terminal_alert_text,
            &mut self.font_system,
            model.terminal.failure.as_deref().unwrap_or(""),
            terminal_rect.map(|rect| rect.2).unwrap_or(1.0),
            24.0,
        );
        let editor = format!(
            "{}{}",
            model.editor_text,
            if model.focused == PaneId::Editor && !model.ime_marked.is_empty() {
                format!("{}", model.ime_marked)
            } else {
                String::new()
            }
        );
        set_text(
            &mut self.secondary_text,
            &mut self.font_system,
            &editor,
            (editor_surface.width - 36.0).max(1.0) as f32,
            canvas_height - 34.0,
        );
        set_text(
            &mut self.status_text,
            &mut self.font_system,
            &format!(
                "{}  •  {}  •  Scrollback {}/{}",
                model.connection_text,
                model.terminal.title.as_deref().unwrap_or("Terminal"),
                model.terminal.display_offset,
                model.terminal.scrollback_lines,
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
        if !model.overlay_text.is_empty() {
            areas.push(text_area(
                &self.overlay_text,
                canvas_left + (canvas_width - overlay_width) / 2.0 + 14.0,
                canvas_top + (canvas_height - overlay_height) / 2.0 + 14.0,
                canvas_left as i32,
                canvas_top as i32,
                (canvas_left + canvas_width) as i32,
                canvas_bottom as i32,
                TextColor::rgb(226, 232, 240),
            ));
        }
        match model.zoomed {
            None => {
                areas.push(text_area(
                    &self.primary_text,
                    terminal_surface.x as f32 + 22.0,
                    terminal_surface.y as f32 + 22.0,
                    terminal_surface.x as i32,
                    terminal_surface.y as i32,
                    (terminal_surface.x + terminal_surface.width) as i32,
                    (terminal_surface.y + terminal_surface.height) as i32,
                    TextColor::rgb(226, 232, 240),
                ));
                if model.terminal.failure.is_some() {
                    areas.push(text_area(
                        &self.terminal_alert_text,
                        terminal_surface.x as f32 + 28.0,
                        terminal_surface.y as f32 + terminal_surface.height as f32 - 34.0,
                        terminal_surface.x as i32,
                        terminal_surface.y as i32,
                        (terminal_surface.x + terminal_surface.width) as i32,
                        (terminal_surface.y + terminal_surface.height) as i32,
                        TextColor::rgb(248, 113, 113),
                    ));
                }
                areas.push(text_area(
                    &self.secondary_text,
                    editor_surface.x as f32 + 18.0,
                    editor_surface.y as f32 + 22.0,
                    editor_surface.x as i32,
                    editor_surface.y as i32,
                    (editor_surface.x + editor_surface.width) as i32,
                    (editor_surface.y + editor_surface.height) as i32,
                    TextColor::rgb(226, 232, 240),
                ));
            }
            Some(PaneId::Editor) => {
                areas.push(text_area(
                    &self.secondary_text,
                    editor_surface.x as f32 + 22.0,
                    editor_surface.y as f32 + 22.0,
                    editor_surface.x as i32,
                    editor_surface.y as i32,
                    (editor_surface.x + editor_surface.width) as i32,
                    (editor_surface.y + editor_surface.height) as i32,
                    TextColor::rgb(226, 232, 240),
                ));
            }
            Some(_) => {
                areas.push(text_area(
                    &self.primary_text,
                    terminal_surface.x as f32 + 22.0,
                    terminal_surface.y as f32 + 22.0,
                    terminal_surface.x as i32,
                    terminal_surface.y as i32,
                    (terminal_surface.x + terminal_surface.width) as i32,
                    (terminal_surface.y + terminal_surface.height) as i32,
                    TextColor::rgb(226, 232, 240),
                ));
                if model.terminal.failure.is_some() {
                    areas.push(text_area(
                        &self.terminal_alert_text,
                        terminal_surface.x as f32 + 28.0,
                        terminal_surface.y as f32 + terminal_surface.height as f32 - 34.0,
                        terminal_surface.x as i32,
                        terminal_surface.y as i32,
                        (terminal_surface.x + terminal_surface.width) as i32,
                        (terminal_surface.y + terminal_surface.height) as i32,
                        TextColor::rgb(248, 113, 113),
                    ));
                }
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
                label: Some("Herdr IDE frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Herdr IDE render pass"),
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

fn set_terminal_text(
    buffer: &mut TextBuffer,
    font_system: &mut FontSystem,
    terminal: &TerminalView,
    marked_text: &str,
    width: f32,
    height: f32,
) {
    buffer.set_size(Some(width.max(1.0)), Some(height.max(1.0)));
    let default = Attrs::new().family(Family::Monospace);
    let mut spans = Vec::with_capacity(terminal.cells.len() + terminal.rows);
    for row in 0..terminal.rows {
        for column in 0..terminal.columns {
            let Some(cell) = terminal.cell(row, column) else {
                continue;
            };
            let cursor = terminal
                .cursor
                .is_some_and(|cursor| cursor.row == row && cursor.column == column);
            let mut color = cell.foreground;
            if cell.selected || cursor {
                color = [248, 250, 252];
            }
            let mut attrs = Attrs::new()
                .family(Family::Monospace)
                .color(TextColor::rgb(color[0], color[1], color[2]));
            if cell.bold {
                attrs = attrs.weight(Weight::BOLD);
            }
            if cell.italic {
                attrs = attrs.style(Style::Italic);
            }
            if cell.underlined || cell.hyperlink.is_some() {
                attrs.text_decoration.underline = glyphon::cosmic_text::UnderlineStyle::Single;
            }
            let text = if cursor && !marked_text.is_empty() {
                attrs = attrs.color(TextColor::rgb(103, 232, 249));
                attrs.text_decoration.underline = glyphon::cosmic_text::UnderlineStyle::Single;
                marked_text.to_owned()
            } else if cell.spacer {
                String::new()
            } else {
                cell.text.clone()
            };
            spans.push((text, attrs));
        }
        if row + 1 < terminal.rows {
            spans.push(("\n".to_owned(), default.clone()));
        }
    }
    buffer.set_rich_text(
        spans
            .iter()
            .map(|(text, attrs)| (text.as_str(), attrs.clone())),
        &default,
        Shaping::Advanced,
        None,
    );
    buffer.shape_until_scroll(font_system, false);
}

#[allow(clippy::too_many_arguments)]
fn push_terminal_backgrounds(
    vertices: &mut Vec<Vertex>,
    surface_width: f32,
    surface_height: f32,
    terminal: &TerminalView,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) {
    if terminal.columns == 0 || terminal.rows == 0 {
        return;
    }
    let cell_width = width / terminal.columns as f32;
    let cell_height = height / terminal.rows as f32;
    for row in 0..terminal.rows {
        for column in 0..terminal.columns {
            let Some(cell) = terminal.cell(row, column) else {
                continue;
            };
            let cursor = terminal
                .cursor
                .filter(|cursor| cursor.row == row && cursor.column == column);
            let color = if cursor.is_some() {
                [0.32, 0.42, 0.56, 1.0]
            } else if cell.selected {
                [0.12, 0.30, 0.55, 1.0]
            } else {
                [
                    cell.background[0] as f32 / 255.0,
                    cell.background[1] as f32 / 255.0,
                    cell.background[2] as f32 / 255.0,
                    1.0,
                ]
            };
            if cursor.is_none() && (cell.selected || cell.background != [12, 16, 24]) {
                push_rect(
                    vertices,
                    surface_width,
                    surface_height,
                    x + column as f32 * cell_width,
                    y + row as f32 * cell_height,
                    cell_width.max(1.0),
                    cell_height.max(1.0),
                    color,
                );
            }
            if let Some(cursor) = cursor {
                let cell_x = x + column as f32 * cell_width;
                let cell_y = y + row as f32 * cell_height;
                match cursor.shape {
                    CursorShape::Block => push_rect(
                        vertices,
                        surface_width,
                        surface_height,
                        cell_x,
                        cell_y,
                        cell_width.max(1.0),
                        cell_height.max(1.0),
                        color,
                    ),
                    CursorShape::Beam => push_rect(
                        vertices,
                        surface_width,
                        surface_height,
                        cell_x,
                        cell_y,
                        2.0,
                        cell_height.max(1.0),
                        color,
                    ),
                    CursorShape::Underline => push_rect(
                        vertices,
                        surface_width,
                        surface_height,
                        cell_x,
                        cell_y + cell_height - 2.0,
                        cell_width.max(1.0),
                        2.0,
                        color,
                    ),
                    CursorShape::HollowBlock => {
                        push_rect(
                            vertices,
                            surface_width,
                            surface_height,
                            cell_x,
                            cell_y,
                            cell_width.max(1.0),
                            1.0,
                            color,
                        );
                        push_rect(
                            vertices,
                            surface_width,
                            surface_height,
                            cell_x,
                            cell_y + cell_height - 1.0,
                            cell_width.max(1.0),
                            1.0,
                            color,
                        );
                        push_rect(
                            vertices,
                            surface_width,
                            surface_height,
                            cell_x,
                            cell_y,
                            1.0,
                            cell_height.max(1.0),
                            color,
                        );
                        push_rect(
                            vertices,
                            surface_width,
                            surface_height,
                            cell_x + cell_width - 1.0,
                            cell_y,
                            1.0,
                            cell_height.max(1.0),
                            color,
                        );
                    }
                    CursorShape::Hidden => {}
                }
            }
            if cell.underlined || cell.hyperlink.is_some() {
                push_rect(
                    vertices,
                    surface_width,
                    surface_height,
                    x + column as f32 * cell_width,
                    y + (row + 1) as f32 * cell_height - 1.0,
                    cell_width.max(1.0),
                    1.0,
                    [0.40, 0.78, 0.94, 1.0],
                );
            }
        }
    }
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

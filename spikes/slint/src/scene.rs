//! What the composition shows, from omareel_core into the `Comp` global: the project's
//! settings once, then each frame's state (the editor's preview variant or the exporter's), and
//! the pictures (video frames as wgpu textures or CPU buffers, and the camera's blurred copy).

use crate::{
    BorderSettings, CameraSettings, CameraShape, Comp, Corner, CropRect, CursorSettings, CursorState, CursorStyle,
    FrameSettings, FullFrame, KeysPosition, KeystrokeSettings, LayoutState, PillState, RippleState, ShadowSettings,
    ZoomState,
};
use omareel_core::compat::qcolor::QColor;
use omareel_core::keystroke_track::KeystrokePill;
use omareel_core::motion_track::RippleSample;
use omareel_core::project::{Border, Project, Shadow};
use slint::wgpu_30::wgpu;
use slint::{Brush, Color, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel};
use std::rc::Rc;

fn color(c: &QColor) -> Color {
    Color::from_argb_u8(c.alpha() as u8, c.red() as u8, c.green() as u8, c.blue() as u8)
}

fn shadow(s: &Shadow) -> ShadowSettings {
    ShadowSettings {
        enabled: s.enabled,
        intensity: s.intensity as f32,
        blur: s.blur as f32,
        distance: s.distance as f32,
        angle: s.angle as f32,
    }
}

fn border(b: &Border) -> BorderSettings {
    BorderSettings { enabled: b.enabled, width: b.width as f32, color: color(&b.color), alpha: b.alpha as f32 }
}

/// The recording's facts the composition needs.
pub struct Recording {
    pub source_width: i32,
    pub source_height: i32,
    pub capture_scale: f64,
    pub camera: Option<(i32, i32)>,
}

/// The wallpaper or image behind the frame, decoded once.
pub fn load_background(path: &str) -> Option<Image> {
    if path.is_empty() {
        return None;
    }
    let decoded = match image::open(path) {
        Ok(decoded) => decoded.into_rgba8(),
        Err(error) => {
            eprintln!("background {path}: {error}");
            return None;
        }
    };
    let (w, h) = decoded.dimensions();
    let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(decoded.as_raw(), w, h);
    Some(Image::from_rgba8(buffer))
}

/// The project map's part the composition reads (`comp.project`, `renderTheme`) and the
/// recording's facts.
pub fn apply_project(comp: &Comp<'_>, project: &Project, recording: &Recording, background: Option<&Image>) {
    let bg = &project.background;
    comp.set_no_background(bg.type_ == "none");
    let fill = if bg.type_ == "gradient" && bg.gradient.stops.len() >= 2 {
        // Background.qml's canvas gradient runs along (angle - 90°) through the centre, as
        // Slint's linear gradient does for `angle`.
        // Slint's public API has `Brush::LinearGradient` but not the types to build one; the
        // generated code's private re-exports have them.
        use slint::private_unstable_api::re_exports::{GradientStop, LinearGradientBrush};
        let stops = bg.gradient.stops.iter().map(|s| GradientStop { color: color(&s.color), position: s.position as f32 });
        Brush::LinearGradient(LinearGradientBrush::new(bg.gradient.angle as f32, stops))
    } else {
        Brush::SolidColor(if bg.color.is_valid() { color(&bg.color) } else { Color::from_rgb_u8(26, 27, 38) })
    };
    comp.set_background_fill(fill);
    let show_image = (bg.type_ == "image" || bg.type_ == "wallpaper") && background.is_some();
    comp.set_has_background_image(show_image);
    comp.set_background_image(background.cloned().unwrap_or_default());
    let frame = &project.frame;
    comp.set_frame(FrameSettings {
        padding: frame.padding as f32,
        radius: frame.radius as f32,
        shadow: shadow(&frame.shadow),
        border: border(&frame.border),
    });
    let crop = &project.crop;
    comp.set_crop(CropRect { x: crop.x as f32, y: crop.y as f32, w: crop.width as f32, h: crop.height as f32 });
    let camera = &project.camera;
    comp.set_camera(CameraSettings {
        shape: match camera.shape.as_str() {
            "square" => CameraShape::Square,
            "rectangle" => CameraShape::Rectangle,
            _ => CameraShape::Round,
        },
        full_frame: if camera.full_frame == "square" { FullFrame::Square } else { FullFrame::Fill },
        position: match camera.position.as_str() {
            "top-left" => Corner::TopLeft,
            "top-right" => Corner::TopRight,
            "bottom-left" => Corner::BottomLeft,
            _ => Corner::BottomRight,
        },
        size: camera.size as f32,
        radius: camera.radius as f32,
        flip_horizontal: camera.flip_horizontal,
        rotation: camera.rotation,
        shadow: shadow(&camera.shadow),
        border: border(&camera.border),
        scale_during_zoom: camera.scale_during_zoom as f32,
        offset_x: camera.offset.x as f32,
        offset_y: camera.offset.y as f32,
    });
    let cursor = &project.cursor;
    comp.set_cursor_settings(CursorSettings {
        visible: cursor.visible,
        size: cursor.size as f32,
        style: match cursor.style.as_str() {
            "dark-arrow" => CursorStyle::DarkArrow,
            "hand" => CursorStyle::Hand,
            "dot" => CursorStyle::Dot,
            _ => CursorStyle::LightArrow,
        },
        ring_color: color(&cursor.ring_color),
    });
    let keys = &project.keystrokes;
    comp.set_keystrokes(KeystrokeSettings {
        enabled: keys.enabled,
        position: match keys.position.as_str() {
            "top-left" => KeysPosition::TopLeft,
            "top-center" => KeysPosition::TopCenter,
            "top-right" => KeysPosition::TopRight,
            "bottom-left" => KeysPosition::BottomLeft,
            "bottom-right" => KeysPosition::BottomRight,
            _ => KeysPosition::BottomCenter,
        },
        size: keys.size as f32,
    });
    let (background_color, foreground_color) = omareel_core::theme::render_colors();
    comp.set_theme_background(color(&background_color));
    comp.set_theme_foreground(color(&foreground_color));
    if let Some(family) = omareel_core::theme::monospace_family() {
        comp.set_mono_family(family.into());
    }
    comp.set_source_width(recording.source_width);
    comp.set_source_height(recording.source_height);
    comp.set_capture_scale(recording.capture_scale as f32);
    comp.set_camera_available(recording.camera.is_some());
    let (camera_width, camera_height) = recording.camera.unwrap_or((0, 0));
    comp.set_camera_source_width(camera_width);
    comp.set_camera_source_height(camera_height);
}

/// One frame's state, from either the editor's `PreviewComposition` or the exporter's
/// `CompositionState`.
pub struct FrameState<'a> {
    pub zoom: (f64, f64, f64, f64),
    pub camera_full: f64,
    pub bubble: f64,
    pub cursor: (f64, f64, bool, f64, f64, f64),
    pub ripples: &'a [RippleSample],
    pub pills: &'a [KeystrokePill],
    pub camera_visible: bool,
}

impl<'a> FrameState<'a> {
    pub fn preview(c: &'a omareel_core::editor::PreviewComposition) -> FrameState<'a> {
        FrameState {
            zoom: (c.zoom.scale, c.zoom.cx, c.zoom.cy, c.zoom.velocity),
            camera_full: c.layout.camera_full,
            bubble: c.layout.bubble,
            cursor: (c.cursor.x, c.cursor.y, c.cursor.visible, c.cursor.scale, c.cursor.opacity, c.cursor.rotation),
            ripples: &c.ripples,
            pills: &c.keystroke_pills,
            camera_visible: c.camera_visible,
        }
    }

    pub fn export(c: &'a omareel_core::export::composition::CompositionState) -> FrameState<'a> {
        FrameState {
            zoom: (c.zoom.scale, c.zoom.cx, c.zoom.cy, c.zoom.velocity),
            camera_full: c.layout.camera_full,
            bubble: c.layout.bubble,
            cursor: (c.cursor.x, c.cursor.y, c.cursor.visible, c.cursor.scale, c.cursor.opacity, c.cursor.rotation),
            ripples: &c.ripples,
            pills: &c.keystroke_pills,
            camera_visible: c.camera_visible,
        }
    }
}

/// The models behind the repeaters, kept so that a frame that changes only values updates rows
/// in place instead of rebuilding the elements.
pub struct Models {
    ripples: Rc<VecModel<RippleState>>,
    pills: Rc<VecModel<PillState>>,
    pill_keys: Vec<Vec<String>>,
}

impl Models {
    pub fn install(comp: &Comp<'_>) -> Models {
        let models = Models { ripples: Rc::new(VecModel::default()), pills: Rc::new(VecModel::default()), pill_keys: Vec::new() };
        comp.set_ripples(ModelRc::from(models.ripples.clone()));
        comp.set_pills(ModelRc::from(models.pills.clone()));
        models
    }

    pub fn apply(&mut self, comp: &Comp<'_>, state: &FrameState<'_>) {
        let (scale, cx, cy, velocity) = state.zoom;
        comp.set_zoom(ZoomState { scale: scale as f32, cx: cx as f32, cy: cy as f32, velocity: velocity as f32 });
        comp.set_layout(LayoutState { camera_full: state.camera_full as f32, bubble: state.bubble as f32 });
        let (x, y, visible, cursor_scale, opacity, rotation) = state.cursor;
        comp.set_cursor(CursorState {
            x: x as f32,
            y: y as f32,
            visible,
            scale: cursor_scale as f32,
            opacity: opacity as f32,
            rotation: rotation as f32,
        });
        comp.set_camera_visible(state.camera_visible);

        let ripples: Vec<RippleState> =
            state.ripples.iter().map(|r| RippleState { x: r.x as f32, y: r.y as f32, progress: r.progress as f32 }).collect();
        if ripples.len() == self.ripples.row_count() {
            for (i, ripple) in ripples.into_iter().enumerate() {
                if self.ripples.row_data(i).as_ref() != Some(&ripple) {
                    self.ripples.set_row_data(i, ripple);
                }
            }
        } else {
            self.ripples.set_vec(ripples);
        }

        let keys: Vec<Vec<String>> = state.pills.iter().map(|p| p.keys.clone()).collect();
        if keys != self.pill_keys {
            self.pill_keys = keys;
            self.pills.set_vec(
                state
                    .pills
                    .iter()
                    .map(|p| PillState {
                        keys: ModelRc::new(VecModel::from(p.keys.iter().map(SharedString::from).collect::<Vec<_>>())),
                        opacity: p.opacity as f32,
                    })
                    .collect::<Vec<_>>(),
            );
        } else {
            for (i, pill) in state.pills.iter().enumerate() {
                if let Some(mut row) = self.pills.row_data(i) {
                    if row.opacity != pill.opacity as f32 {
                        row.opacity = pill.opacity as f32;
                        self.pills.set_row_data(i, row);
                    }
                }
            }
        }
    }
}

/// How decoded frames reach the renderer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Upload {
    /// The preview's decode threads `queue.write_texture` each frame into a ring of wgpu
    /// textures, imported with `Image::try_from(wgpu::Texture)` (Slint's `unstable-wgpu-30`).
    /// The export has no UI thread to spare, so there it is `WgpuUi`.
    Wgpu,
    /// The same on the UI (or export) thread, into one of two textures per video.
    WgpuUi,
    /// A new `SharedPixelBuffer` image per frame, which the renderer uploads itself.
    Cpu,
}

/// One video's textures. Two alternate, so that each frame sets a different image: Slint caches
/// what an image item (and the rounded clip's layer) rendered until its `source` changes, and
/// writing new pixels into the same texture would not tell it.
pub struct VideoTexture {
    textures: Vec<wgpu::Texture>,
    next: usize,
    width: u32,
    height: u32,
}

impl VideoTexture {
    pub fn new(device: &wgpu::Device, width: u32, height: u32, label: &str) -> VideoTexture {
        let textures = (0..2)
            .map(|_| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    // RENDER_ATTACHMENT is required by Image::try_from.
                    usage: wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                })
            })
            .collect();
        VideoTexture { textures, next: 0, width, height }
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn upload(&mut self, queue: &wgpu::Queue, pixels: &[u8]) -> Image {
        let texture = &self.textures[self.next];
        self.next = (self.next + 1) % self.textures.len();
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(self.width * 4), rows_per_image: Some(self.height) },
            wgpu::Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 },
        );
        Image::try_from(texture.clone()).expect("an Rgba8Unorm texture with TEXTURE_BINDING | RENDER_ATTACHMENT")
    }
}

impl VideoTexture {
    /// Records a copy of a staging buffer (rows padded to 256 bytes) into the next texture.
    pub fn copy_from(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, buffer: &wgpu::Buffer) -> Image {
        let texture = &self.textures[self.next];
        self.next = (self.next + 1) % self.textures.len();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame upload") });
        encoder.copy_buffer_to_texture(
            wgpu::TexelCopyBufferInfo {
                buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(crate::video::padded_row(self.width)),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::TexelCopyTextureInfo { texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 },
        );
        queue.submit([encoder.finish()]);
        Image::try_from(texture.clone()).expect("an Rgba8Unorm texture with TEXTURE_BINDING | RENDER_ATTACHMENT")
    }
}

/// A frame as a CPU image.
pub fn cpu_image(pixels: &[u8], width: u32, height: u32) -> Image {
    Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(pixels, width, height))
}

/// Uploads one video's frames by either path, creating its textures on first use.
pub struct VideoLayer {
    label: &'static str,
    texture: Option<VideoTexture>,
}

impl VideoLayer {
    pub fn new(label: &'static str) -> VideoLayer {
        VideoLayer { label, texture: None }
    }

    /// A frame a decode thread staged in a buffer.
    pub fn staged(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, buffer: &wgpu::Buffer, width: u32, height: u32) -> Image {
        if self.texture.as_ref().map(VideoTexture::size) != Some((width, height)) {
            self.texture = Some(VideoTexture::new(device, width, height, self.label));
        }
        self.texture.as_mut().map(|t| t.copy_from(device, queue, buffer)).unwrap_or_default()
    }

    pub fn image(&mut self, upload: Upload, gpu: Option<(&wgpu::Device, &wgpu::Queue)>, pixels: &[u8], width: u32, height: u32) -> Image {
        match (upload, gpu) {
            (Upload::Wgpu | Upload::WgpuUi, Some((device, queue))) => {
                if self.texture.as_ref().map(VideoTexture::size) != Some((width, height)) {
                    self.texture = Some(VideoTexture::new(device, width, height, self.label));
                }
                self.texture.as_mut().map(|t| t.upload(queue, pixels)).unwrap_or_default()
            }
            _ => cpu_image(pixels, width, height),
        }
    }
}

/// BlurredBackdrop.qml's picture: Qt Quick captures the camera (cover-cropped to the overlay's
/// aspect) into a 24x24 texture and blurs it with MultiEffect (blur 1, blurMax 64, saturation
/// 0.1, which MultiEffect counts from 0 = unchanged). Slint has no blur, so this makes the same
/// tiny picture on the CPU: the 24x24 capture, upsampled to 48x48, a Gaussian over it, and the
/// slight saturation boost. Scaling it up to the canvas with bilinear filtering does the rest.
pub fn blurred_camera(pixels: &[u8], width: usize, height: usize, aspect: f64) -> Image {
    const CAPTURE: usize = 24;
    const OUT: usize = 48;
    if width == 0 || height == 0 || pixels.len() < width * height * 4 {
        return Image::default();
    }
    // The cover crop of the camera at `aspect`.
    let source_aspect = width as f64 / height as f64;
    let (cw, ch) = if source_aspect > aspect {
        ((height as f64 * aspect).round() as usize, height)
    } else {
        (width, (width as f64 / aspect).round() as usize)
    };
    let (ox, oy) = ((width - cw.min(width)) / 2, (height - ch.min(height)) / 2);
    // Box-average into the capture.
    let mut capture = [[0f32; 3]; CAPTURE * CAPTURE];
    for (cy, row) in capture.chunks_mut(CAPTURE).enumerate() {
        let y0 = oy + cy * ch / CAPTURE;
        let y1 = (oy + (cy + 1) * ch / CAPTURE).max(y0 + 1);
        for (cx, texel) in row.iter_mut().enumerate() {
            let x0 = ox + cx * cw / CAPTURE;
            let x1 = (ox + (cx + 1) * cw / CAPTURE).max(x0 + 1);
            let mut sum = [0u32; 3];
            // Every other pixel is plenty for a 24x24 average.
            let mut n = 0u32;
            for y in (y0..y1.min(height)).step_by(2) {
                let line = &pixels[(y * width) * 4..(y * width + width) * 4];
                for x in (x0..x1.min(width)).step_by(2) {
                    let p = &line[x * 4..x * 4 + 3];
                    sum[0] += u32::from(p[0]);
                    sum[1] += u32::from(p[1]);
                    sum[2] += u32::from(p[2]);
                    n += 1;
                }
            }
            let n = n.max(1) as f32;
            *texel = [sum[0] as f32 / n, sum[1] as f32 / n, sum[2] as f32 / n];
        }
    }
    // Bilinear upsample to OUT x OUT.
    let mut image = vec![[0f32; 3]; OUT * OUT];
    for y in 0..OUT {
        let fy = ((y as f32 + 0.5) * CAPTURE as f32 / OUT as f32 - 0.5).clamp(0.0, (CAPTURE - 1) as f32);
        let (y0, ty) = (fy.floor() as usize, fy.fract());
        let y1 = (y0 + 1).min(CAPTURE - 1);
        for x in 0..OUT {
            let fx = ((x as f32 + 0.5) * CAPTURE as f32 / OUT as f32 - 0.5).clamp(0.0, (CAPTURE - 1) as f32);
            let (x0, tx) = (fx.floor() as usize, fx.fract());
            let x1 = (x0 + 1).min(CAPTURE - 1);
            for c in 0..3 {
                let top = capture[y0 * CAPTURE + x0][c] * (1.0 - tx) + capture[y0 * CAPTURE + x1][c] * tx;
                let bottom = capture[y1 * CAPTURE + x0][c] * (1.0 - tx) + capture[y1 * CAPTURE + x1][c] * tx;
                image[y * OUT + x][c] = top * (1.0 - ty) + bottom * ty;
            }
        }
    }
    // Separable Gaussian, sigma 1 texel of 48 (edges clamp, as a texture sampler does).
    let sigma = 1.0f32;
    let radius = 3i32;
    let kernel: Vec<f32> = (-radius..=radius).map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp()).collect();
    let total: f32 = kernel.iter().sum();
    let kernel: Vec<f32> = kernel.iter().map(|k| k / total).collect();
    let pass = |src: &[[f32; 3]], horizontal: bool| -> Vec<[f32; 3]> {
        let mut dst = vec![[0f32; 3]; OUT * OUT];
        for y in 0..OUT {
            for x in 0..OUT {
                let mut acc = [0f32; 3];
                for (k, weight) in kernel.iter().enumerate() {
                    let offset = k as i32 - radius;
                    let (sx, sy) = if horizontal {
                        ((x as i32 + offset).clamp(0, OUT as i32 - 1) as usize, y)
                    } else {
                        (x, (y as i32 + offset).clamp(0, OUT as i32 - 1) as usize)
                    };
                    let p = src[sy * OUT + sx];
                    acc[0] += p[0] * weight;
                    acc[1] += p[1] * weight;
                    acc[2] += p[2] * weight;
                }
                dst[y * OUT + x] = acc;
            }
        }
        dst
    };
    let image = pass(&pass(&image, true), false);
    let mut out = SharedPixelBuffer::<Rgba8Pixel>::new(OUT as u32, OUT as u32);
    for (dst, p) in out.make_mut_slice().iter_mut().zip(image.iter()) {
        let luma = 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2];
        let saturate = |c: f32| (luma + (c - luma) * 1.1).round().clamp(0.0, 255.0) as u8;
        *dst = Rgba8Pixel { r: saturate(p[0]), g: saturate(p[1]), b: saturate(p[2]), a: 255 };
    }
    Image::from_rgba8(out)
}

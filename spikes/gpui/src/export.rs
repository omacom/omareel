//! `omareel-gpui export <bundle> -o <file.mp4>`: the composition rendered without a window and
//! piped to ffmpeg. omareel_core's `ExportJob` does everything `omareel export` does except the
//! drawing (probing, the decode thread, the encoder's arguments for the quality, audio, the
//! encode thread, putting the file in place); this draws each frame with GPUI.
//!
//! GPUI has no public offscreen API. What it has is for tests: with its `test-support` feature,
//! `HeadlessAppContext` runs an app on the test platform with a real renderer
//! (`gpui_wgpu::WgpuHeadlessRenderer`), and `Window::render_to_image` draws the last frame's
//! scene into a texture and reads it back.

use crate::media::{bgra_copy, camera_backdrop, render_image};
use crate::preview::{style_for, wallpaper_path};
use crate::scene::{self, Frame, Style};
use gpui::{AppContext, Context, HeadlessAppContext, IntoElement, Render, RenderImage, Window, div, prelude::*, px, size};
use omareel_core::export::{ExportJob, ExportOptions, Image};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

/// The test platform's windows have a scale factor of 2, fixed (`TestWindow`), and
/// `HeadlessAppContext` offers no way to change it: the window is opened at half the output
/// size in logical pixels, which renders at the output size in device pixels.
const SCALE: f64 = 2.0;

struct ExportView {
    style: Style,
    frame: Option<Frame>,
}

impl Render for ExportView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        match &self.frame {
            Some(frame) => div().size_full().child(scene::composition(&self.style, frame, frame.output_width / SCALE)),
            None => div().size_full(),
        }
    }
}

fn to_render_image(image: &Image) -> Arc<RenderImage> {
    render_image(image.width as u32, image.height as u32, bgra_copy(&image.pixels))
}

pub fn run(bundle: &str, output: &str, quality: &str) -> Result<(), String> {
    let total = Instant::now();
    let options = ExportOptions {
        bundle_path: bundle.to_owned(),
        output_path: std::path::absolute(output).map_err(|e| e.to_string())?.to_string_lossy().into_owned(),
        quality: quality.to_owned(),
        ..ExportOptions::default()
    };
    let mut job = ExportJob::prepare(&options, Arc::new(AtomicBool::new(false)))?;
    let (width, height) = (job.width(), job.height());
    let project = job.project().clone();
    let style = style_for(&project);
    let wallpaper = crate::media::load_wallpaper(&wallpaper_path(&project));

    let text_system = Arc::new(gpui_wgpu::CosmicTextSystem::new("IBM Plex Sans"));
    let mut cx = HeadlessAppContext::with_platform(text_system, Arc::new(()), gpui_platform::current_headless_renderer);
    let window = cx
        .open_window(size(px((width as f64 / SCALE) as f32), px((height as f64 / SCALE) as f32)), |_, cx| cx.new(|_| ExportView { style, frame: None }))
        .map_err(|e| e.to_string())?;

    // Frames come top-down from the readback, which is what `software_rendering` means to the
    // encoder's arguments (no vflip).
    job.start(true)?;
    let (mut screen, mut camera, mut backdrop): (Option<Arc<RenderImage>>, Option<Arc<RenderImage>>, Option<Arc<RenderImage>>) =
        (None, None, None);
    let mut retired: Vec<Arc<RenderImage>> = Vec::new();
    // Experiments: leave pictures out to see what they cost.
    let no_screen = std::env::var_os("SPIKE_NO_SCREEN").is_some();
    let wallpaper = if std::env::var_os("SPIKE_NO_WALLPAPER").is_some() { None } else { wallpaper };
    let (mut upload_ns, mut render_ns, mut readback_ns) = (0i64, 0i64, 0i64);
    let square = project.camera.shape != "rectangle";
    let result = (|| -> Result<(), String> {
        for index in 0..job.total_frames() {
            let images = job.next_frame(index)?;
            let upload = Instant::now();
            if let Some(image) = images.screen.as_ref().filter(|_| !no_screen) {
                retired.extend(screen.replace(to_render_image(image)));
            }
            if let Some(image) = &images.camera {
                let picture = to_render_image(image);
                let blurred = camera_backdrop(image.width as usize, image.height as usize, picture.as_bytes(0).unwrap_or(&[]), square);
                retired.extend(camera.replace(picture));
                retired.extend(backdrop.replace(blurred));
            }
            upload_ns += upload.elapsed().as_nanos() as i64;
            let state = job.state();
            let frame = Frame {
                output_width: state.output_width as f64,
                output_height: state.output_height as f64,
                source_width: state.source_width as f64,
                source_height: state.source_height as f64,
                capture_scale: state.capture_scale,
                camera_available: state.camera_available,
                camera_visible: state.camera_visible,
                camera_source_width: state.camera_source_width as f64,
                camera_source_height: state.camera_source_height as f64,
                zoom_scale: state.zoom.scale,
                zoom_cx: state.zoom.cx,
                zoom_cy: state.zoom.cy,
                camera_full: state.layout.camera_full,
                bubble: state.layout.bubble,
                cursor_x: state.cursor.x,
                cursor_y: state.cursor.y,
                cursor_scale: state.cursor.scale,
                cursor_opacity: state.cursor.opacity,
                cursor_rotation: state.cursor.rotation,
                ripples: state.ripples.clone(),
                pills: state.keystroke_pills.clone(),
                screen: screen.clone(),
                camera: camera.clone(),
                camera_blur: backdrop.clone(),
                wallpaper: wallpaper.clone(),
            };
            let render = Instant::now();
            let old = std::mem::take(&mut retired);
            window
                .update(&mut cx, |view, window, cx| {
                    // Pictures the last frame showed and this one replaced leave the atlas.
                    if std::env::var_os("SPIKE_NO_DROP").is_none() {
                        for image in old {
                            let _ = window.drop_image(image);
                        }
                    }
                    view.frame = Some(frame);
                    cx.notify();
                })
                .map_err(|e| e.to_string())?;
            cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx)).map_err(|e| e.to_string())?;
            render_ns += render.elapsed().as_nanos() as i64;
            let readback = Instant::now();
            let picture = cx.capture_screenshot(window.into()).map_err(|e| e.to_string())?;
            if picture.dimensions() != (width as u32, height as u32) {
                return Err(format!("rendered {:?}, expected {width}x{height}", picture.dimensions()));
            }
            readback_ns += readback.elapsed().as_nanos() as i64;
            while !job.wait_encode_space(100) {}
            job.encode(Box::new(picture.into_raw()))?;
        }
        Ok(())
    })();
    job.finish_decoder();
    job.close_encoder(result.is_err());
    while !job.encoder_done() {
        job.wait_encoder_done(100);
    }
    job.join_encoder();
    result?;
    job.finish()?;
    let seconds = total.elapsed().as_secs_f64();
    eprintln!(
        "exported {} frames ({}x{}) in {:.2} s: {:.1} frames/s; per frame: to BGRA + backdrop {:.2} ms, layout+paint {:.2} ms, render+readback {:.2} ms",
        job.total_frames(),
        width,
        height,
        seconds,
        job.total_frames() as f64 / seconds,
        upload_ns as f64 / job.total_frames() as f64 / 1e6,
        render_ns as f64 / job.total_frames() as f64 / 1e6,
        readback_ns as f64 / job.total_frames() as f64 / 1e6,
    );
    eprint!("{}", job.timing_report(upload_ns, render_ns, readback_ns));
    Ok(())
}

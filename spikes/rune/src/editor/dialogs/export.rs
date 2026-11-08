//! ExportDialog.qml: format, resolution, frame rate, quality and where to; the export running
//! in the app with its progress, frames per second and time left, Cancel; the error; and the
//! finished state with Show in folder and Copy path.
//!
//! The export runs on a thread through `crate::export::export` (the Rune offscreen renderer
//! around omareel_core's `ExportJob`, as `omareel export` does) and feeds the Document's export
//! state as the Qt bridge does (`begin_saved_export`, `export_progressed`, `export_finished` or
//! `export_failed`), so the dialog reads `Document::export_state` like the QML reads
//! `editor.exporting`, `exportProgress`, `exportFps`, `exportEtaSeconds` and `exportError`.

use super::super::inspector::widgets::{self, Btn, act, font, mono, om};
use super::{Modal, close_modal, open_modal};
use crate::preview::Preview;
use omareel_core::editor::{Effect, ExportDialogSettings};
use rune::{Cx, El, LineCap, Shape, Tx, col, el, row, text};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

enum Message {
    Progress(i32, i32),
    Done(Result<String, String>),
}

struct Run {
    receive: Receiver<Message>,
    cancel: Arc<AtomicBool>,
    started: Instant,
}

/// The dialog's own fields (the QML properties) and the running export.
#[derive(Default)]
pub struct State {
    format: String,
    height: i32,
    fps: i32,
    quality: String,
    output: String,
    success: String,
    run: Option<Run>,
}

const QUALITIES: [(&str, &str, &str); 3] = [
    ("Web", "web-high", "Compact files for quick sharing."),
    ("Social", "social", "Balanced size and crisp motion."),
    ("Studio", "studio", "Maximum detail for further editing."),
];

fn heights(format: &str) -> [(&'static str, i32); 3] {
    if format == "mp4" { [("720p", 720), ("1080p", 1080), ("4K", 2160)] } else { [("480p", 480), ("720p", 720), ("1080p", 1080)] }
}

fn rates(format: &str) -> &'static [(&'static str, i32)] {
    if format == "mp4" { &[("30", 30), ("60", 60)] } else { &[("15", 15), ("20", 20), ("30", 30)] }
}

/// Main.qml's `openExport`: the project name is committed, the dialog opens and (onOpened)
/// picks the project's format.
pub fn open_export(s: &mut Preview) {
    super::super::topbar::commit_name(s);
    open_modal(s, Modal::Export);
    s.edit.dialogs.export.success.clear();
    let format = s.document.project().export_settings.format.clone();
    select_format(s, if format.is_empty() { "mp4" } else { &format });
}

/// `selectFormat(value)`: the format's settings from the project, its default path, and the
/// project's format.
fn select_format(s: &mut Preview, value: &str) {
    let e = s.document.project().export_settings.clone();
    let d = &mut s.edit.dialogs.export;
    d.format = value.to_owned();
    if value == "mp4" {
        (d.height, d.fps, d.quality) = (e.height, e.fps, e.quality.clone());
    } else {
        (d.height, d.fps, d.quality) = (e.gif.height, e.gif.fps, e.gif.quality.clone());
    }
    d.output = s.document.default_export_path(value);
    crate::editor::set(s, "export.format", value.into(), false, "");
}

/// `exportTo(path, {fps, height, quality})`: saves, starts the export state and the export.
fn export_to(s: &mut Preview) {
    if s.document.export_state().exporting {
        return;
    }
    s.document.save_now();
    let d = &s.edit.dialogs.export;
    let settings = ExportDialogSettings { fps: d.fps, height: d.height, quality: d.quality.clone() };
    let path = d.output.clone();
    let request = s.document.begin_saved_export(&path, &settings);
    crate::editor::edited(s, "");
    let Some(request) = request else { return };
    let options = crate::export::Options {
        bundle: request.bundle_path,
        output: request.output_path,
        fps: request.fps,
        width: request.width,
        quality: request.quality,
        gif_fps: request.gif_fps,
        gif_width: request.gif_width,
        ..Default::default()
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let (send, receive) = std::sync::mpsc::channel();
    let flag = cancel.clone();
    std::thread::Builder::new()
        .name("export".into())
        .spawn(move || {
            let progress_send = send.clone();
            let result = crate::export::export(&options, flag, &mut |frame, total| {
                let _ = progress_send.send(Message::Progress(frame, total));
            });
            let _ = send.send(Message::Done(result.map_err(|e| if e.is_empty() { "Export failed".into() } else { e })));
        })
        .expect("export thread");
    s.edit.dialogs.export.run = Some(Run { receive, cancel, started: Instant::now() });
}

/// `cancelExport()`.
fn cancel_export(s: &mut Preview) {
    if let Some(run) = &s.edit.dialogs.export.run {
        run.cancel.store(true, Ordering::SeqCst);
    }
}

pub fn on_effect(s: &mut Preview, effect: &Effect) {
    if let Effect::ExportFinished(path) = effect {
        s.edit.dialogs.export.success = path.clone();
    }
}

pub fn poll(s: &mut Preview) -> bool {
    let Some(run) = &s.edit.dialogs.export.run else { return false };
    let mut latest = None;
    let mut done = None;
    // Headless runs keep a virtual clock: give the export real time to move each frame.
    if rune::is_headless() {
        match run.receive.recv_timeout(Duration::from_millis(25)) {
            Ok(Message::Progress(f, t)) => latest = Some((f, t)),
            Ok(Message::Done(r)) => done = Some(r),
            Err(_) => {}
        }
    }
    loop {
        match run.receive.try_recv() {
            Ok(Message::Progress(f, t)) => latest = Some((f, t)),
            Ok(Message::Done(r)) => {
                done = Some(r);
                break;
            }
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                done.get_or_insert(Err("Export failed".into()));
                break;
            }
        }
    }
    let elapsed = run.started.elapsed().as_millis() as i64;
    let mut changed = false;
    if let Some((frame, total)) = latest {
        s.document.export_progressed(frame, total, elapsed);
        changed = true;
    }
    if let Some(result) = done {
        s.edit.dialogs.export.run = None;
        match result {
            Ok(path) => s.document.export_finished(&path),
            Err(error) => s.document.export_failed(&error),
        }
        changed = true;
    }
    if changed {
        crate::editor::handle_effects(s);
    }
    changed
}

pub fn busy(s: &Preview) -> bool {
    s.edit.dialogs.export.run.is_some()
}

/// `estimatedMegabytes()`.
fn estimated_megabytes(d: &State, duration: f64) -> f64 {
    let scale = f64::from(d.height) / 1080.0;
    let base = match d.quality.as_str() {
        "studio" => 18.0,
        "social" => 10.0,
        _ => 5.0,
    };
    let bitrate = if d.format == "gif" { base * 1.8 * scale } else { base * scale * scale };
    (bitrate * duration / 8.0).max(0.1)
}

/// `formatEta(seconds)`.
fn format_eta(seconds: i32) -> String {
    if seconds < 60 { format!("{seconds}s") } else { format!("{}m {}s", seconds / 60, seconds % 60) }
}

/// `path.replace(/^\/home\/[^/]+/, "~")`.
fn tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("/home/") {
        let end = rest.find('/').unwrap_or(rest.len());
        return format!("~{}", &rest[end..]);
    }
    path.to_owned()
}

/// `openContainingFolder(path)`: `xdg-open` on its folder (not in a headless run).
fn open_folder(path: &str) {
    let Some(directory) = omareel_core::editor::containing_folder(path) else { return };
    if rune::is_headless() {
        eprintln!("export dialog: would open {directory}");
        return;
    }
    let _ = std::process::Command::new("xdg-open").arg(directory).stdin(std::process::Stdio::null()).spawn();
}

/// The card's contents.
pub fn view(s: &Preview, _cx: &mut Cx<Preview>) -> El {
    let ui = &s.ui;
    let o = om();
    let d = &s.edit.dialogs.export;
    let state = s.document.export_state();
    let exporting = state.exporting;
    let done = !d.success.is_empty();
    let inner_w = 460.0 - 2.0 * (o.popup_padding + 1.0);

    let header = row()
        .w_full()
        .items_center()
        .child(
            col()
                .gap(2.0)
                .child(text(if done { "Export complete" } else { "Export" }).font(font(o.title, 600.0)).color(ui.text_strong))
                .child(
                    text(format!(
                        "{}  ·  {}×{} source",
                        omareel_core::editor::format_time(s.document.duration()),
                        s.document.source_width(),
                        s.document.source_height()
                    ))
                    .font(mono(o.caption, 400.0))
                    .color(ui.text_muted),
                ),
        )
        .child(el().grow(1.0))
        .child(widgets::icon_button(s, "export-close", &crate::icons::X, "Close export", "", !exporting, false, act(|s, _| close_modal(s))));

    // The current frame doubles as the progress surface while the export runs: the live
    // composition, covering the 16:9 box.
    let thumb_h = (inner_w * 9.0 / 16.0).round();
    let (out_w, out_h) = (s.document.output_width() as f32, s.document.output_height() as f32);
    let cover = (inner_w / out_w.max(1.0)).max(thumb_h / out_h.max(1.0));
    let (cw, ch) = (out_w * cover, out_h * cover);
    let mut thumbnail = el()
        .key("export-thumbnail")
        .w_full()
        .h(thumb_h)
        .radius(o.control_radius)
        .clip_rounded(o.control_radius)
        .fill(ui.canvas)
        .child(
            el().absolute((inner_w - cw) / 2.0, (thumb_h - ch) / 2.0)
                .size(cw, ch)
                .key("export-frame")
                .scale(if exporting { 1.03 } else { 1.0 })
                .child(crate::comp::composition(&s.settings, &s.media, &s.frame, cw, ch, true)),
        );
    if exporting || done {
        thumbnail = thumbnail.child(el().key("export-veil").absolute(0.0, 0.0).w_full().h_full().fill(ui.canvas.alpha(0.62)).transition(rune::Transition::opacity()));
    }
    if exporting {
        let progress = state.progress.min(0.99) as f32;
        let ring = |sweep: f32, start: f32| -> Vec<[f32; 2]> {
            let n = (sweep / 4.0).ceil().max(2.0) as usize;
            (0..=n)
                .map(|i| {
                    let a = (start + sweep * i as f32 / n as f32).to_radians();
                    [36.0 + 32.0 * a.cos(), 36.0 + 32.0 * a.sin()]
                })
                .collect()
        };
        let mut shape = Shape::new().stroke_with(ring(360.0, 0.0), true, 4.0, LineCap::Butt, ui.foreground.alpha(0.16));
        if progress > 0.0 {
            shape = shape.stroke_with(ring(360.0 * progress, -90.0), false, 4.0, if o.rounded { LineCap::Round } else { LineCap::Butt }, ui.accent);
        }
        thumbnail = thumbnail.child(
            el().absolute(0.0, 0.0).w_full().h_full().center().child(
                el().key("export-ring")
                    .size(72.0, 72.0)
                    .shape(shape)
                    .center()
                    .child(text(format!("{}%", ((state.progress * 100.0).round() as i32).min(99))).font(mono(o.title, 600.0)).color(ui.text_strong)),
            ),
        );
    }
    if done {
        thumbnail = thumbnail.child(
            el().absolute(0.0, 0.0).w_full().h_full().center().child(
                el().key("export-done-badge")
                    .size(56.0, 56.0)
                    .radius(if o.rounded { 28.0 } else { 0.0 })
                    .fill(ui.accent)
                    .center()
                    .transition(rune::Transition::scale(0.4))
                    .child(rune::icon(&crate::icons::CHECK, 26.0).color(ui.accent_foreground)),
            ),
        );
    }
    if !exporting && !done {
        let res = if d.height == 2160 { "4K".to_owned() } else { format!("{}p", d.height) };
        thumbnail = thumbnail.child(
            row()
                .key("export-spec")
                .absolute(10.0, thumb_h - 32.0)
                .h(22.0)
                .px(7.0)
                .items_center()
                .radius(o.small_radius)
                .fill(ui.canvas.alpha(0.8))
                .child(text(format!("{}  {}  {} fps", d.format.to_uppercase(), res, d.fps)).font(mono(o.caption, 400.0)).color(ui.text_strong)),
        );
    }

    let mut body = col().key("export-dialog").w_full().gap(14.0).child(header).child(thumbnail);

    if !done {
        let enabled = !exporting;
        let group_w = Some(216.0);
        let format_i = usize::from(d.format == "gif");
        let hs = heights(&d.format);
        let rs = rates(&d.format);
        let height_i = hs.iter().position(|(_, v)| *v == d.height);
        let rate_i = rs.iter().position(|(_, v)| *v == d.fps);
        let quality_i = QUALITIES.iter().position(|(_, v, _)| *v == d.quality);
        let caption = quality_i.map_or("", |i| QUALITIES[i].2);
        let settings = col()
            .w_full()
            .gap(10.0)
            .child(widgets::setting_row(
                ui,
                "Format",
                "",
                enabled,
                widgets::button_group(s, "export-format", &["MP4", "GIF"], Some(format_i), enabled, group_w, |s, i| {
                    select_format(s, if i == 1 { "gif" } else { "mp4" })
                }),
            ))
            .child(widgets::setting_row(
                ui,
                "Resolution",
                "",
                enabled,
                widgets::button_group(s, "export-height", &hs.map(|(l, _)| l), height_i, enabled, group_w, move |s, i| {
                    let value = hs[i].1;
                    s.edit.dialogs.export.height = value;
                    let path = if s.edit.dialogs.export.format == "mp4" { "export.height" } else { "export.gif.height" };
                    crate::editor::set(s, path, value.into(), false, "");
                }),
            ))
            .child(widgets::setting_row(
                ui,
                "Frame rate",
                "",
                enabled,
                widgets::button_group(s, "export-fps", &rs.iter().map(|(l, _)| *l).collect::<Vec<_>>(), rate_i, enabled, group_w, move |s, i| {
                    let value = rs[i].1;
                    s.edit.dialogs.export.fps = value;
                    let path = if s.edit.dialogs.export.format == "mp4" { "export.fps" } else { "export.gif.fps" };
                    crate::editor::set(s, path, value.into(), false, "");
                }),
            ))
            .child(widgets::setting_row(
                ui,
                "Quality",
                caption,
                enabled,
                widgets::button_group(s, "export-quality", &QUALITIES.map(|(l, _, _)| l), quality_i, enabled, group_w, |s, i| {
                    let value = QUALITIES[i].1;
                    s.edit.dialogs.export.quality = value.to_owned();
                    let path = if s.edit.dialogs.export.format == "mp4" { "export.quality" } else { "export.gif.quality" };
                    crate::editor::set(s, path, value.into(), false, "");
                }),
            ))
            .child(el().w_full().pt(2.0).child(widgets::separator(ui)))
            .child(
                row()
                    .w_full()
                    .gap(8.0)
                    .items_center()
                    .child(rune::icon(&crate::icons::FOLDER_OPEN, 15.0).color(ui.text_muted))
                    .child(text(tilde(&d.output)).key("export-path").font(mono(o.caption, 400.0)).color(ui.foreground).grow(1.0).min_w(0.0).truncate_middle())
                    .child(widgets::button(
                        ui,
                        "export-change",
                        "Change…",
                        None,
                        Btn { small: true, disabled: !enabled, ..Btn::default() },
                        act(|s, tx| choose_output(s, tx)),
                    )),
            );
        body = body.child(if enabled { settings } else { settings.dim(0.5) });
    }

    if exporting {
        let progress = state.progress.min(0.99) as f32;
        body = body.child(
            col()
                .w_full()
                .gap(8.0)
                .child(
                    el().key("export-progress")
                        .role("progressbar")
                        .w_full()
                        .h(4.0)
                        .radius(if o.rounded { 2.0 } else { 0.0 })
                        .fill(ui.foreground.alpha(0.12))
                        .child(el().key("export-progress-fill").h_full().w_frac(progress).radius(if o.rounded { 2.0 } else { 0.0 }).fill(ui.accent)),
                )
                .child({
                    let mut r = row().w_full().child(
                        text(if state.progress >= 1.0 { "Finishing encoding…" } else { "Rendering…" })
                            .key("export-status")
                            .font(font(o.small, 400.0))
                            .color(ui.foreground),
                    );
                    r = r.child(el().grow(1.0));
                    if state.progress < 1.0 {
                        r = r.child(
                            text(format!("{:.0} fps  ·  {} left", state.fps, format_eta(state.eta_seconds))).font(mono(o.caption, 400.0)).color(ui.text_muted),
                        );
                    }
                    r
                }),
        );
    }
    if !state.error.is_empty() && !exporting {
        body = body.child(text(state.error.clone()).key("export-error").font(font(o.small, 400.0)).color(ui.record).wrap().w_full());
    }
    if done {
        body = body.child(
            text(tilde(&d.success)).key("export-success-path").font(mono(o.caption, 400.0)).color(ui.text_muted).truncate_middle().align(rune::Align::Center).w_full(),
        );
    }

    let mut buttons = row().w_full().pt(2.0).gap(8.0).items_center();
    if !exporting && !done {
        let mb = estimated_megabytes(d, s.document.duration());
        buttons = buttons.child(text(format!("≈ {:.*} MB", if mb < 10.0 { 1 } else { 0 }, mb)).font(mono(o.small, 400.0)).color(ui.text_muted));
    }
    if done {
        let path = d.success.clone();
        let copy = d.success.clone();
        buttons = buttons
            .child(widgets::button(ui, "export-show", "Show in folder", Some(&crate::icons::FOLDER_OPEN), Btn::bordered(), act(move |_, _| open_folder(&path))))
            .child(widgets::button(ui, "export-copy", "Copy path", None, Btn::default(), act(move |_, tx: &mut Tx| tx.copy(copy.clone()))));
    }
    buttons = buttons.child(el().grow(1.0));
    if exporting {
        buttons = buttons.child(widgets::button(ui, "export-cancel", "Cancel", None, Btn::bordered(), act(|s, _| cancel_export(s))));
    }
    if !exporting && !done {
        buttons = buttons.child(widgets::button(
            ui,
            "export-start",
            "Export",
            Some(&crate::icons::DOWNLOAD),
            Btn { width: Some(120.0), ..Btn::primary() },
            act(|s, _| export_to(s)),
        ));
    }
    if done {
        buttons = buttons.child(widgets::button(ui, "export-done", "Done", None, Btn { width: Some(88.0), ..Btn::primary() }, act(|s, _| close_modal(s))));
    }
    body.child(buttons)
}

/// "Change…": a Save dialog for the output, with the format's suffix.
fn choose_output(s: &mut Preview, tx: &mut Tx) {
    let d = &s.edit.dialogs.export;
    let path = std::path::Path::new(&d.output);
    let gif = d.format == "gif";
    let options = rune::FileDialogOptions {
        title: "Export to".into(),
        directory: path.parent().map(std::path::Path::to_path_buf),
        file_name: path.file_name().map(|n| n.to_string_lossy().into_owned()),
        ..Default::default()
    }
    .filter(if gif { "GIF image" } else { "MP4 video" }, if gif { &["gif"] } else { &["mp4"] });
    tx.save_file(options, move |s: &mut Preview, result, _tx| {
        if let Ok(Some(chosen)) = result {
            let mut chosen = chosen.to_string_lossy().into_owned();
            // FileDialog's defaultSuffix.
            if std::path::Path::new(&chosen).extension().is_none() {
                chosen.push_str(if gif { ".gif" } else { ".mp4" });
            }
            s.edit.dialogs.export.output = chosen;
        }
    });
}

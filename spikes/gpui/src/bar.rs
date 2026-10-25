//! `omareel-gpui bar`: the recording bar's pill (a recording dot, the elapsed time and a stop
//! button) as a wlr-layer-shell surface anchored to the top of the screen, as
//! src/ui/qml/RecordingBar.qml is. Stop quits.

use gpui::layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions};
use gpui::{
    App, AppContext, Bounds, Context, FontWeight, IntoElement, Render, Window, WindowBackgroundAppearance, WindowBounds,
    WindowKind, WindowOptions, div, point, prelude::*, px, rgb, rgba, size,
};
use std::time::{Duration, Instant};

struct Bar {
    started: Instant,
    mono: gpui::SharedString,
}

impl Bar {
    fn new(cx: &mut Context<Self>) -> Bar {
        // The time label changes once a second; nothing else needs a frame.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(250)).await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        })
        .detach();
        Bar {
            started: Instant::now(),
            mono: omareel_core::theme::monospace_family().unwrap_or_else(|| "monospace".into()).into(),
        }
    }
}

impl Render for Bar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let elapsed = self.started.elapsed().as_secs();
        let (background, foreground) = omareel_core::theme::render_colors();
        let background = gpui::Rgba {
            r: background.red() as f32 / 255.0,
            g: background.green() as f32 / 255.0,
            b: background.blue() as f32 / 255.0,
            a: 0.94,
        };
        let foreground = gpui::Rgba {
            r: foreground.red() as f32 / 255.0,
            g: foreground.green() as f32 / 255.0,
            b: foreground.blue() as f32 / 255.0,
            a: 1.0,
        };
        div().size_full().flex().items_center().justify_center().child(
            div()
                .h(px(36.0))
                .px(px(6.0))
                .pl(px(14.0))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .rounded_full()
                .bg(background)
                .border_1()
                .border_color(rgba(0xffffff1f))
                .text_color(foreground)
                .child(div().size(px(10.0)).rounded_full().bg(rgb(0xf7768e)))
                .child(
                    div()
                        .font_family(self.mono.clone())
                        .font_weight(FontWeight::MEDIUM)
                        .text_size(px(14.0))
                        .child(format!("{:02}:{:02}", elapsed / 60, elapsed % 60)),
                )
                .child(
                    div()
                        .id("stop")
                        .h(px(26.0))
                        .px(px(12.0))
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .rounded_full()
                        .bg(rgba(0xf7768e33))
                        .hover(|s| s.bg(rgba(0xf7768e55)))
                        .child(div().size(px(9.0)).rounded(px(2.0)).bg(rgb(0xf7768e)))
                        .child(div().text_size(px(13.0)).child("Stop"))
                        .on_click(cx.listener(|_, _, _, cx| cx.quit())),
                ),
        )
    }
}

pub fn run() {
    gpui_platform::application().run(|cx: &mut App| {
        cx.open_window(
            WindowOptions {
                titlebar: None,
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: point(px(0.0), px(0.0)),
                    size: size(px(240.0), px(44.0)),
                })),
                app_id: Some("omareel-gpui-bar".into()),
                window_background: WindowBackgroundAppearance::Transparent,
                kind: WindowKind::LayerShell(LayerShellOptions {
                    namespace: "omareel-bar".into(),
                    layer: Layer::Overlay,
                    anchor: Anchor::TOP,
                    margin: Some((px(12.0), px(0.0), px(0.0), px(0.0))),
                    keyboard_interactivity: KeyboardInteractivity::None,
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_, cx| cx.new(Bar::new),
        )
        .expect("open the layer-shell bar");
    });
}

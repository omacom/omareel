//! The recording bar: a pill with a pulsing dot, the elapsed time and a stop button, as a
//! wlr-layer-shell surface on the overlay layer anchored to the top of the screen
//! (iced_layershell), like omareel's RecordingBar.qml through LayerShellQt.

use iced::widget::{button, container, row, text};
use iced::{Alignment, Border, Color, Element, Length, Subscription, Task, Theme};
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::{LayerShellSettings, Settings, StartMode};
use iced_layershell::to_layer_message;
use omareel_core::compat::qcolor::QColor;
use std::time::{Duration, Instant};

#[to_layer_message]
#[derive(Debug, Clone)]
enum Message {
    Tick,
    Stop,
}

struct Bar {
    started: Instant,
    elapsed: Duration,
    surface: Color,
    stroke: Color,
    text: Color,
    muted: Color,
    record: Color,
}

fn color(c: QColor) -> Color {
    Color::from_rgba(c.red_f(), c.green_f(), c.blue_f(), c.alpha_f())
}

impl Bar {
    fn new() -> Bar {
        let theme = omareel_core::theme::Theme::new(None);
        let palette = &theme.palette;
        Bar {
            started: Instant::now(),
            elapsed: Duration::ZERO,
            surface: color(palette.surface()),
            stroke: color(palette.stroke_strong()),
            text: color(palette.foreground),
            muted: color(palette.text_muted()),
            record: color(palette.record),
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Tick => self.elapsed = self.started.elapsed(),
            Message::Stop => {
                println!("stopped after {:.1} s", self.started.elapsed().as_secs_f64());
                return iced::exit();
            }
            _ => {}
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let seconds = self.elapsed.as_secs();
        // The dot pulses as the QML one does (an opacity loop), here a 1.6 s cosine.
        let phase = (self.elapsed.as_secs_f32() / 1.6 * std::f32::consts::TAU).cos() * 0.5 + 0.5;
        let record = Color { a: 0.35 + 0.65 * phase, ..self.record };
        let dot = container(text(""))
            .width(8)
            .height(8)
            .style(move |_| container::Style { background: Some(record.into()), border: Border { radius: 4.0.into(), ..Border::default() }, ..container::Style::default() });
        let (text_color, muted, stroke) = (self.text, self.muted, self.stroke);
        let stop = button(container(text("")).width(10).height(10).style(move |_| container::Style {
            background: Some(text_color.into()),
            border: Border { radius: 2.0.into(), ..Border::default() },
            ..container::Style::default()
        }))
        .width(28)
        .height(28)
        .padding(9)
        .on_press(Message::Stop)
        .style(move |_, status| button::Style {
            background: match status {
                button::Status::Hovered | button::Status::Pressed => Some(Color { a: 0.12, ..text_color }.into()),
                _ => None,
            },
            border: Border { radius: 14.0.into(), ..Border::default() },
            text_color,
            ..button::Style::default()
        });
        let time = text(format!("{:02}:{:02}", seconds / 60, seconds % 60)).size(13).font(iced::Font::MONOSPACE).color(text_color);
        let surface = self.surface;
        let pill = container(
            row![dot, time, text("REC").size(11).color(muted), stop].spacing(10).align_y(Alignment::Center).padding([0, 6]),
        )
        .height(40)
        .padding([0, 8])
        .align_y(Alignment::Center)
        .style(move |_| container::Style {
            background: Some(surface.into()),
            border: Border { radius: 20.0.into(), width: 1.0, color: stroke },
            ..container::Style::default()
        });
        container(pill).width(Length::Fill).height(Length::Fill).center_x(Length::Fill).center_y(Length::Fill).into()
    }
}

pub fn run() -> Result<(), String> {
    iced_layershell::application(Bar::new, || "omareel-recording-bar".to_owned(), Bar::update, Bar::view)
        .style(|_: &Bar, theme: &Theme| iced::theme::Style { background_color: Color::TRANSPARENT, text_color: theme.palette().text })
        .subscription(|_: &Bar| Subscription::batch([iced::time::every(Duration::from_millis(50)).map(|_| Message::Tick)]))
        .settings(Settings {
            layer_settings: LayerShellSettings {
                size: Some((200, 48)),
                anchor: Anchor::Top,
                layer: Layer::Overlay,
                exclusive_zone: 0,
                margin: (8, 0, 0, 0),
                keyboard_interactivity: KeyboardInteractivity::None,
                start_mode: StartMode::Active,
                events_transparent: false,
            },
            ..Settings::default()
        })
        .run()
        .map_err(|error| error.to_string())
}

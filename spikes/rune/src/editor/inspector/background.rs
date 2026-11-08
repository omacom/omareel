//! BackgroundPanel.qml: no background; the theme wallpapers as thumbnails (eleven, then "show
//! more") with a tile to choose any image; the gradient presets (ten, then more) with the angle;
//! plain colours and a custom one; and the background blur.

use super::widgets::{self, Btn, SliderSpec, act, button, om, project_slider, section_header, separator, track_hot};
use crate::editor::{dialogs, edited};
use crate::preview::Preview;
use omareel_core::compat::qcolor::QColor;
use omareel_core::compat::qjson::Value;
use rune::{Color, Cursor, Cx, El, Tx, col, el, row};

/// The plain colours, as BackgroundPanel's `plainColours` (Qt.rgba).
const PLAIN: [&str; 7] = ["#000000", "#ffffff", "#374151", "#9ca3af", "#7f1d1d", "#047857", "#1d4ed8"];

/// `choose(type, path, value)`: the type and its value as one undo step.
fn choose(s: &mut Preview, type_: &str, path: &str, value: Value) {
    s.document.begin_coalesced_edit("background-choice");
    s.document.set_project_value("background.type", type_.into(), true);
    if !path.is_empty() {
        s.document.set_project_value(path, value, true);
    }
    s.document.end_coalesced_edit();
    edited(s, "");
}

/// SwatchTile: a hairline-edged face that springs up under the pointer; the chosen one has an
/// accent ring with a small gap.
fn swatch(s: &Preview, key: &str, w: f32, h: f32, selected: bool, interactive: bool, face: El, help: &str, on: impl Fn(&mut Preview) + 'static) -> El {
    let ui = &s.ui;
    let o = om();
    let hot = interactive && widgets::is_hot(s, key);
    let ring_radius = o.control_radius + if o.rounded { 3.0 } else { 0.0 };
    let inset = if selected { 4.0 } else { 0.0 };
    let ring = if selected {
        ui.accent
    } else if hot {
        ui.stroke_strong
    } else {
        Color::TRANSPARENT
    };
    let mut tile = el()
        .key(key.to_owned())
        .size(w, h)
        .radius(ring_radius)
        .border(2.0, ring)
        .scale(if hot { 1.04 } else { 1.0 })
        .child(
            el().key(format!("{key}-face"))
                .absolute(inset, inset)
                .size(w - 2.0 * inset, h - 2.0 * inset)
                .radius(o.control_radius)
                .clip_rounded(o.control_radius)
                .fill(ui.fill)
                .border(1.0, ui.stroke)
                .child(face),
        );
    if interactive {
        tile = track_hot(tile.role("radio").checked(selected).cursor(Cursor::Pointer).on_press(on), key);
    }
    if !help.is_empty() {
        tile = tile.help(help.to_owned());
    }
    tile
}

/// SectionToggle: "Show N more" / "Show less".
fn section_toggle(s: &Preview, key: &str, expanded: bool, hidden: usize, on: impl Fn(&mut Preview) + 'static) -> El {
    let label = if expanded { "Show less".to_owned() } else { format!("Show {hidden} more") };
    let _ = s;
    button(&s.ui, key, &label, None, Btn { fill: true, small: true, height: Some(28.0), trailing: Some(&crate::icons::CHEVRON_DOWN), ..Btn::default() }, act(move |s, _| on(s)))
}

/// The custom image: an Open dialog for pictures.
fn choose_image(_s: &mut Preview, tx: &mut Tx) {
    let options = rune::FileDialogOptions { title: "Choose a background image".into(), ..Default::default() }.filter("Images", &["png", "jpg", "jpeg", "webp"]);
    tx.open_file(options, |s: &mut Preview, result, _| {
        if let Ok(Some(path)) = result {
            choose(s, "image", "background.image", path.to_string_lossy().as_ref().into());
        }
    });
}

/// `gradientMatches(colours)`: the stops are these colours, evenly spaced.
fn gradient_matches(stops: &[omareel_core::project::GradientStop], colours: &[String]) -> bool {
    stops.len() == colours.len()
        && stops.iter().zip(colours).enumerate().all(|(i, (stop, colour))| {
            stop.color.name().eq_ignore_ascii_case(&QColor::parse(colour).name()) && (stop.position - i as f64 / (colours.len().max(2) - 1) as f64).abs() <= 0.0001
        })
}

pub fn view(s: &Preview, _cx: &mut Cx<Preview>, w: f32) -> El {
    let ui = &s.ui;
    let o = om();
    let bg = &s.document.project().background;
    let none = bg.type_ == "none";
    let resolved = crate::editor::background_path(&s.document);
    let state = &s.edit.inspector;

    // Wallpapers: four columns, eleven and the picker until expanded.
    let all = super::wallpapers(s);
    let shown = if state.show_all_wallpapers { all.len() } else { all.len().min(11) };
    let tile_w = ((w - 8.0 * 3.0) / 4.0).max(1.0);
    let tile_h = tile_w * 10.0 / 16.0;
    let mut tiles: Vec<El> = all[..shown]
        .iter()
        .enumerate()
        .map(|(i, (theme, wallpaper))| {
            let selected = bg.type_ == "wallpaper" && wallpaper.path == resolved;
            let face = if wallpaper.thumbnail_url.is_empty() {
                el()
            } else {
                let path = omareel_core::omarchy_paths::wallpaper_thumbnail_path(&wallpaper.path);
                let image = super::thumbnail(s, &path);
                el().absolute(1.0, 1.0).w_full().h_full().pixels_fit(image.pixels(), rune::SurfaceFit::Fill).transition(rune::Transition::opacity())
            };
            let path = wallpaper.path.clone();
            swatch(s, &format!("wallpaper-{i}"), tile_w, tile_h, selected, true, face, &format!("{theme} · {}", wallpaper.file), move |s| {
                choose(s, "wallpaper", "background.wallpaper", path.as_str().into())
            })
        })
        .collect();
    let picker = el()
        .key("wallpaper-pick")
        .role("button")
        .label("Pick file")
        .w_full()
        .h_full()
        .center()
        .hover(ui.fill_hover)
        .press(ui.fill_pressed)
        .cursor(Cursor::Pointer)
        .help("Choose an image…")
        .on_press_tx(choose_image)
        .child(rune::icon(&crate::icons::PLUS, 16.0).color(ui.text_muted));
    tiles.push(swatch(s, "wallpaper-picker", tile_w, tile_h, bg.type_ == "image", false, picker, "", |_| {}));
    let mut grid = col().w_full().gap(8.0);
    for chunk in tiles.chunks(4) {
        let mut r = row().w_full().gap(8.0);
        for t in chunk {
            r = r.child(t.clone());
        }
        grid = grid.child(r);
    }

    let mut panel = col()
        .w_full()
        .gap(o.panel_gap)
        .child(button(
            ui,
            "background-none",
            if none { "No background" } else { "Remove background" },
            Some(&crate::icons::X),
            Btn { bordered: true, selected: none, fill: true, ..Btn::default() },
            act(|s, _| choose(s, "none", "", Value::Null)),
        ))
        .child(section_header(ui, "Wallpaper").mt(4.0))
        .child(grid);
    let hidden = all.len().saturating_sub(11);
    if hidden > 0 {
        panel = panel.child(section_toggle(s, "wallpapers-more", state.show_all_wallpapers, hidden, |s| {
            s.edit.inspector.show_all_wallpapers = !s.edit.inspector.show_all_wallpapers
        }));
    }

    // Gradients: five columns, ten until expanded.
    let presets: Vec<Vec<String>> = s.document.gradients().iter().map(|g| g.to_array().iter().map(|c| c.to_string_or("")).collect()).collect();
    let cell = ((w - 8.0 * 4.0) / 5.0).max(1.0);
    let shown = if state.show_all_gradients { presets.len() } else { presets.len().min(10) };
    let mut grid = col().w_full().gap(8.0);
    for (r, chunk) in presets[..shown].chunks(5).enumerate() {
        let mut line = row().w_full().gap(8.0);
        for (c, colours) in chunk.iter().enumerate() {
            let index = r * 5 + c;
            let selected = bg.type_ == "gradient" && gradient_matches(&bg.gradient.stops, colours);
            let first = QColor::parse(colours.first().map_or("#000000", String::as_str));
            let last = QColor::parse(colours.last().map_or("#000000", String::as_str));
            let pixels = crate::comp::gradient_pixels(180.0, &[(0.0, first), (1.0, last)], 1.0, 48);
            let face = el().w_full().h_full().pixels(&pixels);
            line = line.child(swatch(s, &format!("gradientTile-{index}"), cell, cell, selected, true, face, "", move |s| {
                s.document.apply_gradient_preset(index as i32);
                edited(s, "");
            }));
        }
        grid = grid.child(line);
    }
    panel = panel.child(section_header(ui, "Gradient").mt(4.0)).child(grid);
    if presets.len() > 10 {
        panel = panel.child(section_toggle(s, "gradients-more", state.show_all_gradients, presets.len() - 10, |s| {
            s.edit.inspector.show_all_gradients = !s.edit.inspector.show_all_gradients
        }));
    }
    if bg.type_ == "gradient" {
        panel = panel.child(project_slider(s, SliderSpec::new("background-angle", "Angle", 0.0, 360.0, bg.gradient.angle, 1.0).suffix("°"), "background.gradient.angle"));
    }

    // Plain colours and the custom one.
    let colour_w = (w - 6.0 * 7.0) / 8.0;
    let colour_h = colour_w.min(34.0);
    let mut colours = row().w_full().gap(6.0);
    for (i, hex) in PLAIN.iter().enumerate() {
        let selected = bg.type_ == "color" && bg.color.name().eq_ignore_ascii_case(hex);
        let face = el().w_full().h_full().fill(crate::theme::hex(hex));
        colours = colours.child(swatch(s, &format!("colour-{i}"), colour_w, colour_h, selected, true, face, "", move |s| {
            choose(s, "color", "background.color", (*hex).into())
        }));
    }
    let current = bg.color.clone();
    let custom = el()
        .key("colour-custom")
        .role("button")
        .label("Custom color")
        .w_full()
        .h_full()
        .center()
        .hover(ui.fill_hover)
        .press(ui.fill_pressed)
        .cursor(Cursor::Pointer)
        .help("Custom color…")
        .on_press(move |s: &mut Preview| {
            dialogs::open_color(s, "Background color", &current, |s, c| choose(s, "color", "background.color", dialogs::color_value(&c).into()));
        })
        .child(rune::icon(&crate::icons::PALETTE, 16.0).color(ui.text_muted));
    colours = colours.child(swatch(s, "colour-custom-tile", colour_w, colour_h, false, false, custom, "", |_| {}));
    panel = panel.child(section_header(ui, "Color").mt(4.0)).child(colours).child(el().w_full().pt(4.0).child(separator(ui)));
    let blur = project_slider(s, SliderSpec::new("background-blur", "Background blur", 0.0, 100.0, bg.blur, 1.0).enabled(!none), "background.blur");
    panel.child(if none { blur.dim(0.42) } else { blur })
}

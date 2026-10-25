#![cfg_attr(not(target_arch = "x86_64"), allow(dead_code, unused_imports))]

//! theme: `theme::Theme` against `Omareel::Theme` (README "theme"), with the oracle's state
//! directories, `OMAREEL_THEME_RADIUS` and the `fc-match` and `hyprctl` shims of
//! `tests/golden/shims` first on `PATH`.
//!
//! Theme mixes float colour channels, rounds with qRound and computes luminance with glibc's
//! `pow`, so `cases.txt` is checked on x86_64 only, where it was recorded.
//!
//! Left out: the first row, `bundledUiFamily()`, which registers the bundled Inter with
//! `QFontDatabase` and stays in C++ (the oracle had no resources, so it found none and Theme
//! fell back to "sans-serif", which is what `Theme::new(None)` gives). `setFontFamilies()` is a
//! C++ static; its two cases pass the family to `Theme::new` instead.

use crate::hermetic::*;
use crate::support::*;
use omareel_core::compat::qcolor::QColor;
use omareel_core::theme::Theme;

/// A `QVariantMap` of ints: `{key=value,...}` in `QMap` order (UTF-16 code units).
fn map_text(entries: &[(&str, i32)]) -> String {
    let mut entries = entries.to_vec();
    entries.sort_by(|a, b| a.0.encode_utf16().cmp(b.0.encode_utf16()));
    let items: Vec<String> = entries.iter().map(|(key, value)| format!("{key}={value}")).collect();
    format!("{{{}}}", items.join(","))
}

/// The oracle's `dumpTheme`: every Q_PROPERTY in `Theme.h` order, then `controlFill`,
/// `controlBorder` and `controlBorderWidth` for every (focused, hot, selected).
fn dump_theme(out: &mut Text, theme: &Theme) {
    let p = &theme.palette;
    let colors: [(&str, QColor); 48] = [
        ("accent", p.accent),
        ("accentForeground", p.accent_foreground()),
        ("background", p.background),
        ("lighterBackground", p.lighter_background),
        ("darkBackground", p.dark_background),
        ("foreground", p.foreground),
        ("surface", p.surface()),
        ("surfaceRaised", p.surface_raised()),
        ("hairline", p.hairline()),
        ("hairlineStrong", p.hairline_strong()),
        ("separator", p.separator()),
        ("textMuted", p.text_muted()),
        ("textFaint", p.text_faint()),
        ("accentSoft", p.accent_soft()),
        ("zoomAccent", p.zoom_accent),
        ("record", p.record),
        ("normalFill", p.normal_fill),
        ("hoverFill", p.hover_fill),
        ("selectedFill", p.selected_fill),
        ("pressedFill", p.pressed_fill),
        ("selectionFill", p.selection_fill),
        ("normalBorder", p.normal_border),
        ("hoverBorder", p.hover_border),
        ("selectedBorder", p.selected_border),
        ("popupBackground", p.popup_background),
        ("popupText", p.popup_text),
        ("popupBorder", p.popup_border),
        ("tooltipBackground", p.tooltip_background),
        ("tooltipText", p.tooltip_text),
        ("tooltipBorder", p.tooltip_border),
        ("menuBackground", p.menu_background),
        ("menuText", p.menu_text),
        ("menuBorder", p.menu_border),
        ("menuScrim", p.menu_scrim),
        ("menuSelectedBackground", p.menu_selected_background),
        ("menuSelectedText", p.menu_selected_text),
        ("canvas", p.canvas),
        ("card", p.card),
        ("raised", p.raised),
        ("stroke", p.stroke()),
        ("strokeStrong", p.stroke_strong()),
        ("fill", p.fill()),
        ("fillHover", p.fill_hover()),
        ("fillPressed", p.fill_pressed()),
        ("textStrong", p.text_strong),
        ("accentHover", p.accent_hover()),
        ("accentPressed", p.accent_pressed()),
        ("shadow", p.shadow()),
    ];
    for (name, value) in colors {
        out.row([name.to_owned(), color(&value)]);
    }
    let radii = theme.radii();
    let rows: [(&str, String); 15] = [
        ("fontFamily", quote(&theme.font_family)),
        ("monoFamily", quote(&theme.mono_family)),
        ("themeName", quote(&theme.theme_name)),
        ("font", map_text(&p.font_sizes)),
        ("space", map_text(&p.spaces)),
        ("type", map_text(&p.type_sizes)),
        ("radius", theme.radius.to_string()),
        ("cardRadius", radii.card.to_string()),
        ("controlRadius", radii.control.to_string()),
        ("smallRadius", radii.small.to_string()),
        ("rounded", boolean(theme.rounded()).to_owned()),
        ("normalBorderWidth", p.normal_border_width.to_string()),
        ("hoverBorderWidth", p.hover_border_width.to_string()),
        ("selectedBorderWidth", p.selected_border_width.to_string()),
        ("dark", boolean(p.dark).to_owned()),
    ];
    for (name, value) in rows {
        out.row([name.to_owned(), value]);
    }
    for focused in [false, true] {
        for hot in [false, true] {
            for selected in [false, true] {
                let arguments = format!("({},{},{})", boolean(focused), boolean(hot), boolean(selected));
                out.row([format!("controlFill{arguments}"), color(&p.control_fill(focused, hot, selected))]);
                out.row([format!("controlBorder{arguments}"), color(&p.control_border(focused, hot, selected))]);
                out.row([
                    format!("controlBorderWidth{arguments}"),
                    p.control_border_width(focused, hot, selected).to_string(),
                ]);
            }
        }
    }
}

/// The oracle's `makeState`: `theme.name`, `theme/colors.toml` and `theme/shell.toml` from the
/// case's files that exist, in `$HOME/oracle-scratch/theme/<group>/<name>`.
fn make_state(h: &Hermetic, group: &str, name: &str, source: &str) -> String {
    let state = format!("{}/{group}/{name}", h.scratch());
    remove_path(&state);
    std::fs::create_dir_all(&state).expect("mkpath state");
    for (from, to) in
        [("colors.toml", "theme/colors.toml"), ("shell.toml", "theme/shell.toml"), ("theme.name", "theme.name")]
    {
        if let Ok(data) = std::fs::read(format!("{source}/{from}")) {
            write_file(&format!("{state}/{to}"), data);
        }
    }
    state
}

/// The variables a case may set; all are unset between cases.
const VARIABLES: [&str; 5] = [
    "OMAREEL_THEME_RADIUS",
    "OMAREEL_ORACLE_HYPRCTL_OUTPUT",
    "OMAREEL_ORACLE_HYPRCTL_EXIT",
    "OMAREEL_ORACLE_FC_MATCH",
    "OMAREEL_ORACLE_FC_MATCH_EXIT",
];

fn apply_environment(h: &Hermetic, set: &[(&str, &str)]) {
    for name in VARIABLES {
        h.unset_env(name);
    }
    for (name, value) in set {
        h.set_env(name, value);
    }
}

/// The start of what Hyprland prints for `getoption decoration:rounding -j`.
const HYPRCTL_ROUNDING_PREFIX: &str = r#"{"option": "decoration:rounding", "int": "#;

/// The oracle's environment variants of tokyo-night, by label.
fn variants() -> Vec<(String, Vec<(&'static str, String)>)> {
    let mut variants: Vec<(String, Vec<(&'static str, String)>)> = Vec::new();
    for value in ["0", "1", "4", "7", "8", "13", "24", "-3", "0x10", "010"] {
        variants.push((format!("OMAREEL_THEME_RADIUS={value}"), vec![("OMAREEL_THEME_RADIUS", value.into())]));
    }
    for value in [" 12", "12 "] {
        variants.push((format!("OMAREEL_THEME_RADIUS='{value}'"), vec![("OMAREEL_THEME_RADIUS", value.into())]));
    }
    for value in ["+5", "abc"] {
        variants.push((format!("OMAREEL_THEME_RADIUS={value}"), vec![("OMAREEL_THEME_RADIUS", value.into())]));
    }
    variants.push(("OMAREEL_THEME_RADIUS=''".into(), vec![("OMAREEL_THEME_RADIUS", String::new())]));
    for value in ["4.5", "99999999999"] {
        variants.push((format!("OMAREEL_THEME_RADIUS={value}"), vec![("OMAREEL_THEME_RADIUS", value.into())]));
    }
    for value in ["0", "2", "5", "13", "20", "-4"] {
        variants.push((
            format!("hyprctl int {value}"),
            vec![("OMAREEL_ORACLE_HYPRCTL_OUTPUT", format!("{HYPRCTL_ROUNDING_PREFIX}{value}, \"set\": true }}"))],
        ));
    }
    let hyprctl =
        |label: &str, output: &str| (label.to_owned(), vec![("OMAREEL_ORACLE_HYPRCTL_OUTPUT", output.to_owned())]);
    variants.push(hyprctl("hyprctl int 12.5", r#"{"int":12.5}"#));
    variants.push(hyprctl("hyprctl compact int 6", r#"{"int":6}"#));
    variants.push(hyprctl("hyprctl int 99999999999", r#"{"int": 99999999999}"#));
    variants.push(hyprctl("hyprctl two ints", r#"{"int": 3, "int": 9}"#));
    variants.push(hyprctl("hyprctl no int", r#"{"option": "decoration:rounding", "float": 4.0}"#));
    variants.push(hyprctl("hyprctl not JSON", "no such option"));
    variants.push(hyprctl("hyprctl empty output", ""));
    variants.push((
        "hyprctl exits 1 after printing int 7".into(),
        vec![("OMAREEL_ORACLE_HYPRCTL_OUTPUT", r#"{"int": 7}"#.into()), ("OMAREEL_ORACLE_HYPRCTL_EXIT", "1".into())],
    ));
    variants.push((
        "hyprctl int 7 and OMAREEL_THEME_RADIUS=3".into(),
        vec![("OMAREEL_ORACLE_HYPRCTL_OUTPUT", r#"{"int": 7}"#.into()), ("OMAREEL_THEME_RADIUS", "3".into())],
    ));
    let fc_match = |label: &str, name: &'static str, value: &str| (label.to_owned(), vec![(name, value.to_owned())]);
    variants.push(fc_match("fc-match JetBrainsMono Nerd Font", "OMAREEL_ORACLE_FC_MATCH", "JetBrainsMono Nerd Font"));
    variants.push(fc_match("fc-match with surrounding whitespace", "OMAREEL_ORACLE_FC_MATCH", "  Spaced Mono \n"));
    variants.push(fc_match("fc-match empty output", "OMAREEL_ORACLE_FC_MATCH", ""));
    variants.push(fc_match("fc-match exits 1", "OMAREEL_ORACLE_FC_MATCH_EXIT", "1"));
    variants.push(fc_match("fc-match non-ASCII", "OMAREEL_ORACLE_FC_MATCH", "M\u{f6}no \u{2116}1"));
    variants
}

fn cases_txt() -> String {
    let h = Hermetic::new("theme");
    apply_environment(&h, &[]);
    let mut out = Text::new();
    let run_case = |out: &mut Text,
                    label: &str,
                    state: &str,
                    environment_label: &str,
                    set: &[(&str, &str)],
                    family: Option<&str>| {
        h.set_env("OMAREEL_OMARCHY_STATE_DIR", state);
        apply_environment(&h, set);
        let theme = Theme::new(family.map(str::to_owned));
        out.section(label);
        if !environment_label.is_empty() {
            out.row(["environment", environment_label]);
        }
        dump_theme(out, &theme);
        apply_environment(&h, &[]);
    };

    for group in ["themes", "themes-crafted"] {
        let root = corpus_path(group);
        for name in sorted_entries(&root, true) {
            let source = root.join(&name);
            let state = make_state(&h, group, &name, source.to_str().expect("UTF-8 corpus path"));
            run_case(&mut out, &format!("{group}/{name}"), &state, "", &[], None);
        }
    }
    let tokyo_night = corpus_path("themes/tokyo-night");
    let tokyo_night = tokyo_night.to_str().expect("UTF-8 corpus path");
    let base = make_state(&h, "variants", "tokyo-night", tokyo_night);
    for (label, set) in variants() {
        let set: Vec<(&str, &str)> = set.iter().map(|(name, value)| (*name, value.as_str())).collect();
        run_case(&mut out, &format!("variant tokyo-night {label}"), &base, &label, &set, None);
    }

    // setFontFamilies() set the interface family for later Themes; an empty one restores the
    // default.
    run_case(
        &mut out,
        "variant tokyo-night setFontFamilies(\"Configured Sans\", \"Ignored Mono\")",
        &base,
        "",
        &[],
        Some("Configured Sans"),
    );
    run_case(&mut out, "variant tokyo-night setFontFamilies(\"\", \"\")", &base, "", &[], Some(""));

    // reload() re-reads the files but keeps the radius when hyprctl stops answering.
    let state = make_state(&h, "reload", "tokyo-night", tokyo_night);
    h.set_env("OMAREEL_OMARCHY_STATE_DIR", &state);
    apply_environment(&h, &[("OMAREEL_ORACLE_HYPRCTL_OUTPUT", r#"{"int": 14}"#)]);
    let mut theme = Theme::new(None);
    out.section("reload: constructed with tokyo-night and hyprctl int 14");
    dump_theme(&mut out, &theme);
    let other = corpus_path("themes/catppuccin-latte");
    write_file(&format!("{state}/theme/colors.toml"), read_file(other.join("colors.toml").to_str().expect("UTF-8")));
    write_file(&format!("{state}/theme/shell.toml"), read_file(other.join("shell.toml").to_str().expect("UTF-8")));
    write_file(&format!("{state}/theme.name"), "catppuccin-latte\n");
    apply_environment(&h, &[("OMAREEL_ORACLE_HYPRCTL_EXIT", "1"), ("OMAREEL_ORACLE_HYPRCTL_OUTPUT", "")]);
    theme.reload();
    out.section("reload: files replaced by catppuccin-latte, hyprctl fails");
    dump_theme(&mut out, &theme);
    apply_environment(&h, &[("OMAREEL_THEME_RADIUS", "-9")]);
    theme.reload();
    out.section("reload: OMAREEL_THEME_RADIUS=-9");
    dump_theme(&mut out, &theme);
    apply_environment(&h, &[]);
    remove_path(&state);
    theme.reload();
    out.section("reload: state directory removed, default hyprctl");
    dump_theme(&mut out, &theme);
    out.into_string()
}

#[cfg(target_arch = "x86_64")]
#[test]
fn cases_txt_without_bundled_ui_family() {
    let expected = keep_lines(&read_expected("theme/cases.txt"), |line| !line.starts_with(b"bundledUiFamily()\t"));
    assert_same("theme/cases.txt (without the bundledUiFamily() row)", &expected, cases_txt().as_bytes());
}

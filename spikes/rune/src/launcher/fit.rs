//! The window's height follows its content (`App::fit_content_height`), but Hyprland ignores a
//! floating window's own resize requests: it only clamps it into its minimum and maximum size,
//! which is how Launcher.qml pins the height (minimumHeight = maximumHeight = desiredHeight).
//! Rune sets a window's limits only when it opens, so the launcher asks Hyprland itself to size
//! its window, as the app asks Hyprland for everything else (`hyprctl`).

use std::process::{Command, Stdio};

/// Resizes this process's Hyprland window to `width` x `height` points, off the UI thread.
pub fn resize_window(width: i32, height: i32) {
    let pid = i64::from(std::process::id());
    std::thread::spawn(move || {
        let Ok(output) = Command::new("hyprctl").args(["-j", "clients"]).stderr(Stdio::null()).output() else { return };
        let Ok(clients) = omareel_core::compat::qjson::from_json(&output.stdout) else { return };
        let clients = clients.to_array();
        let Some(address) = clients
            .iter()
            .map(|client| client.to_object())
            .find(|client| client.value("pid").to_variant_long_long() == pid)
            .map(|client| client.value("address").to_string_or(""))
            .filter(|address| !address.is_empty())
        else {
            return;
        };
        // Hyprland 0.57's Lua dispatchers, then the older text dispatcher.
        let lua = format!("hl.dispatch(hl.dsp.window.resize({{x={width},y={height},window='address:{address}'}}))");
        let ok = Command::new("hyprctl").args(["eval", &lua]).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
        if !ok {
            let _ = Command::new("hyprctl")
                .args(["dispatch", "resizewindowpixel", &format!("exact {width} {height},address:{address}")])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    });
}

//! The microphone side of `LauncherBase`, without Qt Multimedia: the level meter's input
//! (`QAudioSource` there, `parec` here: PulseAudio/PipeWire, the same server and source names
//! gpu-screen-recorder lists and records from) and the device watch
//! (`QMediaDevices::audioInputsChanged` there, `pactl subscribe` here).
//!
//! The level itself is omareel_core's `MicrophoneLevel`, as in the Qt bridge.

use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};

/// A running input for the meter: 16-bit mono samples from one source.
pub struct Meter {
    /// The source as the launcher names it (a PulseAudio source name or `default_input`).
    pub device: String,
    child: Option<Child>,
    samples: Receiver<Vec<u8>>,
    running: Arc<AtomicBool>,
}

impl Meter {
    /// Starts listening to `device`, as `listenToMicrophone` opened a `QAudioSource`.
    pub fn start(device: &str) -> Meter {
        let (send, samples) = channel();
        let running = Arc::new(AtomicBool::new(false));
        let source = if device == "default_input" { "@DEFAULT_SOURCE@" } else { device };
        let child = Command::new("parec")
            .args(["--raw", "--format=s16le", "--channels=1", "--rate=16000", "--latency-msec=50", "--client-name=omareel"])
            .arg(format!("--device={source}"))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok();
        let mut child = child;
        if let Some(mut out) = child.as_mut().and_then(|c| c.stdout.take()) {
            running.store(true, Ordering::SeqCst);
            let running = running.clone();
            std::thread::Builder::new()
                .name("microphone-meter".into())
                .spawn(move || {
                    let mut buffer = vec![0u8; 1600];
                    while let Ok(n) = out.read(&mut buffer) {
                        if n == 0 || send.send(buffer[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    running.store(false, Ordering::SeqCst);
                })
                .ok();
        }
        Meter { device: device.to_owned(), child, samples, running }
    }

    /// Whether the input still delivers (a stopped source is reopened, as Qt's was).
    pub fn running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Everything read since the last call, as `readAll()` every 50 ms did. Whole samples only.
    pub fn take(&mut self) -> Vec<u8> {
        let mut bytes = Vec::new();
        while let Ok(chunk) = self.samples.try_recv() {
            bytes.extend(chunk);
        }
        bytes.truncate(bytes.len() & !1);
        bytes
    }
}

impl Drop for Meter {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// `pactl subscribe`, raising a flag when a source comes or goes or the default one changes.
pub struct Watch {
    child: Option<Child>,
    changed: Arc<AtomicBool>,
}

impl Watch {
    pub fn start() -> Watch {
        let changed = Arc::new(AtomicBool::new(false));
        let mut child = Command::new("pactl").arg("subscribe").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok();
        if let Some(out) = child.as_mut().and_then(|c| c.stdout.take()) {
            let changed = changed.clone();
            std::thread::Builder::new()
                .name("audio-devices".into())
                .spawn(move || {
                    for line in BufReader::new(out).lines().map_while(Result::ok) {
                        // "Event 'new' on source #81", "Event 'remove' on source #81",
                        // "Event 'change' on server #-1" (the default source moved).
                        let source = line.contains("on source ") && (line.contains("'new'") || line.contains("'remove'"));
                        if source || line.contains("on server") {
                            changed.store(true, Ordering::SeqCst);
                        }
                    }
                })
                .ok();
        }
        Watch { child, changed }
    }

    /// True once after the sources changed.
    pub fn take_changed(&self) -> bool {
        self.changed.swap(false, Ordering::SeqCst)
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

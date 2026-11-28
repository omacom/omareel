//! The recording's sound as speech engines want it: 16 kHz mono, read with the ffmpeg command
//! into memory (one hour is 115 MB of samples), and written back out as WAV pieces.

use super::job::{self, Control, Stopped};
use crate::compat::qstandardpaths;
use std::io::Write;
use std::path::Path;
use std::process::Command;

/// The speech engines' rate.
pub const SAMPLE_RATE: u32 = 16_000;

/// Mono 16-bit samples at [`SAMPLE_RATE`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Audio {
    pub samples: Vec<i16>,
}

impl Audio {
    pub fn duration(&self) -> f64 {
        self.samples.len() as f64 / SAMPLE_RATE as f64
    }

    /// The samples from `start` to `end` seconds, clamped to the audio.
    pub fn slice(&self, start: f64, end: f64) -> &[i16] {
        let at = |seconds: f64| ((seconds.max(0.0) * SAMPLE_RATE as f64).round() as usize).min(self.samples.len());
        let (from, to) = (at(start), at(end));
        &self.samples[from..to.max(from)]
    }
}

/// The ffmpeg arguments that read `media`'s first audio stream as [`Audio`] on standard output.
///
/// Sample 0 is time 0 of the file, as its video and the editor play it: a recording's sound is
/// muxed in with `-itsoffset` (it starts when the first sample came, often some tenths of a
/// second after the screen), so the stream may start after 0, and `aresample` pads the start
/// with silence from 0 (and fills any gap the same way).
pub fn extract_arguments(media: &str) -> Vec<String> {
    let rate = SAMPLE_RATE.to_string();
    ["-nostdin", "-v", "error", "-i", media, "-map", "0:a:0", "-vn", "-af", "aresample=async=1:first_pts=0"]
        .into_iter()
        .chain(["-ac", "1", "-ar", &rate, "-f", "s16le", "-acodec", "pcm_s16le", "pipe:1"])
        .map(str::to_owned)
        .collect()
}

/// The first audio stream of `media` (all its channels mixed down), or an error naming what
/// went wrong. A file with no audio stream is an error too.
pub fn extract(media: &str) -> Result<Audio, String> {
    extract_with(media, &Control::default())
}

/// [`extract`] that stops when `control` is cancelled, and waits while it holds (reading again
/// from the start afterwards).
pub fn extract_with(media: &str, control: &Control) -> Result<Audio, String> {
    let ffmpeg = qstandardpaths::find_executable("ffmpeg");
    if ffmpeg.is_empty() {
        return Err("ffmpeg is not installed".into());
    }
    let output = loop {
        if !control.proceed() {
            return Err("cancelled".into());
        }
        let mut command = Command::new(&ffmpeg);
        command.args(extract_arguments(media));
        match job::run(command, None, control) {
            Ok(output) => break output,
            Err(Stopped::Held) => continue,
            Err(Stopped::Cancelled) => return Err("cancelled".into()),
            Err(Stopped::TimedOut) => return Err("ffmpeg took too long".into()),
            Err(Stopped::Failed(error)) => return Err(error),
        }
    };
    if output.code != 0 {
        let reason = output.stderr.lines().rev().find(|line| !line.trim().is_empty()).unwrap_or("no audio stream");
        return Err(format!("could not read the recording's audio: {}", reason.trim()));
    }
    let samples = output.stdout.chunks_exact(2).map(|pair| i16::from_le_bytes([pair[0], pair[1]])).collect();
    Ok(Audio { samples })
}

/// A 16-bit mono WAV file of `samples` at [`SAMPLE_RATE`].
pub fn wav_bytes(samples: &[i16]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

pub fn write_wav(path: &Path, samples: &[i16]) -> Result<(), String> {
    let mut file = std::fs::File::create(path).map_err(|error| format!("{}: {error}", path.display()))?;
    file.write_all(&wav_bytes(samples)).map_err(|error| format!("{}: {error}", path.display()))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn wav_header_describes_the_samples() {
        let bytes = wav_bytes(&[0, 1, -1]);
        assert_eq!(bytes.len(), 44 + 6);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(bytes[24..28].try_into().unwrap()), 16_000);
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 6);
        assert_eq!(&bytes[44..], &[0, 0, 1, 0, 0xff, 0xff]);
    }

    /// A bundle-style screen.mp4 in `dir`: 3 s of video and a 1 s tone muxed in `offset`
    /// seconds late with `-itsoffset`, as `omareel_record::recorder::audio::mux_arguments`
    /// does with the recorder's AAC sound. None without ffmpeg.
    pub(crate) fn late_audio_recording(dir: &Path, offset: f64) -> Option<String> {
        if !Command::new("ffmpeg").arg("-version").output().is_ok_and(|o| o.status.success()) {
            return None;
        }
        std::fs::create_dir_all(dir).unwrap();
        let path = |name: &str| dir.join(name).to_string_lossy().into_owned();
        let ffmpeg = |args: &[&str]| {
            let status = Command::new("ffmpeg").args(["-nostdin", "-v", "error", "-y"]).args(args).status().unwrap();
            assert!(status.success(), "{args:?}");
        };
        ffmpeg(&["-f", "lavfi", "-i", "testsrc=size=160x90:rate=30", "-t", "3", "-c:v", "libx264", "-pix_fmt", "yuv420p", &path("v.mp4")]);
        ffmpeg(&["-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000", "-t", "1", "-c:a", "aac", &path("a.m4a")]);
        let offset = format!("{offset:.6}");
        ffmpeg(&[
            "-i", &path("v.mp4"), "-itsoffset", &offset, "-i", &path("a.m4a"), "-map", "0:v:0", "-map", "1:a:0", "-c", "copy",
            "-t", "3.000000", &path("screen.mp4"),
        ]);
        Some(path("screen.mp4"))
    }

    /// Where a tone starts in 16-bit samples at `rate`, in seconds.
    pub(crate) fn onset(samples: &[i16], rate: u32) -> f64 {
        samples.iter().position(|s| s.unsigned_abs() > 1000).unwrap() as f64 / rate as f64
    }

    /// The recording's sound starts late in the file; its samples still count from time 0.
    #[test]
    fn audio_that_starts_late_keeps_its_place() {
        let dir = std::env::temp_dir().join(format!("omareel-audio-offset-{}", std::process::id()));
        let Some(screen) = late_audio_recording(&dir, 0.7) else {
            eprintln!("skipped: no ffmpeg");
            return;
        };
        let audio = extract(&screen).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let onset = onset(&audio.samples, SAMPLE_RATE);
        assert!((onset - 0.7).abs() < 0.03, "the tone starts at {onset}");
        assert!((audio.duration() - 1.7).abs() < 0.05, "{}", audio.duration());
    }

    #[test]
    fn slice_clamps() {
        let audio = Audio { samples: vec![0; 16_000] };
        assert_eq!(audio.slice(0.5, 2.0).len(), 8_000);
        assert_eq!(audio.slice(-1.0, 0.25).len(), 4_000);
        assert!(audio.slice(0.8, 0.2).is_empty());
    }
}

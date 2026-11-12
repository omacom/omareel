//! The recording's sound as speech engines want it: 16 kHz mono, read with the ffmpeg command
//! into memory (one hour is 115 MB of samples), and written back out as WAV pieces.

use crate::compat::qstandardpaths;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

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

/// The first audio stream of `media` (all its channels mixed down), or an error naming what
/// went wrong. A file with no audio stream is an error too.
pub fn extract(media: &str) -> Result<Audio, String> {
    let ffmpeg = qstandardpaths::find_executable("ffmpeg");
    if ffmpeg.is_empty() {
        return Err("ffmpeg is not installed".into());
    }
    let rate = SAMPLE_RATE.to_string();
    let mut child = Command::new(&ffmpeg)
        .args(["-nostdin", "-v", "error", "-i", media, "-map", "0:a:0", "-vn", "-ac", "1", "-ar", &rate])
        .args(["-f", "s16le", "-acodec", "pcm_s16le", "pipe:1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("could not run ffmpeg: {error}"))?;
    let mut stderr = child.stderr.take();
    let errors = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(stderr) = stderr.as_mut() {
            let _ = stderr.read_to_string(&mut text);
        }
        text
    });
    let mut bytes = Vec::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = stdout.read_to_end(&mut bytes);
    }
    let status = child.wait().map_err(|error| format!("ffmpeg failed: {error}"))?;
    let errors = errors.join().unwrap_or_default();
    if !status.success() {
        let reason = errors.lines().rev().find(|line| !line.trim().is_empty()).unwrap_or("no audio stream");
        return Err(format!("could not read the recording's audio: {}", reason.trim()));
    }
    let samples = bytes.chunks_exact(2).map(|pair| i16::from_le_bytes([pair[0], pair[1]])).collect();
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
mod tests {
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

    #[test]
    fn slice_clamps() {
        let audio = Audio { samples: vec![0; 16_000] };
        assert_eq!(audio.slice(0.5, 2.0).len(), 8_000);
        assert_eq!(audio.slice(-1.0, 0.25).len(), 4_000);
        assert!(audio.slice(0.8, 0.2).is_empty());
    }
}

//! The arithmetic of `MicrophoneMeter` (`src/ui/MicrophoneMeter.cpp`), which listens to a
//! microphone while the launcher is open so it can show that sound is arriving before a recording
//! starts: the peak of a buffer of samples as a 0–1 level (0 at -60 dBFS or below, 1 at full
//! scale) and the meter's rise and fall. The audio source itself stays on the Qt side, which
//! hands the bytes it read in with their [`SampleFormat`].

use crate::compat::cpp;

/// `QAudioFormat::SampleFormat`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SampleFormat {
    #[default]
    Unknown,
    UInt8,
    Int16,
    Int32,
    Float,
}

impl SampleFormat {
    /// `QAudioFormat::bytesPerSample`: 0 for `Unknown`.
    pub fn bytes_per_sample(self) -> usize {
        match self {
            SampleFormat::Unknown => 0,
            SampleFormat::UInt8 => 1,
            SampleFormat::Int16 => 2,
            SampleFormat::Int32 | SampleFormat::Float => 4,
        }
    }

    /// `QAudioFormat::normalizedSampleValue` of the sample at the start of `sample` (native byte
    /// order), in Qt's float arithmetic: unsigned bytes are `x / 127 - 1` (so 0 is full scale and
    /// 255 is just past it), signed integers `x / max`, floats as they are.
    pub fn normalized_sample_value(self, sample: &[u8]) -> f32 {
        let bytes = |n: usize| -> Option<&[u8]> { sample.get(..n) };
        match self {
            SampleFormat::UInt8 => {
                bytes(1).map_or(0.0, |b| (f64::from(f32::from(b[0]) / f32::from(i8::MAX as u8)) - 1.0) as f32)
            }
            SampleFormat::Int16 => {
                bytes(2).map_or(0.0, |b| f32::from(i16::from_ne_bytes([b[0], b[1]])) / f32::from(i16::MAX))
            }
            SampleFormat::Int32 => {
                bytes(4).map_or(0.0, |b| i32::from_ne_bytes([b[0], b[1], b[2], b[3]]) as f32 / i32::MAX as f32)
            }
            SampleFormat::Float => bytes(4).map_or(0.0, |b| f32::from_ne_bytes([b[0], b[1], b[2], b[3]])),
            SampleFormat::Unknown => 0.0,
        }
    }
}

/// `MicrophoneMeter::peakLevel(format, samples)`: the loudest whole sample's magnitude in dBFS,
/// mapped from -60..0 onto 0..1; 0 when nothing is louder than silence or the format has no
/// sample size. A trailing partial sample is ignored, as is every channel boundary (each sample
/// counts on its own).
pub fn peak_level(format: SampleFormat, samples: &[u8]) -> f64 {
    let bytes = format.bytes_per_sample();
    if bytes == 0 {
        return 0.0;
    }
    let mut peak = 0.0f32;
    let mut offset = 0;
    while offset + bytes <= samples.len() {
        // std::max: a NaN sample never wins.
        peak = cpp::max(peak, format.normalized_sample_value(&samples[offset..]).abs());
        offset += bytes;
    }
    if peak <= 0.0 {
        return 0.0;
    }
    cpp::clamp((20.0 * f64::from(peak).log10() + 60.0) / 60.0, 0.0, 1.0)
}

/// The level `MicrophoneMeter` shows, and how it moves with each read.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MicrophoneLevel {
    level: f64,
}

impl MicrophoneLevel {
    /// `level()`: 0 at -60 dBFS or below, 1 at full scale.
    pub fn level(&self) -> f64 {
        self.level
    }

    /// `MicrophoneMeter::read` after its device check (lines 66–71): rises with each peak and
    /// falls back by a quarter per read, like a VU meter; below 0.01 it is 0, and a change under
    /// 0.005 is ignored. True when the level changed, where the meter emits `levelChanged`.
    pub fn read(&mut self, format: SampleFormat, samples: &[u8]) -> bool {
        let mut level = cpp::max(peak_level(format, samples), self.level * 0.75);
        if level < 0.01 {
            level = 0.0;
        }
        if (level - self.level).abs() < 0.005 {
            return false;
        }
        self.level = level;
        true
    }

    /// `MicrophoneMeter::stop`'s reset: true when the level was not already 0 (`levelChanged`).
    pub fn reset(&mut self) -> bool {
        if self.level == 0.0 {
            return false;
        }
        self.level = 0.0;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_normalize_as_qt_does() {
        assert_eq!(SampleFormat::UInt8.normalized_sample_value(&[0]), -1.0);
        assert_eq!(SampleFormat::UInt8.normalized_sample_value(&[127]), 0.0);
        assert_eq!(SampleFormat::Int16.normalized_sample_value(&i16::MIN.to_ne_bytes()), -32768.0 / 32767.0);
        assert_eq!(SampleFormat::Int32.normalized_sample_value(&i32::MAX.to_ne_bytes()), 1.0);
        assert_eq!(SampleFormat::Unknown.normalized_sample_value(&[1, 2, 3, 4]), 0.0);
        assert_eq!(peak_level(SampleFormat::Int16, &[0, 0, 0]), 0.0);
        assert_eq!(peak_level(SampleFormat::Unknown, &[0xff; 8]), 0.0);
        let mut meter = MicrophoneLevel::default();
        assert!(meter.read(SampleFormat::Int16, &i16::MAX.to_ne_bytes()));
        assert_eq!(meter.level(), 1.0);
        assert!(meter.read(SampleFormat::Int16, &[0, 0]));
        assert_eq!(meter.level(), 0.75);
        assert!(meter.reset() && !meter.reset());
    }
}

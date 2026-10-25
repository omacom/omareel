//! microphone: `microphone_meter::peak_level` and `MicrophoneLevel::read` against
//! `Omareel::MicrophoneMeter::peakLevel` and the meter's update in `MicrophoneMeter::read`
//! (README "microphone"). The level goes through glibc's `log10`, so this runs on x86_64 only.

use crate::support::*;
use omareel_core::compat::qjson::Value;
use omareel_core::microphone_meter::{MicrophoneLevel, SampleFormat, peak_level};

fn sample_format(name: &str) -> SampleFormat {
    match name {
        "UInt8" => SampleFormat::UInt8,
        "Int16" => SampleFormat::Int16,
        "Int32" => SampleFormat::Int32,
        "Float" => SampleFormat::Float,
        _ => SampleFormat::Unknown,
    }
}

/// `QByteArray::fromHex`: digits read from the end in pairs, anything else skipped; an odd digit
/// left at the front is a byte of its own.
fn from_hex(text: &str) -> Vec<u8> {
    let mut result = Vec::new();
    let mut low: Option<u8> = None;
    for c in text.bytes().rev() {
        let Some(digit) = (c as char).to_digit(16) else { continue };
        match low.take() {
            None => low = Some(digit as u8),
            Some(l) => result.push(((digit as u8) << 4) | l),
        }
    }
    if let Some(l) = low {
        result.push(l);
    }
    result.reverse();
    result
}

#[test]
fn peak_level_and_meter_txt() {
    let cases = read_json("microphone/cases.json");
    let buffers = cases.get("buffers").to_object();
    let channels: Vec<i32> = cases.get("channels").to_array().iter().map(|c| c.to_int(0)).collect();
    let hex_of = |value: &Value| value.to_string_or("");
    let mut out = Text::new();
    for format_name in ["UInt8", "Int16", "Int32", "Float"] {
        let set = buffers.value(format_name).to_object();
        for channel_count in &channels {
            // The channel count does not change the result: every sample counts on its own.
            for (name, value) in set.iter() {
                let level = peak_level(sample_format(format_name), &from_hex(&hex_of(value)));
                out.row([
                    format_name.to_owned(),
                    channel_count.to_string(),
                    name.to_owned(),
                    hex_of(value),
                    num(level),
                    hex(level),
                ]);
            }
        }
    }
    // Buffers of every format read as each other format, and a format without a sample type.
    for format_name in ["UInt8", "Int16", "Int32", "Float", "Unknown"] {
        for source in ["Int16", "Float"] {
            if format_name == source {
                continue;
            }
            for (name, value) in buffers.value(source).to_object().iter() {
                let level = peak_level(sample_format(format_name), &from_hex(&hex_of(value)));
                out.row([
                    format_name.to_owned(),
                    "1".into(),
                    format!("{source}:{name}"),
                    hex_of(value),
                    num(level),
                    hex(level),
                ]);
            }
        }
    }

    let mut meter = Text::new();
    for (label, sequence) in cases.get("sequences").to_object().iter() {
        let sequence = sequence.to_array();
        let format_name = sequence[0].to_string_or("");
        let set = buffers.value(&format_name).to_object();
        meter.section(label);
        let mut state = MicrophoneLevel::default();
        for name in sequence[1].to_array() {
            let name = name.to_string_or("");
            let changed = state.read(sample_format(&format_name), &from_hex(&hex_of(set.value(&name))));
            meter.row([name, num(state.level()), hex(state.level()), boolean(changed).to_owned()]);
        }
    }
    assert_no_differences(
        [
            difference(
                "microphone/peak-level.txt",
                &read_expected("microphone/peak-level.txt"),
                out.into_string().as_bytes(),
            ),
            difference("microphone/meter.txt", &read_expected("microphone/meter.txt"), meter.into_string().as_bytes()),
        ]
        .into_iter()
        .flatten()
        .collect(),
    );
}

#[test]
fn from_hex_reads_like_qt() {
    assert_eq!(from_hex("00ff"), [0x00, 0xff]);
    assert_eq!(from_hex("abc"), [0x0a, 0xbc]);
    assert_eq!(from_hex("a-b"), [0xab]);
    assert!(from_hex("").is_empty());
}

//! The encoder probe of `ScreenCapture.cpp`: whether ffmpeg can encode with NVENC
//! (`preferredEncoderAvailable`) and convert BGRA frames on the GPU (`gpuConversionAvailable`),
//! asked once per ffmpeg binary and NVIDIA driver and remembered in
//! `$XDG_RUNTIME_DIR/omareel/encoder-probe.json` (`encoderProbeResults`), so a recording does not
//! wait for the probes every time. The runtime directory is a tmpfs, so the probes run again once
//! per login, beside the capture setup, and nothing is written to disk for them.

use omareel_core::compat::qdir::{self, FileInfo};
use omareel_core::compat::qjson::{self, Format, Object, Value};
use omareel_core::compat::qprocess::Process;
use omareel_core::compat::qsavefile::SaveFile;
use omareel_core::compat::qstandardpaths;
use omareel_core::compat::{qfile, qstring};
use crate::recorder::state::state_directory;

/// `EncoderProbeResults`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EncoderProbeResults {
    /// NVENC encodes (`h264_nvenc`).
    pub preferred: bool,
    /// CUDA converts BGRA to NV12 (`hwupload_cuda,scale_cuda`).
    pub gpu_conversion: bool,
    /// The answer came from the cache.
    pub cached: bool,
}

const PREFERRED_PROBE: [&str; 14] = [
    "-hide_banner",
    "-loglevel",
    "error",
    "-f",
    "lavfi",
    "-i",
    "color=size=256x256:rate=1",
    "-frames:v",
    "1",
    "-c:v",
    "h264_nvenc",
    "-f",
    "null",
    "-",
];

const GPU_CONVERSION_PROBE: [&str; 22] = [
    "-hide_banner",
    "-loglevel",
    "error",
    "-f",
    "rawvideo",
    "-pix_fmt",
    "bgra",
    "-s",
    "256x256",
    "-r",
    "1",
    "-i",
    "pipe:0",
    "-frames:v",
    "1",
    "-vf",
    "hwupload_cuda,scale_cuda=format=nv12",
    "-c:v",
    "h264_nvenc",
    "-f",
    "null",
    "-",
];

/// `preferredEncoderAvailable`: one frame of a colour source through `h264_nvenc`.
fn preferred_encoder_available() -> bool {
    let mut probe = Process::new();
    probe.start("ffmpeg", &PREFERRED_PROBE);
    probe.wait_for_finished(10000) && probe.exit_code() == 0
}

/// `gpuConversionAvailable`: one black 256x256 BGRA frame on standard input, converted with
/// CUDA and encoded with `h264_nvenc`.
fn gpu_conversion_available() -> bool {
    let mut probe = Process::new();
    probe.start("ffmpeg", &GPU_CONVERSION_PROBE);
    if !probe.wait_for_started(5000) {
        return false;
    }
    probe.write(&vec![0u8; 256 * 256 * 4]);
    probe.close_write_channel();
    probe.wait_for_finished(10000) && probe.exit_code() == 0
}

/// `encoderProbeKey`: what the probes' answers depend on: the ffmpeg on `PATH` (its canonical
/// path, modification time in milliseconds and size; empty and 0 when there is none) and the
/// NVIDIA driver version from `/sys/module/nvidia/version` ("absent" without the module).
pub fn encoder_probe_key() -> Object {
    let binary = qstandardpaths::find_executable("ffmpeg");
    let executable = FileInfo::new(binary);
    let driver_version = match qfile::read_all("/sys/module/nvidia/version") {
        Ok(bytes) => qstring::trimmed(&qstring::from_utf8(&bytes)).to_owned(),
        Err(_) => "absent".to_owned(),
    };
    let mut key = Object::new();
    key.insert("binary", executable.canonical_file_path());
    key.insert("mtime_ms", executable.last_modified_msecs());
    key.insert("size", executable.size());
    key.insert("driver", driver_version);
    key
}

/// `encoderProbeCachePath`: `encoder-probe.json` beside the recorder's state file, in the
/// runtime directory.
pub fn encoder_probe_cache_path() -> String {
    qdir::file_path(&state_directory(), "encoder-probe.json")
}

/// `QJsonDocument::fromJson(bytes).object()` of the cache file, if it can be read.
fn read_cache() -> Option<Object> {
    let bytes = qfile::read_all(encoder_probe_cache_path()).ok()?;
    Some(match qjson::from_json(&bytes) {
        Ok(Value::Object(object)) => object,
        _ => Object::new(),
    })
}

/// `encoderProbeResults`: the cached answers when the cache's key is the current one, else both
/// probes run at once and their answers cached (a cache that cannot be written is left alone).
pub fn encoder_probe_results() -> EncoderProbeResults {
    let key = encoder_probe_key();
    if let Some(cached) = read_cache() {
        if cached.value("key").to_object() == key {
            return EncoderProbeResults {
                preferred: cached.value("preferred").to_bool(false),
                gpu_conversion: cached.value("gpuConversion").to_bool(false),
                cached: true,
            };
        }
    }
    let (preferred, gpu_conversion) = std::thread::scope(|scope| {
        let preferred = scope.spawn(preferred_encoder_available);
        let conversion = scope.spawn(gpu_conversion_available);
        (preferred.join().unwrap_or(false), conversion.join().unwrap_or(false))
    });
    let results = EncoderProbeResults { preferred, gpu_conversion, cached: false };
    let cache_path = encoder_probe_cache_path();
    qdir::mkpath(&FileInfo::new(cache_path.clone()).absolute_path());
    let mut output = SaveFile::new(cache_path);
    if output.open() {
        let mut cache = Object::new();
        cache.insert("key", key);
        cache.insert("preferred", results.preferred);
        cache.insert("gpuConversion", results.gpu_conversion);
        output.write(Value::Object(cache).to_json(Format::Indented).as_bytes());
        output.commit();
    }
    results
}

/// `cachedPreferredEncoderAvailable`: what the cache says about NVENC, without probing; false
/// without a cache or with a stale one.
pub fn cached_preferred_encoder_available() -> bool {
    let Some(cached) = read_cache() else { return false };
    cached.value("key").to_object() == encoder_probe_key() && cached.value("preferred").to_bool(false)
}

//! Which GPU the export may hand video to: CUDA where NVIDIA's driver is loaded, and VA-API on
//! a render node driven by Intel's or AMD's kernel driver. NVIDIA's VA-API driver is never used:
//! on the N1X, `nvidia-vaapi-driver` segfaults or hangs reading frames back (see
//! `src/render/VideoDecoding.cpp`).

use std::path::Path;

/// Kernel drivers whose render nodes have a working VA-API driver (iHD or i965 for Intel,
/// radeonsi for AMD). Anything else (nvidia, nouveau, virtual GPUs) is left alone.
const VAAPI_KERNEL_DRIVERS: [&str; 4] = ["i915", "xe", "amdgpu", "radeon"];

/// The kernel driver behind `<drm>/<node>/device/driver`, "" when there is none.
fn kernel_driver(drm: &Path, node: &str) -> String {
    std::fs::read_link(drm.join(node).join("device/driver"))
        .ok()
        .and_then(|target| target.file_name().map(|name| name.to_string_lossy().into_owned()))
        .unwrap_or_default()
}

/// The render nodes under `drm` (`/sys/class/drm`), in number order.
fn render_nodes(drm: &Path) -> Vec<String> {
    let mut nodes: Vec<(u32, String)> = std::fs::read_dir(drm)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    let number = name.strip_prefix("renderD")?.parse().ok()?;
                    Some((number, name))
                })
                .collect()
        })
        .unwrap_or_default();
    nodes.sort();
    nodes.into_iter().map(|(_, name)| name).collect()
}

/// The VA-API device for the render nodes under `drm`, with `LIBVA_DRIVER_NAME` as
/// `libva_driver`: the first node an Intel or AMD kernel driver runs, as `/dev/dri/<node>`, or
/// none when libva is told to load NVIDIA's driver whatever the node.
pub fn vaapi_device_in(drm: &Path, libva_driver: Option<&str>) -> Option<String> {
    if libva_driver.is_some_and(|driver| driver == "nvidia") {
        return None;
    }
    render_nodes(drm)
        .into_iter()
        .find(|node| VAAPI_KERNEL_DRIVERS.contains(&kernel_driver(drm, node).as_str()))
        .map(|node| format!("/dev/dri/{node}"))
}

/// This machine's VA-API device, if it has one export may use.
pub fn vaapi_device() -> Option<String> {
    let libva_driver = std::env::var("LIBVA_DRIVER_NAME").ok().filter(|driver| !driver.is_empty());
    vaapi_device_in(Path::new("/sys/class/drm"), libva_driver.as_deref())
        .filter(|device| Path::new(device).exists())
}

/// Whether NVIDIA's kernel driver is loaded, which CUDA needs.
pub fn nvidia_loaded() -> bool {
    Path::new("/sys/module/nvidia").exists()
}

/// How a decoder asks ffmpeg to decode on the GPU.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HwAccel {
    Cuda,
    Vaapi(String),
}

impl HwAccel {
    /// ffmpeg's input options. Frames come back to memory as the decoder made them; the
    /// pipeline reads raw frames, so there is nothing to gain from keeping them on the GPU.
    pub fn arguments(&self) -> Vec<String> {
        match self {
            HwAccel::Cuda => vec!["-hwaccel".into(), "cuda".into()],
            HwAccel::Vaapi(device) => {
                vec!["-hwaccel".into(), "vaapi".into(), "-hwaccel_device".into(), device.clone()]
            }
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            HwAccel::Cuda => "cuda",
            HwAccel::Vaapi(_) => "vaapi",
        }
    }
}

/// What the decoders try before decoding in software, for `OMAREEL_EXPORT_DECODER` (`choice`),
/// with NVIDIA's driver loaded or not and the VA-API device: CUDA when NVIDIA's driver is
/// loaded, unless `choice` is "software" or "vaapi"; VA-API only when `choice` is "vaapi".
///
/// VA-API is not the default because it is a loss here. The frames have to come back to memory
/// for the pipe, and on an Iris Xe (i9-13900H) reading NV12 surfaces back cost more CPU than
/// decoding in software saved: exporting a 4K recording took 230 s of CPU instead of 210 s,
/// and no less time. Software decoding of screen recordings is cheap.
pub fn decode_accelerations_for(choice: &str, nvidia: bool, vaapi: Option<String>) -> Vec<HwAccel> {
    let mut accelerations = Vec::new();
    if nvidia && matches!(choice, "cuda" | "") {
        accelerations.push(HwAccel::Cuda);
    }
    if let Some(device) = vaapi.filter(|_| choice == "vaapi") {
        accelerations.push(HwAccel::Vaapi(device));
    }
    accelerations
}

/// [`decode_accelerations_for`] this machine.
pub fn decode_accelerations() -> Vec<HwAccel> {
    let choice = std::env::var("OMAREEL_EXPORT_DECODER").unwrap_or_default();
    decode_accelerations_for(&choice, nvidia_loaded(), vaapi_device())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;

    /// A fake `/sys/class/drm` with each render node's device bound to a driver.
    fn drm(name: &str, nodes: &[(&str, &str)]) -> PathBuf {
        let root = std::env::temp_dir().join(format!("omareel-drm-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (node, driver) in nodes {
            let device = root.join(node).join("device");
            std::fs::create_dir_all(&device).unwrap();
            symlink(format!("../../../bus/pci/drivers/{driver}"), device.join("driver")).unwrap();
        }
        std::fs::create_dir_all(root.join("card0")).unwrap();
        root
    }

    #[test]
    fn vaapi_skips_nvidia_render_nodes() {
        let hybrid = drm("hybrid", &[("renderD128", "nvidia"), ("renderD129", "i915")]);
        assert_eq!(vaapi_device_in(&hybrid, None).as_deref(), Some("/dev/dri/renderD129"));
        assert_eq!(vaapi_device_in(&hybrid, Some("iHD")).as_deref(), Some("/dev/dri/renderD129"));
        assert_eq!(vaapi_device_in(&hybrid, Some("nvidia")), None);
        let nvidia = drm("nvidia", &[("renderD128", "nvidia"), ("renderD129", "nouveau")]);
        assert_eq!(vaapi_device_in(&nvidia, None), None);
        let amd = drm("amd", &[("renderD130", "amdgpu"), ("renderD129", "xe")]);
        assert_eq!(vaapi_device_in(&amd, None).as_deref(), Some("/dev/dri/renderD129"));
        assert_eq!(vaapi_device_in(Path::new("/nonexistent"), None), None);
        for root in [hybrid, nvidia, amd] {
            let _ = std::fs::remove_dir_all(root);
        }
    }

    #[test]
    fn decoders_use_cuda_and_vaapi_when_asked() {
        let intel = Some("/dev/dri/renderD128".to_string());
        assert_eq!(decode_accelerations_for("", false, intel.clone()), []);
        assert_eq!(decode_accelerations_for("", true, intel.clone()), [HwAccel::Cuda]);
        assert_eq!(decode_accelerations_for("cuda", false, intel.clone()), []);
        assert_eq!(decode_accelerations_for("vaapi", false, intel.clone()), [HwAccel::Vaapi("/dev/dri/renderD128".into())]);
        assert_eq!(decode_accelerations_for("vaapi", true, intel.clone()), [HwAccel::Vaapi("/dev/dri/renderD128".into())]);
        assert_eq!(decode_accelerations_for("vaapi", true, None), []);
        assert_eq!(decode_accelerations_for("software", true, intel), []);
        assert_eq!(
            HwAccel::Vaapi("/dev/dri/renderD129".into()).arguments(),
            ["-hwaccel", "vaapi", "-hwaccel_device", "/dev/dri/renderD129"]
        );
    }
}

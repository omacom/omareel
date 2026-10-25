//! mediaprobe: `media_probe::probe_video` and `probe_frame_times` against `Omareel::probeVideo`
//! and `probeFrameTimes` (README "mediaprobe"). The real ffprobe never runs: `shims/ffprobe`,
//! first on the hermetic `PATH`, replays the corpus recordings named after the file it is
//! given, and empty files stand in for the media, as the oracle had it.

use crate::hermetic::{Hermetic, write_file};
use crate::support::*;
use omareel_core::media_probe::{self, VideoInfo};

fn probe_row(h: &Hermetic, label: &str, path: &str) -> String {
    // The oracle set the error to "<untouched>" first; every return sets or clears it.
    let (info, error) = match media_probe::probe_video(path) {
        Ok(info) => (info, String::new()),
        Err(error) => (VideoInfo::default(), error),
    };
    [
        label.to_owned(),
        info.width.to_string(),
        info.height.to_string(),
        num(info.duration),
        num(info.fps),
        boolean(info.audio).to_owned(),
        boolean(info.is_valid()).to_owned(),
        quote(&h.sanitize(&error)),
    ]
    .join("\t")
}

fn frames_row(label: &str, path: &str) -> String {
    let times = media_probe::probe_frame_times(path);
    let mut cells = vec![label.to_owned(), times.len().to_string()];
    cells.extend(times.into_iter().map(num));
    cells.join("\t")
}

#[test]
fn probe_video_and_frame_times_txt() {
    let h = Hermetic::new("mediaprobe");
    let recordings = corpus_path("mediaprobe/ffprobe");
    h.set_env("OMAREEL_ORACLE_FFPROBE_DIR", &recordings);
    let files = format!("{}/files", h.scratch());
    std::fs::create_dir_all(&files).expect("mkpath files");
    let (mut probe, mut frames) = (String::new(), String::new());
    let mut both = |label: &str, path: &str| {
        probe += &probe_row(&h, label, path);
        probe.push('\n');
        frames += &frames_row(label, path);
        frames.push('\n');
    };
    for name in sorted_entries(&recordings, true) {
        let path = format!("{files}/{name}");
        write_file(&path, b"");
        both(&name, &path);
    }
    both("<missing file>", &format!("{files}/missing.mp4"));
    std::fs::create_dir_all(format!("{files}/directory.mp4")).expect("mkpath directory.mp4");
    both("<directory>", &format!("{files}/directory.mp4"));
    both("<empty path>", "");
    // Relative to the working directory ($HOME): ffprobe gets the path as it was given.
    both("<relative path>", "oracle-scratch/mediaprobe/files/testsrc-320x240-30.mp4");
    let path = std::env::var_os("PATH").expect("PATH");
    let empty_bin = format!("{}/empty-bin", h.scratch());
    std::fs::create_dir_all(&empty_bin).expect("mkpath empty-bin");
    h.set_env("PATH", &empty_bin);
    both("<ffprobe not on PATH>", &format!("{files}/testsrc-320x240-30.mp4"));
    h.set_env("PATH", path);
    h.unset_env("OMAREEL_ORACLE_FFPROBE_DIR");
    assert_no_differences(
        [
            difference("mediaprobe/probe-video.txt", &read_expected("mediaprobe/probe-video.txt"), probe.as_bytes()),
            difference(
                "mediaprobe/probe-frame-times.txt",
                &read_expected("mediaprobe/probe-frame-times.txt"),
                frames.as_bytes(),
            ),
        ]
        .into_iter()
        .flatten()
        .collect(),
    );
}

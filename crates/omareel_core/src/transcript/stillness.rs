//! Whether the screen stays still through the recording's silences, read from screen.mp4
//! itself. A silence is only a pause to trim when nothing happens on screen either: typing is
//! not in input.jsonl (only shortcuts and keys that type nothing are), and a video playing or
//! a build scrolling has no input at all.
//!
//! Each silence is decoded on its own (the recorder puts a keyframe every second, so seeking
//! costs little), five frames a second, shrunk by area averaging to cells of 8 logical pixels
//! in grey. A frame differs from the silence's first frame where a cell's level moved by more
//! than [`CELL_DIFFERENCE`]; the screen is still while everything that differed fits in one
//! character's box (a blinking text cursor), and moving once it spreads past that. Decoding
//! stops at the first movement, so busy silences cost little.

use super::job::{self, Control, Stopped};
use super::trim::{silences, PauseSettings};
use crate::compat::qstandardpaths;
use std::process::Command;

/// Silences at least this long are checked; shorter ones are never pauses.
pub const MIN_SILENCE: f64 = 0.5;
/// Frames looked at per second.
const FPS: f64 = 5.0;
/// Cell size in logical pixels.
const CELL: f64 = 8.0;
/// A cell whose grey level moved by more than this (of 255) changed.
const CELL_DIFFERENCE: u8 = 6;
/// The most change that is still "still", in cells: a blinking text cursor or one character.
const STILL_COLUMNS: usize = 3;
const STILL_ROWS: usize = 4;
/// The ends of a silence next to speech are not looked at: the screen often changes as you
/// start or stop talking, and those ends are kept anyway.
const EDGE: f64 = 0.15;

/// The grid `width` x `height` (pixels, at `scale` pixels per logical pixel) is shrunk to.
pub fn grid(width: i32, height: i32, scale: f64) -> (usize, usize) {
    let cell = CELL * if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    let side = |pixels: i32| ((pixels.max(1) as f64 / cell).round() as usize).max(1);
    (side(width), side(height))
}

/// Follows frames of a grid, telling whether they stayed still.
pub struct StillnessCheck {
    columns: usize,
    rows: usize,
    first: Option<Vec<u8>>,
    /// The box of cells that changed: left, top, right, bottom.
    changed: Option<(usize, usize, usize, usize)>,
}

impl StillnessCheck {
    pub fn new(columns: usize, rows: usize) -> StillnessCheck {
        StillnessCheck { columns, rows, first: None, changed: None }
    }

    pub fn frame_size(&self) -> usize {
        self.columns * self.rows
    }

    /// Takes the next frame (`frame_size` grey levels, row by row); false once the screen moved.
    pub fn push(&mut self, frame: &[u8]) -> bool {
        let Some(first) = &self.first else {
            self.first = Some(frame.to_vec());
            return true;
        };
        for (i, (&a, &b)) in first.iter().zip(frame).enumerate() {
            if a.abs_diff(b) > CELL_DIFFERENCE {
                let (column, row) = (i % self.columns, i / self.columns);
                self.changed = Some(match self.changed {
                    None => (column, row, column, row),
                    Some((l, t, r, b)) => (l.min(column), t.min(row), r.max(column), b.max(row)),
                });
            }
        }
        self.still()
    }

    pub fn still(&self) -> bool {
        self.changed.is_none_or(|(l, t, r, b)| r - l < STILL_COLUMNS && b - t < STILL_ROWS)
    }
}

/// The silences to check: every one of at least [`MIN_SILENCE`] between, before and after the
/// speech (see `trim::silences`). With no speech there is none.
pub fn silences_to_check(speech: &[(f64, f64)], duration: f64) -> Vec<(f64, f64)> {
    let settings = PauseSettings { min_pause: MIN_SILENCE, ..PauseSettings::default() };
    silences(speech, duration, &settings).into_iter().map(|silence| (silence.start, silence.end)).collect()
}

/// Of `windows` (source seconds of `video`, a `width` x `height` recording at `scale`), the ones
/// the screen stays still through, ends next to speech aside; `duration` is the recording's.
/// Stops when `control` is cancelled, and waits while it holds (a recording or an export),
/// checking the window it was on again afterwards.
pub fn still_windows(
    video: &str,
    windows: &[(f64, f64)],
    duration: f64,
    (width, height, scale): (i32, i32, f64),
    control: &Control,
) -> Result<Vec<(f64, f64)>, String> {
    let ffmpeg = qstandardpaths::find_executable("ffmpeg");
    if ffmpeg.is_empty() {
        return Err("ffmpeg is not installed".into());
    }
    let (columns, rows) = grid(width, height, scale);
    let mut still = Vec::new();
    for &(start, end) in windows {
        let from = if start > 1e-6 { start + EDGE } else { start };
        let to = if end < duration - 1e-6 { end - EDGE } else { end };
        if to - from < 1.0 / FPS {
            continue;
        }
        loop {
            if !control.proceed() {
                return Err("cancelled".into());
            }
            let mut check = StillnessCheck::new(columns, rows);
            let mut pending: Vec<u8> = Vec::new();
            let mut command = Command::new(&ffmpeg);
            command.args(["-nostdin", "-v", "error", "-ss", &format!("{from:.3}"), "-t", &format!("{:.3}", to - from)]);
            command.args(["-i", video, "-an", "-sn", "-vf"]);
            command.arg(format!("fps={FPS},scale={columns}:{rows}:flags=area,format=gray"));
            command.args(["-f", "rawvideo", "pipe:1"]);
            // A background job: the editor's playback comes first.
            use std::os::unix::process::CommandExt;
            // SAFETY: setpriority is async-signal-safe and touches only the new process.
            unsafe {
                command.pre_exec(|| {
                    libc::setpriority(libc::PRIO_PROCESS, 0, 10);
                    Ok(())
                });
            }
            let result = job::run_streaming(command, None, control, |chunk| {
                pending.extend_from_slice(chunk);
                let size = check.frame_size();
                let mut used = 0;
                while pending.len() - used >= size {
                    if !check.push(&pending[used..used + size]) {
                        return false;
                    }
                    used += size;
                }
                pending.drain(..used);
                true
            });
            match result {
                Ok(output) => {
                    if output.code != 0 {
                        let reason = output.stderr.lines().rev().find(|line| !line.trim().is_empty()).unwrap_or("");
                        return Err(format!("could not read the recording's video: {}", reason.trim()));
                    }
                    if check.still() {
                        still.push((start, end));
                    }
                    break;
                }
                Err(Stopped::Held) => continue,
                Err(Stopped::Cancelled) => return Err("cancelled".into()),
                Err(Stopped::TimedOut) => return Err("ffmpeg took too long".into()),
                Err(Stopped::Failed(error)) => return Err(error),
            }
        }
    }
    Ok(still)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_cells_are_eight_logical_pixels() {
        assert_eq!(grid(1920, 1080, 1.0), (240, 135));
        assert_eq!(grid(3840, 2160, 2.0), (240, 135));
        assert_eq!(grid(3840, 2160, 0.0), (480, 270));
        assert_eq!(grid(3, 3, 1.0), (1, 1));
    }

    #[test]
    fn a_blinking_cursor_is_still_typing_is_not() {
        let (columns, rows) = (20, 10);
        let blank = vec![30u8; columns * rows];
        let with = |cells: &[(usize, usize)]| {
            let mut frame = blank.clone();
            for &(column, row) in cells {
                frame[row * columns + column] = 200;
            }
            frame
        };
        // A cursor two cells wide and three tall blinking on and off.
        let cursor = [(5, 4), (6, 4), (5, 5), (6, 5), (5, 6), (6, 6)];
        let mut check = StillnessCheck::new(columns, rows);
        for i in 0..20 {
            assert!(check.push(&if i % 2 == 0 { blank.clone() } else { with(&cursor) }));
        }
        // Small noise never counts.
        let mut noisy = blank.clone();
        noisy[0] += 6;
        assert!(check.push(&noisy));
        // Typing: the cursor moves on and letters stay behind it.
        let mut check = StillnessCheck::new(columns, rows);
        check.push(&blank);
        assert!(check.push(&with(&[(5, 4), (5, 5), (5, 6)])));
        assert!(check.push(&with(&[(5, 4), (5, 5), (5, 6), (6, 5), (7, 4)])));
        assert!(!check.push(&with(&[(5, 4), (5, 5), (5, 6), (6, 5), (7, 4), (8, 5)])));
        // Scrolling changes everything.
        let mut check = StillnessCheck::new(columns, rows);
        check.push(&blank);
        assert!(!check.push(&vec![200u8; columns * rows]));
    }

    fn ffmpeg(args: &[&str]) {
        let status = Command::new("ffmpeg").args(["-nostdin", "-v", "error", "-y"]).args(args).status().unwrap();
        assert!(status.success(), "{args:?}");
    }

    /// A 6 s screen recording, still but for a blinking cursor, with text typed from 2 to 3 s
    /// (a letter every 0.1 s) and nothing else.
    #[test]
    fn checks_silences_against_the_video() {
        if !Command::new("ffmpeg").arg("-version").output().is_ok_and(|o| o.status.success()) {
            eprintln!("skipped: no ffmpeg");
            return;
        }
        let dir = std::env::temp_dir().join(format!("omareel-stillness-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let video = dir.join("screen.mp4").to_string_lossy().into_owned();
        // 320x180 at scale 1: a 40 x 22 grid. The cursor is 8x16 px and blinks twice a second; the
        // text grows by 8 px every 0.1 s from 2 s to 3 s.
        let filter = "drawbox=x=100:y=80:w=8:h=16:color=white:t=fill:enable='lt(mod(t,1),0.5)',\
                      drawbox=x=120:y=80:w='8*floor(min(max(t-2,0),1)*10)':h=16:color=white:t=fill:enable='gte(t,2.1)'";
        ffmpeg(&[
            "-f", "lavfi", "-i", "color=c=0x202020:size=320x180:rate=60", "-t", "6", "-vf", filter, "-c:v", "libx264", "-g", "60",
            "-bf", "0", "-pix_fmt", "yuv420p", &video,
        ]);
        let windows = [(0.0, 1.5), (1.8, 3.5), (4.0, 6.0)];
        let still = still_windows(&video, &windows, 6.0, (320, 180, 1.0), &Control::default()).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(still, vec![(0.0, 1.5), (4.0, 6.0)]);
    }
}

//! Blur regions baked into the recording's decoded pixels.
//!
//! The exporter hides what a blur overlay covers here, in the frame the decoder made, before
//! the composition ever sees it, so the result can't depend on the renderer: the GPU and the
//! software scene graph get the same redacted picture, and so do zooms, the crop and the
//! motion blur drawn from it. The preview draws the same grid on the GPU (OverlayLayer.qml).
//!
//! The grid: a region of `w` x `h` pixels splits into `round(w / block)` x `round(h / block)`
//! cells (at least one each way), where `block` is [`block_fraction`] of the frame's height.
//! "pixelate" fills each cell with its average; "blur" runs three box blurs of a block
//! each way, which approach a Gaussian, on a copy averaged down to a quarter block a pixel and
//! scaled back up (the same picture for a blur this wide, at a fraction of the work).

use crate::compat::geometry::RectF;
use crate::project::Overlay;
use std::sync::OnceLock;

/// The smallest cell (and blur radius) as a fraction of the recording's height, at `amount` 0
/// and 1. Text 80 px high on a 1080p screen is two cells high at the least.
const FLOOR: (f64, f64) = (0.04, 0.08);
/// How many cells high a region is at most, at `amount` 0 and 1: a bigger region gets bigger
/// cells, so the text it hides (as big as the region, at worst) never keeps its shape.
const ROWS: (f64, f64) = (8.0, 4.0);

/// The size of a pixelation cell (and the blur radius) as a fraction of the recording's
/// height, for an overlay's `amount` (0–1) and its region's height (a fraction of the
/// recording's height too): an eighth to a quarter of the region, and 4 % to 8 % of the frame
/// at the least.
pub fn block_fraction(amount: f64, region_height: f64) -> f64 {
    let amount = if amount.is_finite() { amount.clamp(0.0, 1.0) } else { 0.5 };
    let floor = FLOOR.0 + (FLOOR.1 - FLOOR.0) * amount;
    let rows = ROWS.0 + (ROWS.1 - ROWS.0) * amount;
    floor.max(region_height / rows)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Redaction {
    /// 0–1 of the frame.
    pub rect: RectF,
    pub blur: bool,
    /// [`block_fraction`].
    pub block: f64,
}

impl Redaction {
    pub fn of(overlay: &Overlay) -> Redaction {
        Redaction {
            rect: overlay.rect,
            blur: overlay.style == "blur",
            block: block_fraction(overlay.amount, overlay.rect.height),
        }
    }
}

/// A region in pixels, its grid, and the blur radius.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Area {
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
    cols: usize,
    rows: usize,
    radius: usize,
}

/// The pixels of `r` in a `width` x `height` frame, from the floor of its top left to the
/// ceiling of its bottom right (everything it touches), aligned out to `align`.
fn area(r: &Redaction, width: usize, height: usize, align: usize) -> Option<Area> {
    let w = width as f64;
    let h = height as f64;
    let down = |v: f64, limit: usize| ((v.floor().max(0.0) as usize).min(limit) / align) * align;
    let up = |v: f64, limit: usize| ((v.ceil().max(0.0) as usize).div_ceil(align) * align).min(limit);
    let x0 = down(r.rect.x * w, width);
    let y0 = down(r.rect.y * h, height);
    let x1 = up((r.rect.x + r.rect.width) * w, width);
    let y1 = up((r.rect.y + r.rect.height) * h, height);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let block = (r.block * h).max(1.0);
    let cols = (((x1 - x0) as f64 / block).round() as usize).max(1);
    let rows = (((y1 - y0) as f64 / block).round() as usize).max(1);
    Some(Area { x0, y0, x1, y1, cols, rows, radius: (block.round() as usize).max(1) })
}

impl Area {
    /// The same area on a plane subsampled by `factor` (YUV 4:2:0 chroma).
    fn scaled_down(self, factor: usize) -> Area {
        Area {
            x0: self.x0 / factor,
            y0: self.y0 / factor,
            x1: self.x1 / factor,
            y1: self.y1 / factor,
            radius: (self.radius / factor).max(1),
            ..self
        }
    }
}

/// How many threads share a pass over `bytes`: one for a small region, up to eight (each with
/// half a megabyte at least) for a big one.
fn threads_for(bytes: usize) -> usize {
    static CORES: OnceLock<usize> = OnceLock::new();
    let cores = *CORES.get_or_init(|| std::thread::available_parallelism().map_or(4, std::num::NonZeroUsize::get));
    cores.min(8).min(bytes >> 19).max(1)
}

/// Runs `work(band, rows)` for each of `bands` (consecutive row ranges, starting at `bands[0].0`
/// and ending where the next starts) over the rows of `data` (`stride` bytes each) it covers,
/// on up to `threads` threads, each with a run of consecutive bands.
fn for_bands<F>(data: &mut [u8], stride: usize, bands: &[(usize, usize)], threads: usize, work: F)
where
    F: Fn(usize, &mut [u8]) + Sync,
{
    let Some(&(first, _)) = bands.first() else { return };
    let mut rest = &mut data[first * stride..bands.last().map_or(first, |b| b.1) * stride];
    let mut slices = Vec::with_capacity(bands.len());
    for &(start, end) in bands {
        let (band, tail) = std::mem::take(&mut rest).split_at_mut((end - start) * stride);
        slices.push(band);
        rest = tail;
    }
    let threads = threads.clamp(1, slices.len());
    if threads == 1 {
        for (index, band) in slices.into_iter().enumerate() {
            work(index, band);
        }
        return;
    }
    let per = slices.len().div_ceil(threads);
    let work = &work;
    std::thread::scope(|scope| {
        let mut groups: Vec<Vec<(usize, &mut [u8])>> = Vec::new();
        for (index, band) in slices.into_iter().enumerate() {
            if index % per == 0 {
                groups.push(Vec::with_capacity(per));
            }
            groups.last_mut().unwrap().push((index, band));
        }
        let last = groups.pop();
        for group in groups {
            scope.spawn(move || group.into_iter().for_each(|(index, band)| work(index, band)));
        }
        last.into_iter().flatten().for_each(|(index, band)| work(index, band));
    });
}

/// `count` rows from `start` in `parts` bands about as high (none empty).
fn row_bands(start: usize, count: usize, parts: usize) -> Vec<(usize, usize)> {
    let parts = parts.clamp(1, count.max(1));
    (0..parts).map(|i| (start + i * count / parts, start + (i + 1) * count / parts)).filter(|b| b.1 > b.0).collect()
}

/// One plane: `channels` interleaved bytes per pixel, `stride` bytes per row, worked on by
/// `threads` threads (0: as many as the region's size calls for, [`threads_for`]).
struct Plane<'a> {
    data: &'a mut [u8],
    stride: usize,
    channels: usize,
    threads: usize,
}

impl Plane<'_> {
    fn threads_for(&self, a: &Area) -> usize {
        if self.threads > 0 { self.threads } else { threads_for((a.x1 - a.x0) * (a.y1 - a.y0) * self.channels) }
    }

    fn pixelate(&mut self, a: &Area) {
        let c = self.channels;
        let w = a.x1 - a.x0;
        let h = a.y1 - a.y0;
        let edges: Vec<usize> = (0..=a.cols).map(|col| a.x0 + col * w / a.cols).collect();
        let bands: Vec<(usize, usize)> =
            (0..a.rows).map(|row| (a.y0 + row * h / a.rows, a.y0 + (row + 1) * h / a.rows)).collect();
        let stride = self.stride;
        let threads = self.threads_for(a);
        for_bands(self.data, stride, &bands, threads, |_, band| {
            let rows = band.len() / stride;
            if rows == 0 {
                return;
            }
            let mut sums = vec![0u64; a.cols * c];
            for line in band.chunks_exact(stride) {
                for col in 0..a.cols {
                    let cell = &line[edges[col] * c..edges[col + 1] * c];
                    let sum = &mut sums[col * c..(col + 1) * c];
                    if c == 1 {
                        sum[0] += cell.iter().map(|&v| u64::from(v)).sum::<u64>();
                    } else {
                        for pixel in cell.chunks_exact(c) {
                            for (s, v) in sum.iter_mut().zip(pixel) {
                                *s += u64::from(*v);
                            }
                        }
                    }
                }
            }
            let mut means = vec![0u8; a.cols * c];
            for col in 0..a.cols {
                let count = ((edges[col + 1] - edges[col]) * rows) as u64;
                for ch in 0..c {
                    means[col * c + ch] = if count == 0 { 0 } else { ((sums[col * c + ch] + count / 2) / count) as u8 };
                }
            }
            for line in band.chunks_exact_mut(stride) {
                for col in 0..a.cols {
                    let mean = &means[col * c..(col + 1) * c];
                    let cell = &mut line[edges[col] * c..edges[col + 1] * c];
                    if c == 1 {
                        cell.fill(mean[0]);
                    } else {
                        for pixel in cell.chunks_exact_mut(c) {
                            pixel.copy_from_slice(mean);
                        }
                    }
                }
            }
        });
    }

    /// Three box blurs of `radius` across then down, inside the area (its edges repeat
    /// outwards; nothing from outside comes in). The area is averaged down by a quarter of the
    /// radius first and scaled back up after with bilinear filtering: the blur leaves nothing
    /// that fine, so the result is the same picture.
    fn blur(&mut self, a: &Area) {
        let c = self.channels;
        let w = a.x1 - a.x0;
        let h = a.y1 - a.y0;
        let factor = (a.radius / 4).max(1);
        let (sw, sh) = (w.div_ceil(factor), h.div_ceil(factor));
        let stride = self.stride;
        let threads = self.threads_for(a);

        // Averaged down: each small pixel is the mean of its factor x factor block (fewer at the
        // right and bottom edges).
        let mut small = vec![0f32; sw * sh * c];
        {
            let data: &[u8] = self.data;
            let bands = row_bands(0, sh, threads);
            let mut rest = small.as_mut_slice();
            let mut parts = Vec::new();
            for &(start, end) in &bands {
                let (part, tail) = std::mem::take(&mut rest).split_at_mut((end - start) * sw * c);
                parts.push((start, part));
                rest = tail;
            }
            let down = |start: usize, part: &mut [f32]| {
                let mut sums = vec![0u32; sw * c];
                for (i, out) in part.chunks_exact_mut(sw * c).enumerate() {
                    let sy = start + i;
                    let (ys, ye) = (a.y0 + sy * factor, (a.y0 + (sy + 1) * factor).min(a.y1));
                    sums.fill(0);
                    for y in ys..ye {
                        let line = &data[y * stride + a.x0 * c..y * stride + a.x1 * c];
                        if c == 1 {
                            for (sum, block) in sums.iter_mut().zip(line.chunks(factor)) {
                                *sum += block.iter().map(|&v| u32::from(v)).sum::<u32>();
                            }
                            continue;
                        }
                        for (sx, block) in line.chunks(factor * c).enumerate() {
                            let sum = &mut sums[sx * c..(sx + 1) * c];
                            for pixel in block.chunks_exact(c) {
                                for (s, v) in sum.iter_mut().zip(pixel) {
                                    *s += u32::from(*v);
                                }
                            }
                        }
                    }
                    for sx in 0..sw {
                        let count = ((ye - ys) * ((a.x0 + (sx + 1) * factor).min(a.x1) - (a.x0 + sx * factor))) as f32;
                        for ch in 0..c {
                            out[sx * c + ch] = sums[sx * c + ch] as f32 / count;
                        }
                    }
                }
            };
            if parts.len() <= 1 {
                parts.into_iter().for_each(|(start, part)| down(start, part));
            } else {
                let down = &down;
                std::thread::scope(|scope| {
                    for (start, part) in parts {
                        scope.spawn(move || down(start, part));
                    }
                });
            }
        }

        // The blur, across the rows, then across the rows of the transposed copy (down).
        let radius = ((a.radius as f64 / factor as f64).round() as usize).max(1);
        let mut line = Vec::new();
        let mut out = Vec::new();
        box_blur_rows(&mut small, sw, c, radius, &mut line, &mut out);
        let mut down = transpose(&small, sw, sh, c);
        box_blur_rows(&mut down, sh, c, radius, &mut line, &mut out);
        let small = transpose(&down, sh, sw, c);

        // Scaled back up: small pixel centres sit at the centres of their blocks. Across, the
        // pixels between two small ones are a run that mixes the same pair.
        let place = |i: usize, n: usize| {
            let at = ((i as f32 + 0.5) / factor as f32 - 0.5).clamp(0.0, (n - 1) as f32);
            let first = at.floor() as usize;
            (first, (first + 1).min(n - 1), at - first as f32)
        };
        let columns: Vec<(usize, usize, f32)> = (0..w).map(|x| place(x, sw)).collect();
        let weights: Vec<f32> = columns.iter().map(|column| column.2).collect();
        let mut runs: Vec<(usize, usize, usize, usize)> = Vec::new();
        for (x, &(left, right, _)) in columns.iter().enumerate() {
            match runs.last_mut() {
                Some(run) if run.2 == left && run.3 == right => run.1 = x + 1,
                _ => runs.push((x, x + 1, left, right)),
            }
        }
        let bands = row_bands(a.y0, h, threads);
        let small = &small;
        let (weights, runs) = (&weights, &runs);
        for_bands(self.data, stride, &bands, threads, |band, rows| {
            let mut mixed = vec![0f32; sw * c];
            for (i, row) in rows.chunks_exact_mut(stride).enumerate() {
                let (top, bottom, t) = place(bands[band].0 + i - a.y0, sh);
                let top = &small[top * sw * c..(top + 1) * sw * c];
                let bottom = &small[bottom * sw * c..(bottom + 1) * sw * c];
                for ((m, t0), b0) in mixed.iter_mut().zip(top).zip(bottom) {
                    *m = t0 + (b0 - t0) * t;
                }
                let pixels = &mut row[a.x0 * c..a.x1 * c];
                for &(start, end, left, right) in runs {
                    let out = &mut pixels[start * c..end * c];
                    let weights = &weights[start..end];
                    if c == 1 {
                        let (l, r) = (mixed[left] + 0.5, mixed[right] + 0.5);
                        for (value, s) in out.iter_mut().zip(weights) {
                            *value = (l + (r - l) * s) as u8;
                        }
                        continue;
                    }
                    for ch in 0..c {
                        let (l, r) = (mixed[left * c + ch] + 0.5, mixed[right * c + ch] + 0.5);
                        for (value, s) in out.iter_mut().skip(ch).step_by(c).zip(weights) {
                            *value = (l + (r - l) * s) as u8;
                        }
                    }
                }
            }
        });
    }
}

/// `values` (`width` pixels of `channels` a row) with rows and columns swapped.
fn transpose(values: &[f32], width: usize, height: usize, channels: usize) -> Vec<f32> {
    let mut out = vec![0f32; values.len()];
    for y in 0..height {
        for x in 0..width {
            let from = (y * width + x) * channels;
            let to = (x * height + y) * channels;
            out[to..to + channels].copy_from_slice(&values[from..from + channels]);
        }
    }
    out
}

/// Three running-sum box blurs of `radius` along each row of `values` (`width` pixels of
/// `channels` each), the row's ends repeated.
fn box_blur_rows(values: &mut [f32], width: usize, channels: usize, radius: usize, line: &mut Vec<f32>, out: &mut Vec<f32>) {
    if width == 0 {
        return;
    }
    for row in values.chunks_exact_mut(width * channels) {
        for ch in 0..channels {
            line.clear();
            line.extend(row.iter().skip(ch).step_by(channels));
            for _ in 0..3 {
                box_blur(line, out, radius);
                std::mem::swap(line, out);
            }
            for (value, blurred) in row.iter_mut().skip(ch).step_by(channels).zip(line.iter()) {
                *value = *blurred;
            }
        }
    }
}

/// A running-sum box blur of `radius` over `input`, its ends repeated.
fn box_blur(input: &[f32], out: &mut Vec<f32>, radius: usize) {
    let n = input.len();
    out.clear();
    if n == 0 {
        return;
    }
    let at = |i: isize| input[i.clamp(0, n as isize - 1) as usize];
    let r = radius as isize;
    let size = (2 * radius + 1) as f32;
    let mut sum: f32 = (-r..=r).map(at).sum();
    for i in 0..n as isize {
        out.push(sum / size);
        sum += at(i + r + 1);
        sum -= at(i - r);
    }
}

fn apply(plane: &mut Plane, a: &Area, blur: bool) {
    if blur {
        plane.blur(a);
    } else {
        plane.pixelate(a);
    }
}

/// Redacts tightly packed RGBA rows.
pub fn redact_rgba(pixels: &mut [u8], width: usize, height: usize, regions: &[Redaction]) {
    if pixels.len() < width * height * 4 {
        return;
    }
    for r in regions {
        if let Some(a) = area(r, width, height, 1) {
            apply(&mut Plane { data: pixels, stride: width * 4, channels: 4, threads: 0 }, &a, r.blur);
        }
    }
}

/// Redacts a YUV 4:2:0 frame (Y at `width` x `height`, then U and V at half each way). The
/// region is widened to even pixels so the chroma cells cover all of it.
pub fn redact_yuv420(pixels: &mut [u8], width: usize, height: usize, regions: &[Redaction]) {
    let luma = width * height;
    let chroma = (width / 2) * (height / 2);
    if pixels.len() < luma + 2 * chroma {
        return;
    }
    for r in regions {
        let Some(a) = area(r, width, height, 2) else { continue };
        let (y_plane, rest) = pixels.split_at_mut(luma);
        apply(&mut Plane { data: y_plane, stride: width, channels: 1, threads: 0 }, &a, r.blur);
        let (u_plane, v_plane) = rest.split_at_mut(chroma);
        let half = a.scaled_down(2);
        apply(&mut Plane { data: u_plane, stride: width / 2, channels: 1, threads: 0 }, &half, r.blur);
        apply(&mut Plane { data: &mut v_plane[..chroma], stride: width / 2, channels: 1, threads: 0 }, &half, r.blur);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-pixel checkerboard, as high-contrast as small text gets.
    fn stripes(width: usize, height: usize, channels: usize) -> Vec<u8> {
        let mut pixels = vec![0u8; width * height * channels];
        for y in 0..height {
            for x in 0..width {
                let v = if (x + y) % 2 == 0 { 255 } else { 0 };
                for c in 0..channels {
                    pixels[(y * width + x) * channels + c] = if c == 3 { 255 } else { v };
                }
            }
        }
        pixels
    }

    fn variance(pixels: &[u8], width: usize, channels: usize, x0: usize, y0: usize, x1: usize, y1: usize) -> f64 {
        let values: Vec<f64> =
            (y0..y1).flat_map(|y| (x0..x1).map(move |x| f64::from(pixels[(y * width + x) * channels]))).collect();
        let mean = values.iter().sum::<f64>() / values.len() as f64;
        values.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / values.len() as f64
    }

    const REGION: Redaction = Redaction { rect: RectF::new(0.25, 0.25, 0.5, 0.5), blur: false, block: 0.05 };

    #[test]
    fn pixelate_flattens_the_region_and_leaves_the_rest() {
        let (w, h) = (200, 100);
        let mut pixels = stripes(w, h, 4);
        let original = pixels.clone();
        assert!(variance(&pixels, w, 4, 50, 25, 150, 75) > 10000.0);
        redact_rgba(&mut pixels, w, h, &[REGION]);
        // Each 5 px cell averages to mid-grey, close to every other cell.
        assert!(variance(&pixels, w, 4, 50, 25, 150, 75) < 300.0, "{}", variance(&pixels, w, 4, 50, 25, 150, 75));
        assert_eq!(&pixels[..w * 4 * 25], &original[..w * 4 * 25]);
        assert_eq!(pixels[(30 * w + 10) * 4], original[(30 * w + 10) * 4]);
        assert!(pixels.chunks_exact(4).all(|p| p[3] == 255));
    }

    #[test]
    fn blur_flattens_the_region() {
        let (w, h) = (200, 100);
        let mut pixels = stripes(w, h, 4);
        redact_rgba(&mut pixels, w, h, &[Redaction { blur: true, ..REGION }]);
        assert!(variance(&pixels, w, 4, 50, 25, 150, 75) < 300.0, "{}", variance(&pixels, w, 4, 50, 25, 150, 75));
    }

    #[test]
    fn yuv_planes_are_all_redacted() {
        let (w, h) = (200, 100);
        let mut pixels = stripes(w, h, 1);
        pixels.extend(stripes(w / 2, h / 2, 1));
        pixels.extend(stripes(w / 2, h / 2, 1));
        redact_yuv420(&mut pixels, w, h, &[REGION]);
        assert!(variance(&pixels, w, 1, 50, 25, 150, 75) < 300.0);
        let u = &pixels[w * h..];
        assert!(variance(u, w / 2, 1, 25, 13, 75, 37) < 300.0);
        let v = &pixels[w * h + (w / 2) * (h / 2)..];
        assert!(variance(v, w / 2, 1, 25, 13, 75, 37) < 300.0);
    }

    /// A line of glyph-like shapes `size` px high with strokes an eighth of that, dark on
    /// light, from (x, y): the size of big text on a 1080p screen.
    fn glyphs(pixels: &mut [u8], width: usize, x: usize, y: usize, size: usize, count: usize) {
        let stroke = size / 8;
        let mut fill = |x0: usize, y0: usize, w: usize, h: usize| {
            for yy in y0..y0 + h {
                for xx in x0..x0 + w {
                    pixels[(yy * width + xx) * 4..(yy * width + xx) * 4 + 3].fill(20);
                }
            }
        };
        for i in 0..count {
            let left = x + i * size * 3 / 4;
            let glyph_width = size / 2;
            // E, H, L, T, O by turns.
            match i % 5 {
                0 => {
                    fill(left, y, stroke, size);
                    for k in [0, (size - stroke) / 2, size - stroke] {
                        fill(left, y + k, glyph_width, stroke);
                    }
                }
                1 => {
                    fill(left, y, stroke, size);
                    fill(left + glyph_width - stroke, y, stroke, size);
                    fill(left, y + (size - stroke) / 2, glyph_width, stroke);
                }
                2 => {
                    fill(left, y, stroke, size);
                    fill(left, y + size - stroke, glyph_width, stroke);
                }
                3 => {
                    fill(left, y, glyph_width, stroke);
                    fill(left + (glyph_width - stroke) / 2, y, stroke, size);
                }
                _ => {
                    fill(left, y, stroke, size);
                    fill(left + glyph_width - stroke, y, stroke, size);
                    fill(left, y, glyph_width, stroke);
                    fill(left, y + size - stroke, glyph_width, stroke);
                }
            }
        }
    }

    /// The mean difference between neighbouring pixels (across and down) in a region: the
    /// edges that make up the strokes of text.
    fn edge_energy(pixels: &[u8], width: usize, x0: usize, y0: usize, x1: usize, y1: usize) -> f64 {
        let at = |x: usize, y: usize| f64::from(pixels[(y * width + x) * 4]);
        let mut sum = 0.0;
        let mut count = 0.0;
        for y in y0..y1 - 1 {
            for x in x0..x1 - 1 {
                sum += (at(x + 1, y) - at(x, y)).abs() + (at(x, y + 1) - at(x, y)).abs();
                count += 2.0;
            }
        }
        sum / count
    }

    /// How many times a row of the region changes value, at most: what is left of the strokes
    /// a reader would follow.
    fn most_changes_in_a_row(pixels: &[u8], width: usize, x0: usize, y0: usize, x1: usize, y1: usize) -> usize {
        (y0..y1)
            .map(|y| (x0 + 1..x1).filter(|&x| pixels[(y * width + x) * 4] != pixels[(y * width + x - 1) * 4]).count())
            .max()
            .unwrap_or(0)
    }

    #[test]
    fn the_weakest_redaction_destroys_big_text() {
        let (w, h) = (1920, 1080);
        for size in [60, 80] {
            for region_height in [0.12, 0.6] {
                let rect = RectF::new(0.1, 0.3, 0.8, region_height);
                let (x0, y0) = ((0.1 * w as f64) as usize, (0.3 * h as f64) as usize);
                let (x1, y1) = ((0.9 * w as f64) as usize, ((0.3 + region_height) * h as f64) as usize);
                let mut page = vec![235u8; w * h * 4];
                glyphs(&mut page, w, x0 + 20, y0 + 20, size, 30);
                let before = edge_energy(&page, w, x0, y0, x1, y1);
                assert!(most_changes_in_a_row(&page, w, x0, y0, x1, y1) >= 60);
                let block = block_fraction(0.0, region_height);
                // Every cell (and the blur's radius) is at least half a glyph high.
                assert!(block * h as f64 >= size as f64 / 2.0, "{size} px text, {} px cells", block * h as f64);

                let mut pixelated = page.clone();
                redact_rgba(&mut pixelated, w, h, &[Redaction { rect, blur: false, block }]);
                // Strokes an eighth of a glyph wide leave nothing of themselves: each row changes
                // only from one cell to the next.
                let cells = ((x1 - x0) as f64 / (block * h as f64)).round() as usize;
                assert!(most_changes_in_a_row(&pixelated, w, x0, y0, x1, y1) < cells, "{size} px text");
                assert!(edge_energy(&pixelated, w, x0, y0, x1, y1) < before * 0.15, "{size} px text");

                let mut blurred = page.clone();
                redact_rgba(&mut blurred, w, h, &[Redaction { rect, blur: true, block }]);
                let after = edge_energy(&blurred, w, x0, y0, x1, y1);
                assert!(after < before * 0.03, "{size} px text: edge energy {before} -> {after}");
            }
        }
    }

    #[test]
    fn cells_grow_with_the_region_and_the_amount() {
        assert_eq!(block_fraction(0.0, 0.1), 0.04);
        assert_eq!(block_fraction(1.0, 0.1), 0.08);
        assert!((block_fraction(0.0, 0.8) - 0.1).abs() < 1e-12);
        assert!((block_fraction(1.0, 0.8) - 0.2).abs() < 1e-12);
        assert!(block_fraction(0.5, 0.4) > block_fraction(0.0, 0.4));
        assert_eq!(block_fraction(f64::NAN, 0.1), 0.06);
    }

    #[test]
    fn the_fast_blur_matches_a_full_size_one() {
        // A region of noise and a bright bar: blurred at full size and through the averaged-down
        // copy, the two agree to within a few levels.
        let (w, h) = (1200, 800);
        let mut noise = vec![0u8; w * h];
        let mut state = 12345u32;
        for v in &mut noise {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            *v = (state >> 24) as u8;
        }
        for y in 300..500 {
            noise[y * w + 100..y * w + 1100].fill(255);
        }
        let a = Area { x0: 0, y0: 0, x1: w, y1: h, cols: 1, rows: 1, radius: 64 };
        let mut fast = noise.clone();
        Plane { data: &mut fast, stride: w, channels: 1, threads: 0 }.blur(&a);
        let mut slow = noise.clone();
        let mut line = Vec::new();
        let mut out = Vec::new();
        let mut values: Vec<f32> = slow.iter().map(|&v| f32::from(v)).collect();
        box_blur_rows(&mut values, w, 1, 64, &mut line, &mut out);
        let mut down = transpose(&values, w, h, 1);
        box_blur_rows(&mut down, h, 1, 64, &mut line, &mut out);
        let values = transpose(&down, h, w, 1);
        for (s, v) in slow.iter_mut().zip(&values) {
            *s = (v + 0.5) as u8;
        }
        let differences: Vec<i32> = fast.iter().zip(&slow).map(|(a, b)| (i32::from(*a) - i32::from(*b)).abs()).collect();
        let worst = *differences.iter().max().unwrap();
        let mean = differences.iter().sum::<i32>() as f64 / differences.len() as f64;
        // A level or two on average; a little more at the region's edges, which repeat a
        // block's average rather than the last pixel.
        assert!(worst <= 16 && mean < 3.0, "worst {worst}, mean {mean}");
    }

    #[test]
    fn threads_give_the_same_result() {
        let (w, h) = (2000, 1200);
        let mut pixels = stripes(w, h, 4);
        for (i, v) in pixels.iter_mut().enumerate() {
            *v = v.wrapping_add((i * 7 % 13) as u8);
        }
        for blur in [false, true] {
            let a = area(&Redaction { rect: RectF::new(0.05, 0.05, 0.9, 0.9), blur, block: 0.03 }, w, h, 1).unwrap();
            let mut threaded = pixels.clone();
            apply(&mut Plane { data: &mut threaded, stride: w * 4, channels: 4, threads: 7 }, &a, blur);
            let mut single = pixels.clone();
            apply(&mut Plane { data: &mut single, stride: w * 4, channels: 4, threads: 1 }, &a, blur);
            assert!(threaded == single, "blur {blur}");
            assert!(threaded != pixels);
        }
    }

    #[test]
    fn region_covers_every_pixel_it_touches() {
        let r = Redaction { rect: RectF::new(0.101, 0.2, 0.3, 0.3), blur: false, block: 0.1 };
        let a = area(&r, 100, 100, 2).unwrap();
        assert_eq!((a.x0, a.y0, a.x1, a.y1), (10, 20, 42, 50));
        assert!(area(&Redaction { rect: RectF::new(1.0, 0.0, 0.1, 0.1), ..r }, 100, 100, 1).is_none());
    }
}

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
//! each way, which approach a Gaussian.

use crate::compat::geometry::RectF;
use crate::project::Overlay;

/// The size of a pixelation cell (and the blur radius) as a fraction of the recording's
/// height, from the overlay's `amount` (0–1): 1 % to 4 %.
pub fn block_fraction(amount: f64) -> f64 {
    0.01 + 0.03 * amount.clamp(0.0, 1.0)
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
        Redaction { rect: overlay.rect, blur: overlay.style == "blur", block: block_fraction(overlay.amount) }
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

/// One plane: `channels` interleaved bytes per pixel, `stride` bytes per row.
struct Plane<'a> {
    data: &'a mut [u8],
    stride: usize,
    channels: usize,
}

impl Plane<'_> {
    fn pixelate(&mut self, a: &Area) {
        let w = a.x1 - a.x0;
        let h = a.y1 - a.y0;
        for row in 0..a.rows {
            let ys = a.y0 + row * h / a.rows;
            let ye = a.y0 + (row + 1) * h / a.rows;
            for col in 0..a.cols {
                let xs = a.x0 + col * w / a.cols;
                let xe = a.x0 + (col + 1) * w / a.cols;
                if xe <= xs || ye <= ys {
                    continue;
                }
                let mut sums = [0u64; 4];
                for y in ys..ye {
                    let line = &self.data[y * self.stride + xs * self.channels..y * self.stride + xe * self.channels];
                    for pixel in line.chunks_exact(self.channels) {
                        for (sum, value) in sums.iter_mut().zip(pixel) {
                            *sum += u64::from(*value);
                        }
                    }
                }
                let count = ((xe - xs) * (ye - ys)) as u64;
                let mean: Vec<u8> = sums[..self.channels].iter().map(|sum| ((sum + count / 2) / count) as u8).collect();
                for y in ys..ye {
                    let line = &mut self.data[y * self.stride + xs * self.channels..y * self.stride + xe * self.channels];
                    for pixel in line.chunks_exact_mut(self.channels) {
                        pixel.copy_from_slice(&mean);
                    }
                }
            }
        }
    }

    /// Three box blurs of `radius` across then down, inside the area (its edges repeat
    /// outwards; nothing from outside comes in).
    fn blur(&mut self, a: &Area) {
        let c = self.channels;
        let mut line: Vec<u32> = Vec::new();
        let mut out: Vec<u32> = Vec::new();
        for _ in 0..3 {
            for y in a.y0..a.y1 {
                for ch in 0..c {
                    line.clear();
                    line.extend((a.x0..a.x1).map(|x| u32::from(self.data[y * self.stride + x * c + ch])));
                    box_blur(&line, &mut out, a.radius);
                    for (i, v) in out.iter().enumerate() {
                        self.data[y * self.stride + (a.x0 + i) * c + ch] = *v as u8;
                    }
                }
            }
            for x in a.x0..a.x1 {
                for ch in 0..c {
                    line.clear();
                    line.extend((a.y0..a.y1).map(|y| u32::from(self.data[y * self.stride + x * c + ch])));
                    box_blur(&line, &mut out, a.radius);
                    for (i, v) in out.iter().enumerate() {
                        self.data[(a.y0 + i) * self.stride + x * c + ch] = *v as u8;
                    }
                }
            }
        }
    }
}

/// A running-sum box blur of `radius` over `input`, its ends repeated.
fn box_blur(input: &[u32], out: &mut Vec<u32>, radius: usize) {
    let n = input.len();
    out.clear();
    if n == 0 {
        return;
    }
    let at = |i: isize| input[i.clamp(0, n as isize - 1) as usize];
    let r = radius as isize;
    let size = 2 * radius as u32 + 1;
    let mut sum: u32 = (-r..=r).map(at).sum();
    for i in 0..n as isize {
        out.push((sum + size / 2) / size);
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
            apply(&mut Plane { data: pixels, stride: width * 4, channels: 4 }, &a, r.blur);
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
        apply(&mut Plane { data: y_plane, stride: width, channels: 1 }, &a, r.blur);
        let (u_plane, v_plane) = rest.split_at_mut(chroma);
        let half = a.scaled_down(2);
        apply(&mut Plane { data: u_plane, stride: width / 2, channels: 1 }, &half, r.blur);
        apply(&mut Plane { data: &mut v_plane[..chroma], stride: width / 2, channels: 1 }, &half, r.blur);
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

    #[test]
    fn region_covers_every_pixel_it_touches() {
        let r = Redaction { rect: RectF::new(0.101, 0.2, 0.3, 0.3), blur: false, block: 0.1 };
        let a = area(&r, 100, 100, 2).unwrap();
        assert_eq!((a.x0, a.y0, a.x1, a.y1), (10, 20, 42, 50));
        assert!(area(&Redaction { rect: RectF::new(1.0, 0.0, 0.1, 0.1), ..r }, 100, 100, 1).is_none());
    }
}

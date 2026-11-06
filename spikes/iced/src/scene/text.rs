//! Labels for the keystroke pills: laid out and rasterised by cosmic-text (the text engine iced
//! itself uses, through `iced::advanced::graphics`), at the pixel size they are drawn at, into
//! one single-channel atlas the scene samples.

use iced::advanced::graphics::text::cosmic_text as ct;
use iced::advanced::graphics::text::font_system;
use std::collections::HashMap;

pub const ATLAS_SIZE: u32 = 1024;

/// A rasterised label: where it is in the atlas, and its box in pixels (advance width, and the
/// line height, which is what a QML Label's implicit size is).
#[derive(Clone, Copy, Debug)]
pub struct Label {
    pub atlas: [u32; 4],
    pub width: f32,
    pub height: f32,
}

pub struct TextAtlas {
    pub pixels: Vec<u8>,
    pub dirty: bool,
    family: String,
    swash: ct::SwashCache,
    labels: HashMap<(String, u32, u16), Label>,
    cursor: (u32, u32),
    row_height: u32,
}

impl TextAtlas {
    pub fn new(family: &str) -> TextAtlas {
        TextAtlas {
            pixels: vec![0; (ATLAS_SIZE * ATLAS_SIZE) as usize],
            dirty: true,
            family: family.to_owned(),
            swash: ct::SwashCache::new(),
            labels: HashMap::new(),
            cursor: (0, 0),
            row_height: 0,
        }
    }

    pub fn set_family(&mut self, family: &str) {
        if self.family != family {
            self.family = family.to_owned();
            self.clear();
        }
    }

    fn clear(&mut self) {
        self.pixels.fill(0);
        self.labels.clear();
        self.cursor = (0, 0);
        self.row_height = 0;
        self.dirty = true;
    }

    /// The label for `text` at `pixel_size` and `weight`, rasterised on first use.
    pub fn label(&mut self, text: &str, pixel_size: f32, weight: u16) -> Label {
        let key = (text.to_owned(), (pixel_size * 64.0).round() as u32, weight);
        if let Some(label) = self.labels.get(&key) {
            return *label;
        }
        let (width, height, bitmap, bitmap_width, bitmap_height) = self.rasterise(text, pixel_size, weight);
        if self.cursor.0 + bitmap_width > ATLAS_SIZE {
            self.cursor = (0, self.cursor.1 + self.row_height + 1);
            self.row_height = 0;
        }
        if self.cursor.1 + bitmap_height > ATLAS_SIZE {
            // Full: start over. Labels change rarely, so this is almost never hit.
            self.clear();
        }
        let (x, y) = self.cursor;
        for row in 0..bitmap_height.min(ATLAS_SIZE - y) {
            let source = &bitmap[(row * bitmap_width) as usize..][..bitmap_width.min(ATLAS_SIZE - x) as usize];
            let start = ((y + row) * ATLAS_SIZE + x) as usize;
            self.pixels[start..start + source.len()].copy_from_slice(source);
        }
        self.cursor.0 += bitmap_width + 1;
        self.row_height = self.row_height.max(bitmap_height);
        self.dirty = true;
        let label = Label { atlas: [x, y, bitmap_width, bitmap_height], width, height };
        self.labels.insert(key, label);
        label
    }

    fn rasterise(&mut self, text: &str, pixel_size: f32, weight: u16) -> (f32, f32, Vec<u8>, u32, u32) {
        let mut fonts = font_system().write().expect("font system");
        let fonts = fonts.raw();
        let line_height = (pixel_size * 1.32).ceil();
        let mut buffer = ct::Buffer::new(fonts, ct::Metrics::new(pixel_size, line_height));
        buffer.set_size(fonts, None, None);
        let attrs = ct::Attrs::new().family(ct::Family::Name(&self.family)).weight(ct::Weight(weight));
        buffer.set_text(fonts, text, &attrs, ct::Shaping::Advanced, None);
        buffer.shape_until_scroll(fonts, false);
        let mut width: f32 = 0.0;
        let mut glyphs = Vec::new();
        for run in buffer.layout_runs() {
            width = width.max(run.line_w);
            for glyph in run.glyphs {
                let physical = glyph.physical((0.0, run.line_y), 1.0);
                glyphs.push(physical);
            }
        }
        let bitmap_width = (width.ceil() as u32 + 2).max(1);
        let bitmap_height = line_height as u32;
        let mut bitmap = vec![0u8; (bitmap_width * bitmap_height) as usize];
        for glyph in glyphs {
            let Some(image) = self.swash.get_image_uncached(fonts, glyph.cache_key) else { continue };
            let placement = image.placement;
            let channels = match image.content {
                ct::SwashContent::Mask => 1,
                ct::SwashContent::SubpixelMask | ct::SwashContent::Color => 4,
            };
            for gy in 0..placement.height as i32 {
                for gx in 0..placement.width as i32 {
                    let x = glyph.x + placement.left + gx + 1;
                    let y = glyph.y - placement.top + gy;
                    if x < 0 || y < 0 || x >= bitmap_width as i32 || y >= bitmap_height as i32 {
                        continue;
                    }
                    let index = (gy * placement.width as i32 + gx) as usize * channels + channels - 1;
                    let target = &mut bitmap[(y as u32 * bitmap_width + x as u32) as usize];
                    *target = (*target).max(image.data[index]);
                }
            }
        }
        (width, line_height, bitmap, bitmap_width, bitmap_height)
    }
}

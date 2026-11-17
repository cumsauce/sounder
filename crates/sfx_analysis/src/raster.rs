//! Tiny CPU raster helpers: an RGBA canvas with lines, filled polygons, 5x7
//! bitmap text and PNG encoding, plus a colour map for spectrograms. Copied
//! from modeler's `src/raster.rs` (the silhouette functions dropped).

/// An RGBA8 image, row-major, top row first.
#[derive(Clone)]
pub struct Canvas {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

impl Canvas {
    pub fn new(w: usize, h: usize, bg: [u8; 4]) -> Self {
        let mut px = vec![0u8; w * h * 4];
        for c in px.as_chunks_mut::<4>().0 {
            *c = bg;
        }
        Self { w, h, px }
    }

    pub fn from_rgba(w: usize, h: usize, px: Vec<u8>) -> Self {
        Self { w, h, px }
    }

    pub fn set(&mut self, x: i64, y: i64, c: [u8; 4]) {
        if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h {
            return;
        }
        let i = (y as usize * self.w + x as usize) * 4;
        self.px[i..i + 4].copy_from_slice(&c);
    }

    /// A 1 px line (`thick` > 1 draws a square brush).
    pub fn line(&mut self, a: (f64, f64), b: (f64, f64), c: [u8; 4], thick: i64) {
        let steps = (b.0 - a.0).abs().max((b.1 - a.1).abs()).ceil().max(1.0) as i64;
        let r = thick / 2;
        for s in 0..=steps {
            let t = s as f64 / steps as f64;
            let (x, y) = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            for dy in -r..=r {
                for dx in -r..=r {
                    self.set(x.round() as i64 + dx, y.round() as i64 + dy, c);
                }
            }
        }
    }

    /// Fills the even-odd union of `rings` (pixel coordinates).
    pub fn fill_rings(&mut self, rings: &[Vec<(f64, f64)>], c: [u8; 4]) {
        for y in 0..self.h {
            let sy = y as f64 + 0.5;
            let mut xs: Vec<f64> = Vec::new();
            for r in rings {
                for k in 0..r.len() {
                    let (p, q) = (r[k], r[(k + 1) % r.len()]);
                    if (p.1 <= sy) != (q.1 <= sy) {
                        xs.push(p.0 + (sy - p.1) / (q.1 - p.1) * (q.0 - p.0));
                    }
                }
            }
            xs.sort_by(f64::total_cmp);
            for pair in xs.chunks(2) {
                if let [x0, x1] = pair {
                    let (a, b) = (x0.round().max(0.0) as usize, x1.round().max(0.0) as usize);
                    for x in a..b.min(self.w) {
                        self.set(x as i64, y as i64, c);
                    }
                }
            }
        }
    }

    /// White 5x7 text on a darkened box at (x0, y0), `scale` px per dot.
    pub fn text(&mut self, x0: usize, y0: usize, text: &str, scale: usize) {
        let (w, h) = (self.w, self.h);
        let (bw, bh) = ((text.chars().count() * 6 + 2) * scale, 9 * scale);
        for y in y0..(y0 + bh).min(h) {
            for x in x0..(x0 + bw).min(w) {
                let i = (y * w + x) * 4;
                self.px[i..i + 3].iter_mut().for_each(|c| *c /= 4);
                self.px[i + 3] = 255;
            }
        }
        for (k, ch) in text.chars().enumerate() {
            let g = glyph(ch);
            for (row, bits) in g.iter().enumerate() {
                for col in 0..5 {
                    if bits & (16 >> col) == 0 {
                        continue;
                    }
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let x = x0 + (1 + k * 6 + col) * scale + dx;
                            let y = y0 + (1 + row) * scale + dy;
                            self.set(x as i64, y as i64, [255; 4]);
                        }
                    }
                }
            }
        }
    }

    /// Pixel width of `text` at `scale`, matching [`Self::text`].
    pub fn text_width(text: &str, scale: usize) -> usize {
        (text.chars().count() * 6 + 2) * scale
    }

    pub fn to_png(&self) -> Result<Vec<u8>, String> {
        encode_png(self.w as u32, self.h as u32, &self.px)
    }
}

pub fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, w, h);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| format!("png header: {e}"))?;
        writer
            .write_image_data(rgba)
            .map_err(|e| format!("png body: {e}"))?;
    }
    Ok(out)
}

/// 5x7 bitmap glyphs (rows top to bottom, 5 low bits); lowercase draws as
/// uppercase, unknown characters as blanks.
fn glyph(c: char) -> [u8; 7] {
    match c.to_ascii_uppercase() {
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 15],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [14, 4, 4, 4, 4, 4, 14],
        'J' => [7, 2, 2, 2, 2, 18, 12],
        'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'N' => [17, 17, 25, 21, 19, 17, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 21, 10],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 1, 2, 4, 8, 16, 31],
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [31, 2, 4, 2, 1, 17, 14],
        '4' => [2, 6, 10, 18, 31, 2, 2],
        '5' => [31, 16, 30, 1, 1, 17, 14],
        '6' => [6, 8, 16, 30, 17, 17, 14],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 2, 12],
        '-' => [0, 0, 0, 31, 0, 0, 0],
        '+' => [0, 4, 4, 31, 4, 4, 0],
        '=' => [0, 0, 31, 0, 31, 0, 0],
        '.' => [0, 0, 0, 0, 0, 12, 12],
        ',' => [0, 0, 0, 0, 12, 4, 8],
        ':' => [0, 12, 12, 0, 12, 12, 0],
        '%' => [24, 25, 2, 4, 8, 19, 3],
        '/' => [0, 1, 2, 4, 8, 16, 0],
        '(' => [2, 4, 8, 8, 8, 4, 2],
        ')' => [8, 4, 2, 2, 2, 4, 8],
        '_' => [0, 0, 0, 0, 0, 0, 31],
        '<' => [2, 4, 8, 16, 8, 4, 2],
        '>' => [8, 4, 2, 1, 2, 4, 8],
        '*' => [0, 4, 21, 14, 21, 4, 0],
        _ => [0; 7],
    }
}

/// The spectrogram colour map ("inferno"-like: black, purple, red, orange,
/// pale yellow) at `t` in 0..1.
pub fn colormap(t: f32) -> [u8; 4] {
    const STOPS: [[f32; 3]; 7] = [
        [0.0, 0.0, 0.016],
        [0.122, 0.047, 0.282],
        [0.333, 0.059, 0.427],
        [0.533, 0.133, 0.416],
        [0.729, 0.212, 0.333],
        [0.976, 0.557, 0.035],
        [0.988, 1.0, 0.643],
    ];
    let t = t.clamp(0.0, 1.0) * (STOPS.len() - 1) as f32;
    let k = (t.floor() as usize).min(STOPS.len() - 2);
    let f = t - k as f32;
    let c = |j: usize| (STOPS[k][j] + (STOPS[k + 1][j] - STOPS[k][j]) * f) * 255.0;
    [c(0) as u8, c(1) as u8, c(2) as u8, 255]
}

/// A colour for series `i` (waveform, loudness, labels), readable on the
/// dark background.
pub fn series(i: usize) -> [u8; 4] {
    const C: [[u8; 4]; 4] = [
        [120, 200, 255, 255],
        [255, 190, 90, 255],
        [140, 230, 140, 255],
        [240, 120, 200, 255],
    ];
    C[i % C.len()]
}

impl Canvas {
    /// Fills a rectangle.
    pub fn rect(&mut self, x0: usize, y0: usize, w: usize, h: usize, c: [u8; 4]) {
        for y in y0..(y0 + h).min(self.h) {
            for x in x0..(x0 + w).min(self.w) {
                let i = (y * self.w + x) * 4;
                self.px[i..i + 4].copy_from_slice(&c);
            }
        }
    }

    /// Copies `src` in at (x0, y0).
    pub fn blit(&mut self, src: &Canvas, x0: usize, y0: usize) {
        for y in 0..src.h {
            for x in 0..src.w {
                let i = (y * src.w + x) * 4;
                let c = [src.px[i], src.px[i + 1], src.px[i + 2], src.px[i + 3]];
                self.set((x0 + x) as i64, (y0 + y) as i64, c);
            }
        }
    }
}

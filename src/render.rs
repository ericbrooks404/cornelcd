//! An 80x160 RGB565 framebuffer with just enough drawing to compose a screen.
//!
//! Pixels go out big-endian — the panel reads the high byte first. Getting this
//! backwards renders recognisable shapes in wrong colours, which is a confusing
//! way to fail, so it is centralised here.

use crate::proto::{FB_LEN, SCREEN_H, SCREEN_W};

pub const CLAUDE_ORANGE: u32 = 0xD9_77_57;
pub const INK: u32 = 0xE8_E6_E3;
pub const DIM: u32 = 0x6B_6B_6B;
pub const BG: u32 = 0x0A_0A_0A;
pub const EYE: u32 = 0x2A_1F_1B;

pub struct Framebuffer {
    pub data: Vec<u8>,
}

impl Default for Framebuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl Framebuffer {
    pub fn new() -> Framebuffer {
        Framebuffer {
            data: vec![0; FB_LEN],
        }
    }

    pub fn fill(&mut self, rgb: u32) {
        let px = rgb565(rgb);
        for o in (0..FB_LEN).step_by(2) {
            self.data[o] = (px >> 8) as u8;
            self.data[o + 1] = (px & 0xFF) as u8;
        }
    }

    pub fn set(&mut self, x: i32, y: i32, rgb: u32) {
        if x < 0 || y < 0 || x >= SCREEN_W as i32 || y >= SCREEN_H as i32 {
            return;
        }
        let px = rgb565(rgb);
        let o = (y as usize * SCREEN_W + x as usize) * 2;
        self.data[o] = (px >> 8) as u8;
        self.data[o + 1] = (px & 0xFF) as u8;
    }

    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, rgb: u32) {
        for dy in 0..h {
            for dx in 0..w {
                self.set(x + dx, y + dy, rgb);
            }
        }
    }

    /// Horizontal progress bar with a one-pixel border.
    #[allow(clippy::too_many_arguments)] // geometry + two colours; a struct would obscure more than it clarifies
    pub fn bar(&mut self, x: i32, y: i32, w: i32, h: i32, frac: f32, fg: u32, track: u32) {
        self.rect(x, y, w, h, track);
        let fill = ((w - 2) as f32 * frac.clamp(0.0, 1.0)).round() as i32;
        if fill > 0 {
            self.rect(x + 1, y + 1, fill, h - 2, fg);
        }
    }

    /// Draw text in the 5x7 font. `scale` multiplies pixel size.
    pub fn text(&mut self, x: i32, y: i32, s: &str, rgb: u32, scale: i32) {
        let mut cx = x;
        for ch in s.chars() {
            let glyph = font5x7(ch);
            for (col, bits) in glyph.iter().enumerate() {
                for row in 0..7 {
                    if bits & (1 << row) != 0 {
                        if scale == 1 {
                            self.set(cx + col as i32, y + row, rgb);
                        } else {
                            self.rect(
                                cx + col as i32 * scale,
                                y + row * scale,
                                scale,
                                scale,
                                rgb,
                            );
                        }
                    }
                }
            }
            cx += 6 * scale;
        }
    }

    pub fn text_width(s: &str, scale: i32) -> i32 {
        s.chars().count() as i32 * 6 * scale
    }

    pub fn text_centered(&mut self, y: i32, s: &str, rgb: u32, scale: i32) {
        let w = Self::text_width(s, scale);
        self.text((SCREEN_W as i32 - w) / 2, y, s, rgb, scale);
    }

    /// Chunk indices whose 25-byte window differs from `other`.
    /// Used to push only what changed between animation frames.
    pub fn diff_chunks(&self, other: &Framebuffer) -> Vec<u16> {
        const CHUNK: usize = 25;
        let mut out = Vec::new();
        for (i, (a, b)) in self
            .data
            .chunks(CHUNK)
            .zip(other.data.chunks(CHUNK))
            .enumerate()
        {
            if a != b {
                out.push(i as u16);
            }
        }
        out
    }
}

pub fn rgb565(rgb: u32) -> u16 {
    let r = ((rgb >> 16) & 0xFF) as u16;
    let g = ((rgb >> 8) & 0xFF) as u16;
    let b = (rgb & 0xFF) as u16;
    ((r & 0xF8) << 8) | ((g & 0xFC) << 3) | (b >> 3)
}

/// Blend `fg` over `bg` at `alpha` (0.0-1.0).
pub fn blend(bg: u32, fg: u32, alpha: f32) -> u32 {
    let a = alpha.clamp(0.0, 1.0);
    let mix = |sh: u32| {
        let x = ((bg >> sh) & 0xFF) as f32;
        let y = ((fg >> sh) & 0xFF) as f32;
        ((x + (y - x) * a).round() as u32).min(255) << sh
    };
    mix(16) | mix(8) | mix(0)
}

/// 5x7 bitmap font, one byte per column, bit N = row N.
/// Covers digits, uppercase, and the handful of symbols the screens need.
fn font5x7(ch: char) -> [u8; 5] {
    match ch.to_ascii_uppercase() {
        '0' => [0x3E, 0x51, 0x49, 0x45, 0x3E],
        '1' => [0x00, 0x42, 0x7F, 0x40, 0x00],
        '2' => [0x42, 0x61, 0x51, 0x49, 0x46],
        '3' => [0x21, 0x41, 0x45, 0x4B, 0x31],
        '4' => [0x18, 0x14, 0x12, 0x7F, 0x10],
        '5' => [0x27, 0x45, 0x45, 0x45, 0x39],
        '6' => [0x3C, 0x4A, 0x49, 0x49, 0x30],
        '7' => [0x01, 0x71, 0x09, 0x05, 0x03],
        '8' => [0x36, 0x49, 0x49, 0x49, 0x36],
        '9' => [0x06, 0x49, 0x49, 0x29, 0x1E],
        'A' => [0x7E, 0x11, 0x11, 0x11, 0x7E],
        'B' => [0x7F, 0x49, 0x49, 0x49, 0x36],
        'C' => [0x3E, 0x41, 0x41, 0x41, 0x22],
        'D' => [0x7F, 0x41, 0x41, 0x22, 0x1C],
        'E' => [0x7F, 0x49, 0x49, 0x49, 0x41],
        'F' => [0x7F, 0x09, 0x09, 0x09, 0x01],
        'G' => [0x3E, 0x41, 0x49, 0x49, 0x7A],
        'H' => [0x7F, 0x08, 0x08, 0x08, 0x7F],
        'I' => [0x00, 0x41, 0x7F, 0x41, 0x00],
        'J' => [0x20, 0x40, 0x41, 0x3F, 0x01],
        'K' => [0x7F, 0x08, 0x14, 0x22, 0x41],
        'L' => [0x7F, 0x40, 0x40, 0x40, 0x40],
        'M' => [0x7F, 0x02, 0x0C, 0x02, 0x7F],
        'N' => [0x7F, 0x04, 0x08, 0x10, 0x7F],
        'O' => [0x3E, 0x41, 0x41, 0x41, 0x3E],
        'P' => [0x7F, 0x09, 0x09, 0x09, 0x06],
        'Q' => [0x3E, 0x41, 0x51, 0x21, 0x5E],
        'R' => [0x7F, 0x09, 0x19, 0x29, 0x46],
        'S' => [0x46, 0x49, 0x49, 0x49, 0x31],
        'T' => [0x01, 0x01, 0x7F, 0x01, 0x01],
        'U' => [0x3F, 0x40, 0x40, 0x40, 0x3F],
        'V' => [0x1F, 0x20, 0x40, 0x20, 0x1F],
        'W' => [0x7F, 0x20, 0x18, 0x20, 0x7F],
        'X' => [0x63, 0x14, 0x08, 0x14, 0x63],
        'Y' => [0x03, 0x04, 0x78, 0x04, 0x03],
        'Z' => [0x61, 0x51, 0x49, 0x45, 0x43],
        '.' => [0x00, 0x60, 0x60, 0x00, 0x00],
        ',' => [0x00, 0x80, 0x60, 0x00, 0x00],
        ':' => [0x00, 0x36, 0x36, 0x00, 0x00],
        '/' => [0x20, 0x10, 0x08, 0x04, 0x02],
        '%' => [0x23, 0x13, 0x08, 0x64, 0x62],
        '-' => [0x08, 0x08, 0x08, 0x08, 0x08],
        '+' => [0x08, 0x08, 0x3E, 0x08, 0x08],
        '(' => [0x00, 0x1C, 0x22, 0x41, 0x00],
        ')' => [0x00, 0x41, 0x22, 0x1C, 0x00],
        ' ' => [0x00, 0x00, 0x00, 0x00, 0x00],
        _ => [0x7F, 0x41, 0x41, 0x41, 0x7F], // box for anything unmapped
    }
}

//! Clawd, the Claude Code mascot.
//!
//! Sprite data is the genuine article, lifted from the Claude Code binary
//! (`CLAWD_FRAMES` / `CLAWD_PAL`). Two frames of Clawd waving a paintbrush,
//! 16x14 pixels each. At 5x scale that is exactly 80px wide — the panel width.

use crate::render::{blend, Framebuffer, CLAUDE_ORANGE, EYE};

pub const W: i32 = 16;
#[allow(dead_code)]
pub const H: i32 = 14;

/// Frame 0: brush held out to the right. Frame 1: brush raised, mid-stroke.
pub const FRAMES: [[&str; 14]; 2] = [
    [
        "................",
        "................",
        "................",
        "..OOOOOOOO......",
        "..OOOOOOOO......",
        "..OODOODOO......",
        "..OODOODOO....B.",
        "..OOOOOOOOOHHFB.",
        "..OOOOOOOO....B.",
        "..OOOOOOOO......",
        "...OO..OO......b",
        "...OO..OO.......",
        "................",
        "................",
    ],
    [
        "..............b.",
        "..............BB",
        ".............B..",
        "..OOOOOOOO..F...",
        "..OOOOOOOO.H....",
        "..OODOODOOH.....",
        "..OODOODOOO.....",
        "..OOOOOOOO......",
        "..OOOOOOOO......",
        "..OOOOOOOO......",
        "...OO..OO.......",
        "...OO..OO.......",
        "................",
        "................",
    ],
];

const HANDLE: u32 = 0x8B_5E_34; // brush handle, wood
const FERRULE: u32 = 0x7D_84_8A; // metal band

/// Resolve a sprite character to a colour, given the background it sits on.
/// `b` is the drawn stroke — the accent at 55% alpha, lighter than the brush.
fn pal(ch: char, bg: u32) -> Option<u32> {
    Some(match ch {
        'O' => CLAUDE_ORANGE,
        'D' => EYE,
        'H' => HANDLE,
        'F' => FERRULE,
        'B' => CLAUDE_ORANGE,
        'b' => blend(bg, CLAUDE_ORANGE, 0.55),
        _ => return None,
    })
}

/// Blit one frame at `scale`, top-left anchored at (x, y).
pub fn draw(fb: &mut Framebuffer, frame: usize, x: i32, y: i32, scale: i32, bg: u32) {
    let grid = &FRAMES[frame % FRAMES.len()];
    for (row, line) in grid.iter().enumerate() {
        for (col, ch) in line.chars().enumerate() {
            let Some(rgb) = pal(ch, bg) else { continue };
            fb.rect(
                x + col as i32 * scale,
                y + row as i32 * scale,
                scale,
                scale,
                rgb,
            );
        }
    }
}

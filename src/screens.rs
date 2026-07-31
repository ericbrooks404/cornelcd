//! Screen compositions rendered host-side and pushed as raw framebuffers.

use crate::clawd;
use crate::render::{Framebuffer, BG, CLAUDE_ORANGE, DIM, INK};
use crate::usage::{short, Totals};

/// Left half: Claude Code token usage.
///
/// Weekly bars are scaled against a soft reference rather than a real quota —
/// Claude Code stores no limit information locally, so a "% of limit" reading
/// would be invented. The number is the truth here; the bar is only a trend.
pub fn usage_screen(t: &Totals, week_ref: u64) -> Framebuffer {
    let mut fb = Framebuffer::new();
    fb.fill(BG);

    // Header
    fb.rect(0, 0, 80, 14, CLAUDE_ORANGE);
    fb.text_centered(4, "CLAUDE", BG, 1);

    // Session block
    fb.text(4, 22, "SESSION", DIM, 1);
    fb.text(4, 34, &short(t.session.billable()), INK, 2);
    fb.text(4, 52, "TOKENS", DIM, 1);

    // divider
    fb.rect(4, 66, 72, 1, 0x2A_2A_2A);

    // Week block
    fb.text(4, 74, "7 DAYS", DIM, 1);
    fb.text(4, 86, &short(t.week.billable()), INK, 2);

    let frac = if week_ref > 0 {
        t.week.billable() as f32 / week_ref as f32
    } else {
        0.0
    };
    fb.bar(4, 106, 72, 9, frac, CLAUDE_ORANGE, 0x2A_2A_2A);

    // Breakdown
    fb.text(4, 122, "IN", DIM, 1);
    fb.text(28, 122, &short(t.week.input + t.week.cache_write), INK, 1);
    fb.text(4, 134, "OUT", DIM, 1);
    fb.text(28, 134, &short(t.week.output), INK, 1);
    fb.text(4, 146, "CACHE", DIM, 1);
    fb.text(40, 146, &short(t.week.cache_read), DIM, 1);

    fb
}

/// Right half: Clawd, waving his paintbrush.
///
/// 16x14 at 4x is 64x56, centred, with room for a caption underneath.
pub fn clawd_screen(frame: usize, scale: i32) -> Framebuffer {
    let mut fb = Framebuffer::new();
    fb.fill(BG);

    let x = (80 - clawd::W * scale) / 2;
    // Same ground line the jump uses, so idle and animation line up exactly.
    let y = crate::anim::ground(scale);

    // A gentle bob so the whole sprite moves, not just the brush.
    let bob = if frame % 2 == 0 { 0 } else { 2 };

    clawd::draw(&mut fb, frame, x, y + bob, scale, BG);

    fb.text_centered(16, "CLAWD", CLAUDE_ORANGE, 1);

    fb
}

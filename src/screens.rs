//! Screen compositions rendered host-side and pushed as raw framebuffers.

use crate::clawd;
use crate::render::{Framebuffer, BG, CLAUDE_ORANGE, DIM, INK};
use crate::usage::{short, Totals};

/// Left half: Claude Code token usage.
///
/// Weekly bars are scaled against a soft reference rather than a real quota —
/// Claude Code stores no limit information locally, so a "% of limit" reading
/// would be invented. The number is the truth here; the bar is only a trend.
/// Where Clawd is on the panel this frame, if he's visible at all.
pub struct ClawdState {
    pub x: i32,
    pub frame: usize,
    pub legs: crate::clawd::Legs,
    pub mirror: bool,
    pub lift: i32,
    pub scale: i32,
}

/// The whole Claude panel: usage above, Clawd patrolling the bottom strip.
///
/// Both live on the master half. The slave panel cannot take host-pushed
/// pixels — see the guard in `proto::push_image` — so it keeps the firmware's
/// own WPM/layer screen.
pub fn usage_screen(t: &Totals, week_ref: u64, clawd_at: Option<&ClawdState>) -> Framebuffer {
    let mut fb = Framebuffer::new();
    fb.fill(BG);

    // Header
    fb.rect(0, 0, 80, 13, CLAUDE_ORANGE);
    fb.text_centered(3, "CLAUDE", BG, 1);

    // Session
    fb.text(4, 18, "SESSION", DIM, 1);
    fb.text(4, 29, &short(t.session.billable()), INK, 2);

    // Week
    fb.text(4, 50, "7 DAYS", DIM, 1);
    fb.text(4, 61, &short(t.week.billable()), INK, 2);

    let frac = if week_ref > 0 {
        t.week.billable() as f32 / week_ref as f32
    } else {
        0.0
    };
    fb.bar(4, 81, 72, 8, frac, CLAUDE_ORANGE, 0x2A_2A_2A);

    // Breakdown
    fb.text(4, 93, "IN", DIM, 1);
    fb.text(28, 93, &short(t.week.input + t.week.cache_write), INK, 1);
    fb.text(4, 103, "OUT", DIM, 1);
    fb.text(28, 103, &short(t.week.output), INK, 1);

    // Ground line for Clawd's strip.
    fb.rect(0, 156, 80, 1, 0x2A_2A_2A);

    if let Some(c) = clawd_at {
        let grid = crate::clawd::pose(c.frame, c.legs, c.mirror);
        let y = 156 - crate::clawd::H * c.scale - c.lift;
        crate::clawd::draw_pose(&mut fb, &grid, c.x, y, c.scale, BG);
    }

    fb
}

/// Right half: Clawd, waving his paintbrush.
///
/// 16x14 at 4x is 64x56, centred, with room for a caption underneath.
#[allow(dead_code)]
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

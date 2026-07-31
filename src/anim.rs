//! Clawd's running jump from one screen to the other.
//!
//! The two panels are independent framebuffers, so the illusion is entirely a
//! matter of timing: he exits one edge and enters the facing edge of the other
//! at the same moment, on the same ballistic arc.
//!
//! Cost is dominated by how much of the panel the sprite disturbs each step, so
//! every step targets exactly one half and pushes only changed chunks.

use crate::clawd::{self, Legs};
use crate::proto::{Half, SCREEN_H, SCREEN_W};
use crate::render::{Framebuffer, BG};

/// Which way Clawd travels. `LeftToRight` means he leaves the master panel by
/// its right edge and arrives at the slave panel's left edge.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Dir {
    LeftToRight,
    RightToLeft,
}

impl Dir {
    #[allow(dead_code)]
    pub fn flip(self) -> Dir {
        match self {
            Dir::LeftToRight => Dir::RightToLeft,
            Dir::RightToLeft => Dir::LeftToRight,
        }
    }

    /// Half he departs from, then the half he lands on.
    fn halves(self) -> (Half, Half) {
        match self {
            Dir::LeftToRight => (Half::Master, Half::Slave),
            Dir::RightToLeft => (Half::Slave, Half::Master),
        }
    }

    fn sign(self) -> i32 {
        match self {
            Dir::LeftToRight => 1,
            Dir::RightToLeft => -1,
        }
    }
}

/// One rendered step of the sequence: which panel to push, and what it shows.
pub struct Step {
    pub half: Half,
    pub fb: Framebuffer,
}

/// Ground line for the sprite's top edge, so he stands in the lower third.
pub fn ground(scale: i32) -> i32 {
    SCREEN_H as i32 - clawd::H * scale - 18
}

/// Build the whole sequence. `scale` is the sprite multiplier, `flip_landing`
/// mirrors x on the destination panel for a physically reversed display.
pub fn jump_sequence(dir: Dir, scale: i32, flip_landing: bool) -> Vec<Step> {
    let (from, to) = dir.halves();
    let sprite_w = clawd::W * scale;
    let g = ground(scale);
    let sign = dir.sign();
    let mut steps = Vec::new();

    // Run-up: from the middle of the departure panel out to the edge.
    let start_x = (SCREEN_W as i32 - sprite_w) / 2;
    let exit_x = if sign > 0 {
        SCREEN_W as i32
    } else {
        -sprite_w
    };

    let stride = 7 * scale / 2;
    let mut x = start_x;
    let mut cycle = 0;

    while (sign > 0 && x < exit_x) || (sign < 0 && x > exit_x) {
        let legs = if cycle % 2 == 0 { Legs::RunA } else { Legs::RunB };
        steps.push(frame(from, dir, x, g, scale, legs, flip_landing, false));
        x += stride * sign;
        cycle += 1;
    }

    // Airborne: a short arc, half spent leaving, half arriving. Height is a
    // simple parabola so the apex sits at the panel boundary.
    let arc: [i32; 6] = [6, 11, 14, 14, 11, 6];
    let air_stride = 5 * scale;

    for (i, lift) in arc.iter().enumerate() {
        let leaving = i < arc.len() / 2;
        let half = if leaving { from } else { to };

        let ax = if leaving {
            x + air_stride * sign * i as i32
        } else {
            // Entering the far panel from the facing edge.
            let progressed = (i - arc.len() / 2) as i32 + 1;
            if sign > 0 {
                -sprite_w + air_stride * progressed
            } else {
                SCREEN_W as i32 - air_stride * progressed
            }
        };

        steps.push(frame(
            half,
            dir,
            ax,
            g - lift,
            scale,
            Legs::Tuck,
            flip_landing,
            !leaving,
        ));
    }

    // Landing run: carry on to the middle of the destination panel.
    let land_target = (SCREEN_W as i32 - sprite_w) / 2;
    let mut lx = if sign > 0 {
        -sprite_w + air_stride * 3
    } else {
        SCREEN_W as i32 - air_stride * 3
    };

    cycle = 0;
    while (sign > 0 && lx < land_target) || (sign < 0 && lx > land_target) {
        let legs = if cycle % 2 == 0 { Legs::RunB } else { Legs::RunA };
        steps.push(frame(to, dir, lx, g, scale, legs, flip_landing, true));
        lx += stride * sign;
        cycle += 1;
    }

    // Settle.
    steps.push(frame(
        to,
        dir,
        land_target,
        g,
        scale,
        Legs::Stand,
        flip_landing,
        true,
    ));

    steps
}

#[allow(clippy::too_many_arguments)]
fn frame(
    half: Half,
    dir: Dir,
    x: i32,
    y: i32,
    scale: i32,
    legs: Legs,
    flip_landing: bool,
    is_landing_panel: bool,
) -> Step {
    let mut fb = Framebuffer::new();
    fb.fill(BG);

    // Mirror the artwork when running left so he faces where he's going.
    let mirror = dir == Dir::RightToLeft;
    let grid = clawd::pose(if legs == Legs::Tuck { 1 } else { 0 }, legs, mirror);

    // A panel wired mirrored needs x reflected so he enters the correct edge.
    let dx = if flip_landing && is_landing_panel {
        SCREEN_W as i32 - clawd::W * scale - x
    } else {
        x
    };

    clawd::draw_pose(&mut fb, &grid, dx, y, scale, BG);
    Step { half, fb }
}

// ---------------------------------------------------------------------------
// Firmware-rendered variant.
//
// The firmware owns the sprite, so a frame is a handful of bytes rather than a
// screenful of pixels. That is what makes the two-screen jump possible: the
// slave half gets one small split-RPC packet per frame instead of hundreds.
// ---------------------------------------------------------------------------

/// One frame of Clawd's state on one panel.
#[derive(Copy, Clone, Debug)]
pub struct ClawdStep {
    pub half: Half,
    pub x: i16,
    pub pose: u8,
    pub lift: u8,
    /// Panel he just left, which should hide him this frame.
    pub hide_other: Option<Half>,
}

const SPRITE_W: i32 = 64; // 16 * 4, matching CLAWD_W in the firmware

/// Pose ids from the firmware's `enum clawd_pose`.
const P_RUN_A: u8 = 1;
const P_RUN_B: u8 = 2;
const P_TUCK: u8 = 3;
const P_MIRROR: u8 = 4;

/// Build the jump as a list of tiny state updates.
pub fn jump_states(dir: Dir) -> Vec<ClawdStep> {
    let (from, to) = dir.halves();
    let sign = dir.sign();
    let facing = if dir == Dir::RightToLeft { P_MIRROR } else { 0 };
    let mut out = Vec::new();

    let start_x = (SCREEN_W as i32 - SPRITE_W) / 2;
    let exit_x = if sign > 0 { SCREEN_W as i32 } else { -SPRITE_W };

    // Run-up on the departure panel.
    let stride = 6;
    let mut x = start_x;
    let mut cycle = 0;
    while (sign > 0 && x < exit_x) || (sign < 0 && x > exit_x) {
        out.push(ClawdStep {
            half: from,
            x: x as i16,
            pose: facing + if cycle % 2 == 0 { P_RUN_A } else { P_RUN_B },
            lift: 0,
            hide_other: None,
        });
        x += stride * sign;
        cycle += 1;
    }

    // Airborne. The apex lands on the panel boundary: he leaves one edge and
    // appears at the facing edge of the other on the same arc.
    let arc: [u8; 8] = [8, 16, 22, 26, 26, 22, 16, 8];
    let air_stride = 14;

    for (i, lift) in arc.iter().enumerate() {
        let leaving = i < arc.len() / 2;
        let half = if leaving { from } else { to };

        let ax = if leaving {
            x + air_stride * sign * i as i32
        } else {
            let step = (i - arc.len() / 2) as i32 + 1;
            if sign > 0 {
                -SPRITE_W + air_stride * step
            } else {
                SCREEN_W as i32 - air_stride * step
            }
        };

        out.push(ClawdStep {
            half,
            x: ax as i16,
            pose: facing + P_TUCK,
            lift: *lift,
            // The instant he arrives, hide him on the panel he left.
            hide_other: if !leaving && i == arc.len() / 2 {
                Some(from)
            } else {
                None
            },
        });
    }

    // Landing run to the middle of the destination panel.
    let land_target = (SCREEN_W as i32 - SPRITE_W) / 2;
    let mut lx = if sign > 0 {
        -SPRITE_W + air_stride * 4
    } else {
        SCREEN_W as i32 - air_stride * 4
    };
    cycle = 0;
    while (sign > 0 && lx < land_target) || (sign < 0 && lx > land_target) {
        out.push(ClawdStep {
            half: to,
            x: lx as i16,
            pose: facing + if cycle % 2 == 0 { P_RUN_B } else { P_RUN_A },
            lift: 0,
            hide_other: None,
        });
        lx += stride * sign;
        cycle += 1;
    }

    out.push(ClawdStep {
        half: to,
        x: land_target as i16,
        pose: facing,
        lift: 0,
        hide_other: None,
    });

    out
}

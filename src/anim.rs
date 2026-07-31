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

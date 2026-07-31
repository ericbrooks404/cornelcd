//! Wire protocol for the Corne Max LCDs.
//!
//! Reference: `~/vial-qmk/keyboards/mechboards/crkbd/rp2g/display/display.c`,
//! function `raw_hid_receive_kb()` and the comment block above it.
//!
//! ```text
//! data[0]   0x07        VIA command ID (custom set)
//! data[1]   0x00        channel; anything else is dropped
//! data[2]   command
//! data[3]   0x00 master half, 0x01 slave half
//! data[4:]  payload
//! ```

use hidapi::{HidApi, HidDevice};
use std::fmt;

pub const VID: u16 = 0x4653;
pub const PID: u16 = 0x0001;

/// Most chunks we will ever forward to the slave half in one go. Anything
/// beyond this risks wedging the split link; see `push_image`.
pub const SLAVE_CHUNK_LIMIT: usize = 24;

/// QMK's raw-HID interface advertises this vendor-defined usage.
pub const RAW_USAGE_PAGE: u16 = 0xFF60;
pub const RAW_USAGE: u16 = 0x61;

/// `RAW_EPSIZE` in tmk_core/protocol/usb_descriptor.h
pub const REPORT_LEN: usize = 32;

const VIA_CUSTOM_SET: u8 = 0x07;
const CHANNEL: u8 = 0x00;

/// Values of `display_data_type` in display.h. Kept complete to mirror the
/// firmware enum, so some variants have no caller yet.
#[allow(dead_code)]
#[derive(Copy, Clone, Debug)]
#[repr(u8)]
pub enum Cmd {
    Screen = 0,
    Status = 1,
    Time = 2,
    Cpu = 3,
    Gpu = 4,
    Ram = 5,
    Progress = 6,
    NowPlaying = 7,
    Image = 8,
    ImgFs = 9,
    ImgGif = 10,
    Clawd = 11,
    UsgText = 12,
    UsgBar = 13,
    UsgShow = 14,
}

/// Screen geometry. `lv_scr` is 25604 bytes for a 80x160 LV_IMG_CF_TRUE_COLOR
/// image, i.e. raw RGB565 little-endian with four bytes of slack.
pub const SCREEN_W: usize = 80;
pub const SCREEN_H: usize = 160;
pub const FB_LEN: usize = SCREEN_W * SCREEN_H * 2;

/// Which half to address. The master forwards slave-tagged packets over TRRS,
/// so both screens are reachable through the single USB connection.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Half {
    Master = 0,
    Slave = 1,
}

impl Half {
    pub fn both() -> [Half; 2] {
        [Half::Master, Half::Slave]
    }
}

/// Screen ids accepted by `draw_screen()`.
#[derive(Copy, Clone, Debug)]
#[repr(u8)]
pub enum Screen {
    WpmLayer = 0,
    PcStats = 1,
    Clock = 2,
    NowPlaying = 3,
    Gif = 4,
    Claude = 5,
}

impl Screen {
    pub fn from_id(n: u8) -> Option<Screen> {
        Some(match n {
            0 => Screen::WpmLayer,
            1 => Screen::PcStats,
            2 => Screen::Clock,
            3 => Screen::NowPlaying,
            4 => Screen::Gif,
            5 => Screen::Claude,
            _ => return None,
        })
    }
}

#[derive(Debug)]
pub enum Error {
    Hid(hidapi::HidError),
    NotFound,
    SlaveFloodRefused(usize),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Hid(e) => write!(f, "hid error: {e}"),
            Error::SlaveFloodRefused(n) => write!(
                f,
                "refusing to send {n} chunks to the slave half (limit {SLAVE_CHUNK_LIMIT}).\n\
                 Slave-bound reports are forwarded one at a time over TRRS; bulk\n\
                 pixel data there wedges the firmware and needs a power cycle.\n\
                 Render graphics on the master half, or move the drawing into firmware."
            ),
            Error::NotFound => write!(
                f,
                "no Corne Max raw-HID interface found (looked for {VID:04x}:{PID:04x}, \
                 usage page {RAW_USAGE_PAGE:04x}).\n\
                 Is the keyboard plugged in and running the Vial/VIA firmware?"
            ),
        }
    }
}

impl std::error::Error for Error {}

impl From<hidapi::HidError> for Error {
    fn from(e: hidapi::HidError) -> Self {
        Error::Hid(e)
    }
}

pub struct Keyboard {
    dev: HidDevice,
}

impl Keyboard {
    /// Open the raw-HID interface. The keyboard exposes several HID interfaces;
    /// only the one with usage page 0xFF60 accepts these reports.
    pub fn open() -> Result<Keyboard, Error> {
        let api = HidApi::new()?;

        let info = api
            .device_list()
            .find(|d| {
                d.vendor_id() == VID
                    && d.product_id() == PID
                    && d.usage_page() == RAW_USAGE_PAGE
                    && d.usage() == RAW_USAGE
            })
            .ok_or(Error::NotFound)?;

        Ok(Keyboard {
            dev: info.open_device(&api)?,
        })
    }

    /// Send one report. `payload` lands at data[4..] and is zero-padded.
    ///
    /// Zero-padding matters: the firmware's `read_string()` copies a fixed span
    /// and relies on a NUL to terminate, so trailing zeros keep short strings
    /// from picking up whatever was in the buffer.
    pub fn send(&self, cmd: Cmd, half: Half, payload: &[u8]) -> Result<(), Error> {
        let mut buf = [0u8; REPORT_LEN + 1];
        // buf[0] is the HID report ID. QMK's raw interface doesn't use report
        // IDs, so it stays 0 and hidapi strips it on the way out.
        buf[1] = VIA_CUSTOM_SET;
        buf[2] = CHANNEL;
        buf[3] = cmd as u8;
        buf[4] = half as u8;

        let room = REPORT_LEN - 4;
        let n = payload.len().min(room);
        buf[5..5 + n].copy_from_slice(&payload[..n]);

        self.dev.write(&buf)?;
        Ok(())
    }

    pub fn set_screen(&self, half: Half, screen: Screen) -> Result<(), Error> {
        self.send(Cmd::Screen, half, &[screen as u8])
    }

    pub fn set_time(&self, half: Half, text: &str) -> Result<(), Error> {
        self.send(Cmd::Time, half, text.as_bytes())
    }

    pub fn set_now_playing(&self, half: Half, text: &str) -> Result<(), Error> {
        self.send(Cmd::NowPlaying, half, text.as_bytes())
    }

    /// Bar/slider commands all take a single 0-100 byte.
    pub fn set_gauge(&self, cmd: Cmd, half: Half, percent: u8) -> Result<(), Error> {
        self.send(cmd, half, &[percent.min(100)])
    }

    /// Push raw bytes into one of the firmware's image buffers.
    ///
    /// The firmware does `memcpy(&buf[index * 25], &data[7], data[6])`, so each
    /// report carries a chunk index, a length, and up to 25 bytes.
    ///
    /// `only_chunks`, when given, restricts the push to those chunk indices —
    /// used to send just the pixels that changed between animation frames.
    pub fn push_image(
        &self,
        cmd: Cmd,
        half: Half,
        bytes: &[u8],
        only_chunks: Option<&[u16]>,
    ) -> Result<usize, Error> {
        const CHUNK: usize = 25;

        // Hard safety guard, learned the hard way.
        //
        // Master-bound reports are handled straight off USB. Slave-bound ones
        // are each forwarded as a separate `transaction_rpc_send` over TRRS,
        // a link shared with matrix scanning and far slower than USB. Pushing
        // a full 1024-chunk screen that way wedged the firmware hard enough
        // that USB interfaces 2 and 3 stopped enumerating (-110 ETIMEDOUT) and
        // only a full power cycle of both halves recovered it.
        //
        // Bulk pixel data must not go to the slave. Small control packets
        // (screen selection, gauges, strings) are fine — that is what the split
        // RPC was designed to carry.
        if half == Half::Slave {
            let n = only_chunks.map(|c| c.len()).unwrap_or(bytes.len().div_ceil(CHUNK));
            if n > SLAVE_CHUNK_LIMIT {
                return Err(Error::SlaveFloodRefused(n));
            }
        }
        let total = bytes.len().div_ceil(CHUNK);
        let mut payload = [0u8; 3 + CHUNK];
        let mut sent = 0;

        let indices: Vec<u16> = match only_chunks {
            Some(list) => list.to_vec(),
            None => (0..total as u16).collect(),
        };

        for idx in indices {
            let start = idx as usize * CHUNK;
            if start >= bytes.len() {
                continue;
            }
            let end = (start + CHUNK).min(bytes.len());
            let n = end - start;

            payload[0] = (idx >> 8) as u8;
            payload[1] = (idx & 0xFF) as u8;
            payload[2] = n as u8;
            payload[3..3 + n].copy_from_slice(&bytes[start..end]);
            payload[3 + n..].fill(0);

            self.send(cmd, half, &payload)?;
            sent += 1;
        }
        Ok(sent)
    }

    /// Tell the firmware to bind a freshly-pushed buffer to its widget.
    /// `which` is the image command whose buffer should be committed.
    pub fn commit_image(&self, half: Half, which: Cmd) -> Result<(), Error> {
        self.send(Cmd::Status, half, &[which as u8])
    }
}

/// Firmware-rendered Claude screen (id 5) and its state commands.
///
/// These carry a handful of bytes each, so unlike the image commands they are
/// safe to send to the slave half — one small RPC per frame is exactly what
/// the split link was designed to carry.
impl Keyboard {
    pub fn set_clawd(
        &self,
        half: Half,
        x: i16,
        pose: u8,
        lift: u8,
        visible: bool,
    ) -> Result<(), Error> {
        self.send(
            Cmd::Clawd,
            half,
            &[
                (x >> 8) as u8,
                (x & 0xFF) as u8,
                pose,
                lift,
                visible as u8,
            ],
        )
    }

    /// slot 0 = session total, slot 1 = 7-day total.
    pub fn set_usage_text(&self, half: Half, slot: u8, text: &str) -> Result<(), Error> {
        let mut p = vec![slot];
        p.extend_from_slice(text.as_bytes());
        p.push(0);
        self.send(Cmd::UsgText, half, &p)
    }

    pub fn set_usage_bar(&self, half: Half, percent: u8) -> Result<(), Error> {
        self.send(Cmd::UsgBar, half, &[percent.min(100)])
    }

    pub fn set_usage_shown(&self, half: Half, shown: bool) -> Result<(), Error> {
        self.send(Cmd::UsgShow, half, &[shown as u8])
    }
}

/// Pose indices, matching `enum clawd_pose` in the firmware's clawd_gfx.h.
/// Kept as documentation of the wire values; anim.rs uses the numbers directly.
#[allow(dead_code)]
#[derive(Copy, Clone, Debug)]
#[repr(u8)]
pub enum Pose {
    StandR = 0,
    RunAR = 1,
    RunBR = 2,
    TuckR = 3,
    StandL = 4,
    RunAL = 5,
    RunBL = 6,
    TuckL = 7,
}

impl Pose {
    pub fn mirrored(self, facing_left: bool) -> u8 {
        let base = self as u8 % 4;
        if facing_left { base + 4 } else { base }
    }
}

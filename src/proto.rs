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
}

impl Screen {
    pub fn from_id(n: u8) -> Option<Screen> {
        Some(match n {
            0 => Screen::WpmLayer,
            1 => Screen::PcStats,
            2 => Screen::Clock,
            3 => Screen::NowPlaying,
            4 => Screen::Gif,
            _ => return None,
        })
    }
}

#[derive(Debug)]
pub enum Error {
    Hid(hidapi::HidError),
    NotFound,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Hid(e) => write!(f, "hid error: {e}"),
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

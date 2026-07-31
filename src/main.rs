//! Drive the LCD screens on a Corne Max over raw HID.

mod clawd;
mod proto;
mod render;
mod screens;
mod sysinfo;
mod usage;

use proto::{Cmd, Half, Keyboard, Screen};
use std::process::ExitCode;
use std::thread::sleep;
use std::time::Duration;

const USAGE: &str = "\
cornelcd — drive the Corne Max LCD screens

USAGE:
    cornelcd <COMMAND>

COMMANDS:
    clock               Switch to the clock screen and keep the time updated
    stats               Switch to the PC-stats screen and feed CPU/GPU/RAM
    screen <0-4>        Switch screens and exit
                          0 WPM+layer  1 PC stats  2 clock
                          3 now-playing  4 gif
    text <string>       Set the now-playing label (screen 3)
    probe               Show which HID interfaces were found, then exit

OPTIONS:
    --half <master|slave|both>   Which screen to target (default: both)
    --interval <seconds>         Update period for clock/stats (default: 1)
";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "-h" || args[0] == "--help" {
        print!("{USAGE}");
        return Ok(());
    }

    let halves = parse_halves(&args)?;
    let interval = parse_interval(&args)?;

    match args[0].as_str() {
        "probe" => return probe(),
        "testimg" => {
            // Proves the _IMG_FS path end to end: build a raw RGB565 frame,
            // push it, commit it. Vertical colour bands so a wrong stride or
            // byte order is obvious at a glance rather than subtly wrong.
            let mut fb = vec![0u8; proto::FB_LEN];
            for y in 0..proto::SCREEN_H {
                for x in 0..proto::SCREEN_W {
                    let (r, g, b) = match x * 4 / proto::SCREEN_W {
                        0 => (0xFF, 0x00, 0x00),
                        1 => (0x00, 0xFF, 0x00),
                        2 => (0x00, 0x00, 0xFF),
                        _ => (0xD9, 0x77, 0x57), // Claude orange
                    };
                    let px = rgb565(r, g, b);
                    let o = (y * proto::SCREEN_W + x) * 2;
                    // Big-endian: the panel reads the high byte first.
                    fb[o] = (px >> 8) as u8;
                    fb[o + 1] = (px & 0xFF) as u8;
                }
            }

            let kb = Keyboard::open()?;
            let t = std::time::Instant::now();
            for h in &halves {
                let n = kb.push_image(Cmd::ImgFs, *h, &fb, None)?;
                kb.commit_image(*h, Cmd::ImgFs)?;
                println!("pushed {n} chunks ({} bytes) to {h:?}", fb.len());
            }
            println!("took {:.2}s", t.elapsed().as_secs_f32());
        }
        "usage" => {
            let t = usage::collect()?;
            println!(
                "session {} — {} billable ({} in, {} out, {} cache-read)",
                t.session_name,
                usage::short(t.session.billable()),
                usage::short(t.session.input + t.session.cache_write),
                usage::short(t.session.output),
                usage::short(t.session.cache_read),
            );
            println!(
                "7 days  — {} billable across {} transcript(s)",
                usage::short(t.week.billable()),
                t.files_scanned
            );
        }
        "claude" => {
            let kb = Keyboard::open()?;
            run_claude(&kb, interval)?;
        }
        "clock" => {
            let kb = Keyboard::open()?;
            for h in &halves {
                kb.set_screen(*h, Screen::Clock)?;
            }
            println!("clock running on {} — ctrl-c to stop", describe(&halves));
            run_clock(&kb, &halves, interval)?;
        }
        "stats" => {
            let kb = Keyboard::open()?;
            for h in &halves {
                kb.set_screen(*h, Screen::PcStats)?;
            }
            println!("stats running on {} — ctrl-c to stop", describe(&halves));
            run_stats(&kb, &halves, interval)?;
        }
        "screen" => {
            let n: u8 = args
                .get(1)
                .ok_or("screen needs an id, 0-4")?
                .parse()
                .map_err(|_| "screen id must be a number 0-4")?;
            let screen = Screen::from_id(n).ok_or("screen id must be 0-4")?;
            let kb = Keyboard::open()?;
            for h in &halves {
                kb.set_screen(*h, screen)?;
            }
            println!("set screen {n} ({screen:?}) on {}", describe(&halves));
        }
        "text" => {
            let text = args.get(1).ok_or("text needs a string")?;
            let kb = Keyboard::open()?;
            for h in &halves {
                kb.set_screen(*h, Screen::NowPlaying)?;
                kb.set_now_playing(*h, text)?;
            }
            println!("set text on {}", describe(&halves));
        }
        other => {
            eprintln!("unknown command: {other}\n");
            print!("{USAGE}");
            return Err("unknown command".into());
        }
    }

    Ok(())
}

/// Left half shows usage, right half animates Clawd.
///
/// A full frame is 1024 chunks and takes about a second, so only the chunks
/// that actually changed get pushed. Clawd's body is identical between frames,
/// so the animation touches a small fraction of the screen.
fn run_claude(kb: &Keyboard, interval: Duration) -> Result<(), Box<dyn std::error::Error>> {
    // Send both halves a full frame once, then diff from here on.
    let mut last_usage: Option<render::Framebuffer> = None;
    let mut last_clawd: Option<render::Framebuffer> = None;

    let mut frame = 0usize;
    let mut ticks = 0u32;
    let mut week_ref: u64 = 0;

    println!("claude screens running (left: usage, right: clawd) — ctrl-c to stop");

    loop {
        // Usage is expensive to recompute and barely moves; refresh it on the
        // first tick and then every 30 animation frames.
        if ticks % 30 == 0 {
            let t = usage::collect()?;
            // Keep the bar's reference at the high-water mark so it stays
            // meaningful without pretending to know a real quota.
            week_ref = week_ref.max(t.week.billable()).max(1);
            let fb = screens::usage_screen(&t, week_ref);
            push(kb, Half::Master, &fb, last_usage.as_ref())?;
            last_usage = Some(fb);
        }

        let fb = screens::clawd_screen(frame);
        push(kb, Half::Slave, &fb, last_clawd.as_ref())?;
        last_clawd = Some(fb);

        frame += 1;
        ticks += 1;
        sleep(interval);
    }
}

/// Push a framebuffer, sending only what changed when we have a previous one.
fn push(
    kb: &Keyboard,
    half: Half,
    fb: &render::Framebuffer,
    prev: Option<&render::Framebuffer>,
) -> Result<usize, Box<dyn std::error::Error>> {
    let n = match prev {
        Some(p) => {
            let changed = fb.diff_chunks(p);
            if changed.is_empty() {
                return Ok(0);
            }
            kb.push_image(Cmd::ImgFs, half, &fb.data, Some(&changed))?
        }
        None => kb.push_image(Cmd::ImgFs, half, &fb.data, None)?,
    };
    kb.commit_image(half, Cmd::ImgFs)?;
    Ok(n)
}

fn run_clock(
    kb: &Keyboard,
    halves: &[Half],
    interval: Duration,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut last = String::new();
    loop {
        let now = chrono::Local::now().format("%H:%M").to_string();
        // The screen only needs a write when the displayed value changes.
        if now != last {
            for h in halves {
                kb.set_time(*h, &now)?;
            }
            last = now;
        }
        sleep(interval);
    }
}

fn run_stats(
    kb: &Keyboard,
    halves: &[Half],
    interval: Duration,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut prev = sysinfo::read_cpu_times()?;
    loop {
        sleep(interval);

        let now = sysinfo::read_cpu_times()?;
        let cpu = sysinfo::cpu_percent(prev, now);
        prev = now;

        let ram = sysinfo::ram_percent()?;
        let gpu = sysinfo::gpu_percent();

        for h in halves {
            kb.set_gauge(Cmd::Cpu, *h, cpu)?;
            kb.set_gauge(Cmd::Ram, *h, ram)?;
            if let Some(g) = gpu {
                kb.set_gauge(Cmd::Gpu, *h, g)?;
            }
        }
    }
}

fn probe() -> Result<(), Box<dyn std::error::Error>> {
    let api = hidapi::HidApi::new()?;
    let mut found = 0;

    println!("HID interfaces for {:04x}:{:04x}", proto::VID, proto::PID);
    for d in api.device_list() {
        if d.vendor_id() != proto::VID || d.product_id() != proto::PID {
            continue;
        }
        found += 1;
        let raw = d.usage_page() == proto::RAW_USAGE_PAGE && d.usage() == proto::RAW_USAGE;
        println!(
            "  usage_page={:#06x} usage={:#04x} iface={} {} {}",
            d.usage_page(),
            d.usage(),
            d.interface_number(),
            d.path().to_string_lossy(),
            if raw { "  <-- raw HID (this one)" } else { "" }
        );
    }

    if found == 0 {
        println!("  (none — keyboard not connected?)");
    }
    Ok(())
}

fn parse_halves(args: &[String]) -> Result<Vec<Half>, Box<dyn std::error::Error>> {
    let Some(i) = args.iter().position(|a| a == "--half") else {
        return Ok(Half::both().to_vec());
    };
    match args.get(i + 1).map(String::as_str) {
        Some("master") => Ok(vec![Half::Master]),
        Some("slave") => Ok(vec![Half::Slave]),
        Some("both") | None => Ok(Half::both().to_vec()),
        Some(other) => Err(format!("--half must be master, slave or both (got {other})").into()),
    }
}

fn parse_interval(args: &[String]) -> Result<Duration, Box<dyn std::error::Error>> {
    let Some(i) = args.iter().position(|a| a == "--interval") else {
        return Ok(Duration::from_secs(1));
    };
    let secs: f64 = args
        .get(i + 1)
        .ok_or("--interval needs a value in seconds")?
        .parse()
        .map_err(|_| "--interval must be a number")?;
    if secs <= 0.0 {
        return Err("--interval must be positive".into());
    }
    Ok(Duration::from_secs_f64(secs))
}

fn describe(halves: &[Half]) -> String {
    if halves.len() == 2 {
        "both halves".into()
    } else {
        format!("{:?} half", halves[0]).to_lowercase()
    }
}

/// Pack 8-bit RGB into RGB565.
fn rgb565(r: u8, g: u8, b: u8) -> u16 {
    ((r as u16 & 0xF8) << 8) | ((g as u16 & 0xFC) << 3) | (b as u16 >> 3)
}

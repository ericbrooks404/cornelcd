//! Drive the LCD screens on a Corne Max over raw HID.

mod proto;
mod sysinfo;

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

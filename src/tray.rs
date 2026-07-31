//! System-tray daemon.
//!
//! The worker thread does the real job (watch the transcript, drive Clawd); the
//! tray just exposes on/off, status, and quit.
//!
//! Linux speaks StatusNotifierItem over D-Bus, which waybar, GNOME and KDE all
//! implement, so there is no GTK dependency. Windows uses Shell_NotifyIcon via
//! `tray-icon` plus a bare Win32 message pump.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// State shared between the tray UI and the worker thread.
pub struct Shared {
    pub enabled: AtomicBool,
    pub quit: AtomicBool,
    pub connected: AtomicBool,
    pub status: Mutex<String>,
}

impl Shared {
    pub fn new() -> Arc<Shared> {
        Arc::new(Shared {
            enabled: AtomicBool::new(true),
            quit: AtomicBool::new(false),
            connected: AtomicBool::new(false),
            status: Mutex::new("starting".into()),
        })
    }

    pub fn status_line(&self) -> String {
        let s = self.status.lock().map(|g| g.clone()).unwrap_or_default();
        if !self.connected.load(Ordering::Relaxed) {
            return "keyboard not connected".into();
        }
        if !self.enabled.load(Ordering::Relaxed) {
            return "paused — Clawd running solo".into();
        }
        s
    }

    pub fn set_status(&self, s: impl Into<String>) {
        if let Ok(mut g) = self.status.lock() {
            *g = s.into();
        }
    }
}

/// Clawd as a tray icon: the stock sprite at 2x, centred in a 32x32 RGBA field.
pub fn icon_rgba() -> (Vec<u8>, u32, u32) {
    const SIZE: usize = 32;
    const SCALE: usize = 2;
    let grid = &crate::clawd::FRAMES[0];

    let mut px = vec![0u8; SIZE * SIZE * 4];
    let sw = crate::clawd::W as usize * SCALE;
    let sh = crate::clawd::H as usize * SCALE;
    let ox = (SIZE - sw) / 2;
    let oy = (SIZE.saturating_sub(sh)) / 2;

    for (row, line) in grid.iter().enumerate() {
        for (col, ch) in line.chars().enumerate() {
            let Some(rgb) = crate::clawd::pal_pub(ch) else {
                continue;
            };
            for dy in 0..SCALE {
                for dx in 0..SCALE {
                    let x = ox + col * SCALE + dx;
                    let y = oy + row * SCALE + dy;
                    if x >= SIZE || y >= SIZE {
                        continue;
                    }
                    let o = (y * SIZE + x) * 4;
                    px[o] = ((rgb >> 16) & 0xFF) as u8;
                    px[o + 1] = ((rgb >> 8) & 0xFF) as u8;
                    px[o + 2] = (rgb & 0xFF) as u8;
                    px[o + 3] = 0xFF;
                }
            }
        }
    }
    (px, SIZE as u32, SIZE as u32)
}

// ---------------------------------------------------------------------------
// Linux: StatusNotifierItem
// ---------------------------------------------------------------------------
#[cfg(target_os = "linux")]
pub fn run(shared: Arc<Shared>) -> Result<(), Box<dyn std::error::Error>> {
    use ksni::menu::{CheckmarkItem, MenuItem, StandardItem};

    struct ClawdTray {
        shared: Arc<Shared>,
    }

    impl ksni::Tray for ClawdTray {
        fn id(&self) -> String {
            "cornelcd".into()
        }
        fn title(&self) -> String {
            "Clawd".into()
        }
        fn tool_tip(&self) -> ksni::ToolTip {
            ksni::ToolTip {
                title: "Clawd".into(),
                description: self.shared.status_line(),
                ..Default::default()
            }
        }
        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            let (rgba, w, h) = icon_rgba();
            // SNI wants ARGB32 in network byte order.
            let mut argb = Vec::with_capacity(rgba.len());
            for p in rgba.chunks(4) {
                argb.extend_from_slice(&[p[3], p[0], p[1], p[2]]);
            }
            vec![ksni::Icon {
                width: w as i32,
                height: h as i32,
                data: argb,
            }]
        }
        fn menu(&self) -> Vec<MenuItem<Self>> {
            vec![
                StandardItem {
                    label: self.shared.status_line(),
                    enabled: false,
                    ..Default::default()
                }
                .into(),
                MenuItem::Separator,
                CheckmarkItem {
                    label: "Report Claude activity".into(),
                    checked: self.shared.enabled.load(Ordering::Relaxed),
                    activate: Box::new(|t: &mut ClawdTray| {
                        let now = !t.shared.enabled.load(Ordering::Relaxed);
                        t.shared.enabled.store(now, Ordering::Relaxed);
                    }),
                    ..Default::default()
                }
                .into(),
                MenuItem::Separator,
                StandardItem {
                    label: "Quit".into(),
                    activate: Box::new(|t: &mut ClawdTray| {
                        t.shared.quit.store(true, Ordering::Relaxed);
                    }),
                    ..Default::default()
                }
                .into(),
            ]
        }
    }

    let service = ksni::TrayService::new(ClawdTray {
        shared: shared.clone(),
    });
    let handle = service.handle();
    service.spawn();

    // Refresh the menu and tooltip as state changes.
    while !shared.quit.load(Ordering::Relaxed) {
        std::thread::sleep(std::time::Duration::from_secs(1));
        handle.update(|_: &mut ClawdTray| {});
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Windows: Shell_NotifyIcon
// ---------------------------------------------------------------------------
#[cfg(windows)]
pub fn run(shared: Arc<Shared>) -> Result<(), Box<dyn std::error::Error>> {
    use tray_icon::menu::{Menu, MenuEvent, MenuItem as WinMenuItem, PredefinedMenuItem};
    use tray_icon::{TrayIconBuilder, TrayIconEvent};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
    };

    let (rgba, w, h) = icon_rgba();
    let icon = tray_icon::Icon::from_rgba(rgba, w, h)?;

    let toggle = WinMenuItem::new("Report Claude activity", true, None);
    let quit = WinMenuItem::new("Quit", true, None);
    let menu = Menu::new();
    menu.append(&toggle)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&quit)?;

    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Clawd")
        .with_icon(icon)
        .build()?;

    let menu_rx = MenuEvent::receiver();
    let _ = TrayIconEvent::receiver();

    // tray-icon delivers events through the thread's message queue, so we must
    // pump it. PeekMessage keeps this non-blocking so the quit flag is honoured.
    let mut msg: MSG = unsafe { std::mem::zeroed() };
    while !shared.quit.load(Ordering::Relaxed) {
        unsafe {
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        while let Ok(ev) = menu_rx.try_recv() {
            if ev.id == quit.id() {
                shared.quit.store(true, Ordering::Relaxed);
            } else if ev.id == toggle.id() {
                let now = !shared.enabled.load(Ordering::Relaxed);
                shared.enabled.store(now, Ordering::Relaxed);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Ok(())
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn run(_shared: Arc<Shared>) -> Result<(), Box<dyn std::error::Error>> {
    Err("tray is only implemented for Linux and Windows".into())
}

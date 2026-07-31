# cornelcd

Drives the LCD screens on a **Corne Max** (Mechboards R2G PCB, dual RP2040) over
raw HID. Rust, two dependencies.

The firmware ships five screen layouts but no host software to feed them —
CannonKeys' own tool is still listed as WIP. This fills that gap.

## Install

Every installer sets the daemon to start at login.

**Arch**

```sh
cd packaging/arch && makepkg -si
systemctl --user start cornelcd     # or just log out and back in
```

**Debian / Ubuntu** — grab the `.deb` from
[Releases](https://github.com/ericbrooks404/cornelcd/releases):

```sh
sudo apt install ./cornelcd_0.1.0-1_amd64.deb
systemctl --user start cornelcd
```

**Windows** — run `cornelcd-setup.exe` from Releases. Per-user install, no admin
rights, and it adds itself to the login Run key. Uninstall via Add/Remove
Programs.

### Build from source

```sh
cargo build --release
# -> target/release/cornelcd
```

On Arch, if you package it yourself, keep `options=(!lto)` in the PKGBUILD:
hidapi's bundled C is compiled by the `cc` crate, and makepkg's `-flto` makes
those objects LTO bitcode that rust's linker cannot resolve
(`undefined symbol: hid_open`).

## The tray daemon

```sh
cornelcd daemon
```

Sits in the system tray as Clawd. Right-click for status, a toggle for whether
to report Claude's activity, and Quit.

- **Linux** uses StatusNotifierItem over D-Bus — what waybar, GNOME and KDE all
  speak — so there is no GTK dependency.
- **Windows** uses `Shell_NotifyIcon` with a small Win32 message pump.

The worker reconnects on its own, so unplugging the keyboard, reflashing it, or
suspending the machine are all non-events. If the daemon stops entirely, the
firmware watchdog forgets it within 10s and Clawd goes back to his autonomous
routine — nothing on the keyboard depends on this being alive.

### Autostart

Linux packages run `systemctl --global enable cornelcd.service`, which enables
the user unit for every login session without the package needing to know the
username. Windows uses the per-user `HKCU\...\CurrentVersion\Run` key.

## Use

```sh
cornelcd claude                 # THE GOOD ONE: two-screen Clawd + usage, firmware-rendered
cornelcd claude-img             # older host-rendered mode, master panel only
cornelcd usage                  # print token totals to the terminal
cornelcd clock                  # clock screen, keeps the time updated
cornelcd stats                  # CPU / GPU / RAM bars
cornelcd screen 0               # switch screens and exit (0-4)
cornelcd text "now playing"     # set the label on the now-playing screen
cornelcd testimg                # colour bands, for checking byte order/stride
cornelcd probe                  # list HID interfaces, show which one is raw HID
```

### Run it at login

```sh
systemctl --user enable --now cornelcd
```

Unit lives at `~/.config/systemd/user/cornelcd.service`. It restarts forever, so
unplugging the keyboard or flashing it is harmless.

## Two-screen mode (the good one)

`cornelcd claude` drives **screen 5**, a custom screen added to the firmware in
`~/vial-qmk` (branch `corne_max`). Usage lives on one panel, Clawd on the other,
and they trade places every time he makes a running jump across the gap.

This required a firmware change for a hard reason. See "Why the slave half needs
firmware rendering" below — the short version is that a frame here costs **5
bytes instead of ~2500**, which is the difference between working and wedging
the keyboard.

```sh
cornelcd claude --jump-every 20 --interval 0.25
cornelcd clawd --half slave --x 8 --pose 1   # place him by hand, for debugging
```

Poses are `0-3` facing right (stand, run A, run B, tuck) and `4-7` mirrored.

### Rebuilding the firmware

```sh
# Regenerate the sprite if the art or scale changes
cornelcd gensprite --scale 4 > ~/vial-qmk/keyboards/mechboards/crkbd/rp2g/gfx/clawd_gfx.c

cd ~/vial-qmk && make mechboards/crkbd/rp2g:vial ALLOW_WARNINGS=yes
~/qmk_userspace/flash.sh vial     # once per half
```

**Flash with double-tap reset, never by holding Q or P.** Bootmagic wipes the
EEPROM, which is where your Vial keymap lives. A plain reflash is safe:
`WEAR_LEVELING_RP2040_FLASH_BASE` is anchored to the end of flash, so firmware
size changes don't move the EEPROM region.

## Why the slave half needs firmware rendering

Master-bound reports are handled straight off USB. Slave-bound ones are each
forwarded as a separate `transaction_rpc_send` over TRRS — a link shared with
matrix scanning and far slower than USB.

Pushing a full 1024-chunk screen that way **wedged the firmware**: USB
interfaces 2 and 3 stopped enumerating (`can't add hid device: -110`), the right
panel froze on a stale image, and only unplugging both USB *and* TRRS for ~20s
recovered it. `push_image` now refuses slave writes over `SLAVE_CHUNK_LIMIT`
chunks.

Small control packets to the slave are fine — that is what the split RPC is for.
So the fix was to move the drawing into firmware and send state, not pixels.

## Host-rendered screens (`claude-img`)

`cornelcd claude-img` renders **host-side** as a raw RGB565 framebuffer and
pushes it with `_IMG_FS`. It needs no firmware change, which makes it the
fallback if you ever run stock firmware — but it is **master panel only**, for
the reason above.

It draws usage and a patrolling Clawd on the one panel. Usage comes from
`~/.claude/projects/*/*.jsonl`: session total, 7-day total, in/out/cache
breakdown. ~17 ms to scan 176 MB, since files older than a week are skipped by
mtime and lines without `"usage"` are rejected before parsing.

Clawd's sprite is the real one, extracted from the Claude Code binary
(`CLAWD_FRAMES` / `CLAWD_PAL`): 16x14, two frames, `#D97757` body, `#2A1F1B`
eyes. See `src/clawd.rs`.

### Two things that cost real time to discover

**The panel reads RGB565 big-endian.** Little-endian renders correct *shapes* in
wrong *colours* — red shows as blue, orange as purple. Byte packing is
centralised in `render.rs` so this can only be got wrong once.

**Bandwidth is ~25 KB/s.** One 25-byte chunk per USB frame, so a full 25,600-byte
screen takes about a second. Animation therefore pushes only the chunks that
changed (`Framebuffer::diff_chunks`). Big moving sprites are expensive; small
ones are cheap.

### The weekly bar is a trend, not a quota

Claude Code stores no rate-limit or quota data on disk — checked for
`rate_limit`, `resets_at`, `remaining`, `quota`, `utilization` across every
transcript, none present. So the bar is scaled against a running high-water mark.
**The token numbers are exact; the bar's fullness is relative to your own peak,
not a real percentage.**

Options:

| Flag | Default | Meaning |
|------|---------|---------|
| `--half master\|slave\|both` | `both` | which screen to target |
| `--interval <seconds>` | `1` | update period for `clock` / `stats` |

Screens:

| ID | Layout |
|----|--------|
| 0 | WPM + layer indicator (firmware default at boot) |
| 1 | CPU / GPU / RAM bars |
| 2 | Clock |
| 3 | Now-playing: track label, album art, progress |
| 4 | GIF |

## How it works

32-byte reports to the keyboard's raw-HID interface (usage page `0xFF60`):

```
data[0]  0x07        VIA custom-set command
data[1]  0x00        channel
data[2]  command     see proto::Cmd
data[3]  0x00 master / 0x01 slave
data[4:] payload
```

Byte 3 is why one USB cable drives both screens: the master half forwards
slave-tagged packets to the other side over TRRS.

Protocol reference lives in `src/proto.rs`, mirroring
`~/vial-qmk/keyboards/mechboards/crkbd/rp2g/display/display.c`.

## Notes

- Works on both the Vial and QMK/VIA firmware. Vial routes unrecognised command
  IDs through to `raw_hid_receive_kb()`, which is the same handler VIA calls as
  `via_custom_value_command_kb()`.
- No root needed. The `59-vial.rules` udev rule tags the device `uaccess`, which
  grants the active session an ACL on the hidraw nodes.
- GPU is best-effort: it reads `gpu_busy_percent`, which amdgpu exposes but
  Intel i915/xe does not. When unavailable the GPU bar is simply left alone
  rather than shown as a fake zero.
- Payloads are zero-padded. The firmware's `read_string()` copies a fixed span
  and relies on a NUL terminator, so the padding keeps short strings clean.

## Not implemented yet

- **Album-art and GIF push** (`_IMAGE`, `_IMG_GIF`). `_IMG_FS` is implemented;
  these two are not. Same chunked transfer into 8 KB and 125 KB buffers.
- **The firmware's GIF path is broken anyway**: the `ezgif` descriptor is
  declared but never populated (`display.c` has the line that would set its size
  commented out), so screen 4 renders nothing. Fixing that would give native
  in-firmware GIF animation at full frame rate.
- **Real quota percentages.** Only token counts are recoverable locally.

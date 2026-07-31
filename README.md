# cornelcd

Drives the LCD screens on a **Corne Max** (Mechboards R2G PCB, dual RP2040) over
raw HID. Rust, two dependencies.

The firmware ships five screen layouts but no host software to feed them —
CannonKeys' own tool is still listed as WIP. This fills that gap.

## Build

```sh
cargo build --release
# -> target/release/cornelcd
```

## Use

```sh
cornelcd claude                 # THE GOOD ONE: usage on the left, Clawd on the right
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

## Custom screens

`cornelcd claude` renders both screens **host-side** as raw RGB565 framebuffers
and pushes them with `_IMG_FS`. No firmware change, so a Vial keymap is never at
risk.

- **Left (master)** — Claude Code token usage, read from
  `~/.claude/projects/*/*.jsonl`. Session total, 7-day total, in/out/cache
  breakdown. ~17 ms to scan 176 MB: files older than a week are skipped by
  mtime, and lines without `"usage"` are rejected before parsing.
- **Right (slave)** — Clawd waving his paintbrush.

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

Image push (`_IMAGE` / `_IMG_FS` / `_IMG_GIF`, commands 8-10). These transfer in
25-byte chunks into buffers of 8 KB, 25 KB and 125 KB respectively, then commit
with a `_STATUS` write. Pixel format is RGB565 at 80x160. The constants are
already in `proto.rs`.

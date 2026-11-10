# designer

A small Windows tray application for controlling monitor brightness and contrast over **DDC/CI**.

The tray icon exposes one submenu per physical monitor with the current value and `+`/`-` buttons.
All hardware access happens on a background worker, so the menu always stays responsive even when a
monitor is slow or refuses DDC/CI commands.

## Features

- Per-monitor brightness and contrast submenus with step buttons (5% by default).
- Optimistic menu labels with a verification read once a click burst settles.
- Graceful degradation: a monitor that does not answer is shown as `unavailable`, the app keeps
  running and logs the exact error.
- Last confirmed values are persisted as JSON and can optionally be re-applied on startup.
- Single-instance guard, so two tray icons never fight over the same monitors.
- `--dump` diagnostic mode that prints raw VCP values.
- No asset files: the tray icon is rasterised at runtime.

## Requirements

- Windows 10 or Windows 11.
- A monitor with **DDC/CI enabled in its on-screen menu**. Many laptops' internal eDP panels do not
  support DDC/CI at all; external displays usually do.
- The monitor must be awake and on the input that is actually displayed. KVMs and some adapters
  block DDC traffic.

## Build

```sh
cargo build --release
```

The binary is `target\release\designer.exe`. Release builds use the Windows subsystem, so no console
window appears.

Run it directly, or place a shortcut in `shell:startup` if the app should launch with Windows
(there is no built-in autostart switch).

## Tray menu

```
Display 1: Generic PnP Monitor
    Brightness: 45%
    Brightness +5%
    Brightness -5%
    --------------------------
    Contrast: 60%
    Contrast +5%
    Contrast -5%
--------------------------------
Refresh monitors
Restore saved brightness
--------------------------------
Quit
```

- Items are disabled while the corresponding value is unknown (DDC/CI disabled, panel asleep,
  unsupported feature).
- `Refresh monitors` drops all handles, re-enumerates displays and re-reads values. Use it after
  hot-plugging a display or resuming from sleep.
- Double-clicking the tray icon also triggers a refresh.
- `Quit` removes the tray icon cleanly.

## Diagnostics

```sh
target\debug\designer.exe --dump
```

`--dump` prints every detected display with the backend name and raw brightness/contrast values.
In a release build stdout is not attached to a console, so the same text is written to the log file;
use a debug build (`cargo run -- --dump`) to see it directly.

## Configuration

The file is created on first launch (with defaults) at:

```
%APPDATA%\designer\config.json
```

```json
{
  "version": 1,
  "step_percent": 5,
  "min_percent": 5,
  "min_interval_ms": 50,
  "restore_on_start": false,
  "monitors": [
    {
      "index": 0,
      "description": "Generic PnP Monitor",
      "brightness": 45,
      "contrast": 60
    }
  ]
}
```

| Field | Default | Meaning |
| --- | --- | --- |
| `version` | `1` | Schema version, rewritten automatically. |
| `step_percent` | `5` | Delta used by the `+`/`-` menu items. |
| `min_percent` | `5` | Lower brightness clamp. Keeps the panel readable. |
| `min_interval_ms` | `50` | Minimum delay between DDC/CI commands per monitor. Some drivers need more; allowed range is 20-1000. |
| `restore_on_start` | `false` | Re-apply saved values shortly after startup. |
| `monitors` | `[]` | Last confirmed per-monitor values. |

Notes:

- The file is written atomically (temporary file plus rename). A corrupt file is moved to
  `config.json.bak` and defaults are used.
- Out-of-range values, including a hand-edited `step_percent` of 0, are clamped on load.
- Values are saved only after the hardware accepted and confirmed a write, so a rejected command
  cannot poison the saved state.
- Monitors are matched by index plus description. If two identical panels share a description and
  the indices changed, the ambiguous restore is skipped rather than applied to the wrong panel.

## Logs

```
%APPDATA%\designer\designer.log
```

Release builds have no console, so this file is the primary diagnostic channel. It records startup,
enumeration results, write results, DDC/CI errors, configuration failures and panics. Debug builds
also log to stderr.

## Troubleshooting

**`Brightness: unavailable` and I2C errors in the log**

The monitor did not answer a DDC/CI request. Check, in this order:

1. DDC/CI is enabled in the monitor's OSD menu.
2. The monitor is fully awake and on the same input Windows displays.
3. No KVM, capture card or passive adapter sits between the GPU and the panel.
4. Try another cable or port; some USB-C docks and DisplayLink adapters do not pass DDC through.

**Only one display is listed while two are attached**

Mirrored displays share one physical monitor and are enumerated once. After changing the display
configuration, use `Refresh monitors`.

**Values jump back after a click**

The panel clamped the value or rejected the write. The menu reverts to the last confirmed value and
the log contains the backend error.

**The app starts but there is no tray icon**

Check the log for a fatal error, and make sure no other instance is running. The single-instance
guard makes the second launch exit immediately and log that fact.

## Architecture

| Module | Responsibility |
| --- | --- |
| `src/main.rs` | Subsystem attribute, logging, panic hook, single-instance guard, CLI. |
| `src/app.rs` | tao event loop, event dispatch, optimistic state, persistence, shutdown. |
| `src/worker.rs` | Worker thread, command queue, click coalescing, verification reads. |
| `src/monitor/controller.rs` | The only module touching `ddc-hi`; enumeration, VCP reads/writes, throttle, backoff. |
| `src/monitor/scale.rs` | Percentage to raw VCP conversion and back. |
| `src/config.rs` | JSON persistence, defaults, atomic save, corruption recovery. |
| `src/tray/` | Tray icon lifecycle, native menu construction, generated icon. |
| `src/error.rs`, `src/logging.rs` | Shared error types and file logging. |

Design points:

- Display handles are created, used and destroyed on the worker thread; only plain data crosses
  thread boundaries.
- Rapid clicks for the same field are collapsed into one final write, followed by one verification
  read when the queue is idle.
- A 50 ms (configurable) gap between commands per monitor avoids driver-level DDC failures.
- After three consecutive read failures a monitor's reads are paused for 20 seconds to avoid
  hammering a disabled or sleeping panel; a successful write clears the backoff.
- On write failure the UI reverts the label to the last confirmed value.

## Development checks

```sh
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test --all-targets

# Requires a real DDC/CI capable monitor. Reads the current brightness and
# writes the same percentage back, so nothing visible changes.
cargo test hardware_write_roundtrip -- --ignored --nocapture
```

## Limitations

- No global hotkeys, presets or settings window (deliberately lean scope).
- Internal laptop panels usually cannot be controlled via DDC/CI.
- Identical monitors with duplicate descriptions may not restore values reliably after being
  reordered.

# designer

A small Windows tray application for controlling monitor brightness over **DDC/CI**.

The tray icon exposes one submenu per physical monitor with a flat list of brightness values from
`0%` to `100%` in 10% steps. The value matching the panel's current brightness carries a checkmark.
All hardware access happens on a background worker, so the menu always stays responsive even when a
monitor is slow or refuses DDC/CI commands.

## Download

Prebuilt Windows binaries are attached to the
[latest release](https://github.com/ROKBAS/designer/releases/latest):

| Asset | Contents |
| --- | --- |
| `designer.exe` | Portable single-file application. No installer, no runtime dependencies. |
| `designer-<version>-windows-x64.zip` | The same executable with this README and the license. |
| `SHA256SUMS.txt` | SHA-256 checksums for both downloads. |

Download `designer.exe`, move it anywhere you like and double-click it. The tray icon appears in the
notification area.

Windows may show a SmartScreen warning because the executable is not code-signed: choose
**More info → Run anyway**. The app writes only `%APPDATA%\designer\config.json` and
`%APPDATA%\designer\designer.log`; nothing is installed and no registry keys are created.

## Features

- One brightness submenu per monitor: `0%`, `10%`, ..., `100%`, with the current value checked.
- Optimistic checkmarks followed by a verification read once a click burst settles.
- Graceful degradation: a monitor that does not answer is shown as `unavailable` and its values are
  disabled; the app keeps running and logs the exact error.
- Last confirmed brightness is persisted as JSON and can optionally be re-applied on startup.
- Single-instance guard, so two tray icons never fight over the same monitors.
- `--dump` diagnostic mode that prints raw VCP values.
- No asset files: the tray icon is rasterised at runtime.
- Portable single executable: no installer, no background service, no runtime dependencies.

## Requirements

- Windows 10 or Windows 11.
- A monitor with **DDC/CI enabled in its on-screen menu**. Many laptops' internal eDP panels do not
  support DDC/CI at all; external displays usually do.
- The monitor must be awake and on the input that is actually displayed. KVMs and some adapters
  block DDC traffic.

## Build

```sh
cargo build --release --locked
```

The binary is `target\release\designer.exe`. Release builds use the Windows subsystem, so no console
window appears.

Run it directly, or place a shortcut in `shell:startup` if the app should launch with Windows
(there is no built-in autostart switch).

## Publishing a release

The [Release workflow](.github/workflows/release.yml) builds the executable on `windows-latest`,
runs formatting, clippy and the test suite, then attaches `designer.exe`, the zip and
`SHA256SUMS.txt` to a GitHub release.

1. Bump `version` in `Cargo.toml` and commit.
2. Push a matching tag:

   ```sh
   git tag v0.1.0
   git push origin v0.1.0
   ```

3. The workflow publishes the release automatically. It can also be started manually from the
   Actions tab; the artifacts are then attached to the workflow run instead of a release.

To produce identical artifacts locally (PowerShell 5.1 or later):

```powershell
./scripts/package.ps1
```

Everything lands in `dist\`.

## Tray menu

```
Display 1: Generic PnP Monitor — 70%
    ✓ 0%
      10%
      20%
      30%
      40%
      50%
      60%
    ✓ 70%
      80%
      90%
      100%
--------------------------------
Refresh monitors
Restore saved brightness
--------------------------------
Quit
```

- Clicking a value applies it immediately. If the previous value was different, only the new target
  is sent to the hardware; clicking the already checked value just re-syncs the checkmark.
- The checkmark moves right away (optimistic update) and is corrected if the panel rejects the
  write or reports a different result.
- Values are disabled while the brightness is unknown (DDC/CI disabled, panel asleep, unsupported
  feature). `0%` is allowed on purpose: it blanks the backlight, so make sure the monitor's own
  buttons can bring it back.
- `Refresh monitors` drops all handles, re-enumerates displays and re-reads values. Use it after
  hot-plugging a display or resuming from sleep.
- Double-clicking the tray icon also triggers a refresh.
- `Quit` removes the tray icon cleanly.

## Diagnostics

```sh
target\debug\designer.exe --dump
```

`--dump` prints every detected display with the backend name and the raw brightness value. In a
release build stdout is not attached to a console, so the same text is written to the log file; use
a debug build (`cargo run -- --dump`) to see it directly.

## Configuration

The file is created on first launch (with defaults) at:

```
%APPDATA%\designer\config.json
```

```json
{
  "version": 2,
  "min_interval_ms": 50,
  "restore_on_start": false,
  "monitors": [
    {
      "index": 0,
      "description": "Generic PnP Monitor",
      "brightness": 70
    }
  ]
}
```

| Field | Default | Meaning |
| --- | --- | --- |
| `version` | `2` | Schema version, rewritten automatically. |
| `min_interval_ms` | `50` | Minimum delay between DDC/CI commands per monitor. Some drivers need more; allowed range is 20-1000. |
| `restore_on_start` | `false` | Re-apply saved brightness shortly after startup. |
| `monitors` | `[]` | Last confirmed per-monitor brightness. |

Notes:

- Schema version 2 removed contrast tracking and the `step_percent`/`min_percent` settings.
  Version 1 files still load: unknown keys are ignored, missing keys fall back to defaults, and the
  version is rewritten on the next save.
- The file is written atomically (temporary file plus rename). A corrupt file is moved to
  `config.json.bak` and defaults are used.
- Brightness values are clamped to 0-100 and `min_interval_ms` to 20-1000 on load.
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

**Brightness shows `unavailable` and I2C errors appear in the log**

The monitor did not answer a DDC/CI request. Check, in this order:

1. DDC/CI is enabled in the monitor's OSD menu.
2. The monitor is fully awake and on the same input Windows displays.
3. No KVM, capture card or passive adapter sits between the GPU and the panel.
4. Try another cable or port; some USB-C docks and DisplayLink adapters do not pass DDC through.

**Only one display is listed while two are attached**

Mirrored displays share one physical monitor and are enumerated once. After changing the display
configuration, use `Refresh monitors`.

**The checkmark jumps back after a click**

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
| `src/monitor/controller.rs` | The only module touching `ddc-hi`; enumeration, brightness VCP 0x10 reads/writes, throttle, backoff. |
| `src/monitor/scale.rs` | Percentage to raw VCP conversion and back. |
| `src/config.rs` | JSON persistence, defaults, atomic save, corruption recovery. |
| `src/tray/` | Tray icon lifecycle, checkmark menu construction, generated icon. |
| `src/error.rs`, `src/logging.rs` | Shared error types and file logging. |

Design points:

- Display handles are created, used and destroyed on the worker thread; only plain data crosses
  thread boundaries.
- Rapid clicks for the same monitor are collapsed into one final write, followed by one verification
  read when the queue is idle.
- A 50 ms (configurable) gap between commands per monitor avoids driver-level DDC failures.
- After three consecutive read failures a monitor's reads are paused for 20 seconds to avoid
  hammering a disabled or sleeping panel; a successful write clears the backoff.
- On write failure the UI reverts the checkmark to the last confirmed value.

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

- Brightness only; contrast control was removed from the menu.
- No global hotkeys, presets or settings window (deliberately lean scope).
- Internal laptop panels usually cannot be controlled via DDC/CI.
- Identical monitors with duplicate descriptions may not restore values reliably after being
  reordered.

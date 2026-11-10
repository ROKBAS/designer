//! DDC/CI controller built on `ddc-hi`.
//!
//! All hardware interaction is confined to this module. Handles are created
//! and dropped on the worker thread that owns the controller, so no display
//! handle ever crosses a thread boundary.

use std::thread;
use std::time::{Duration, Instant};

use ddc_hi::{Ddc, Display};
use log::{debug, info, warn};
use thiserror::Error;

use super::scale;

/// VESA MCCS VCP code for brightness.
pub const BRIGHTNESS_VCP: u8 = 0x10;

/// Errors produced while talking to a monitor.
#[derive(Debug, Error)]
pub enum MonitorError {
    #[error("monitor {index} is no longer available; refresh the monitor list")]
    NotFound { index: u32 },

    #[error("DDC/CI command failed for monitor {index}: {message}")]
    Command { index: u32, message: String },

    #[error("monitor {index} reports no usable brightness range")]
    NoRange { index: u32 },
}

/// Plain-data view of a monitor, safe to send to the UI thread.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonitorSnapshot {
    /// Enumeration index, used as the menu action identifier.
    pub index: u32,
    /// Human readable description from the backend.
    pub description: String,
    /// Backend name, for diagnostics.
    pub backend: String,
    /// Current brightness in percent, `None` when unreadable.
    pub brightness: Option<u8>,
    /// Raw maximum reported for brightness, when known.
    pub brightness_max: Option<u16>,
}

struct Entry {
    display: Display,
    index: u32,
    description: String,
    backend: String,
    last_command: Option<Instant>,
    failures: u32,
    cooldown_until: Option<Instant>,
    /// Cached raw maximum reported for brightness.
    maximum: Option<u16>,
}

/// Owns DDC/CI display handles and serialises every VCP command.
pub struct MonitorController {
    entries: Vec<Entry>,
    min_interval: Duration,
}

impl MonitorController {
    /// Create an empty controller. Call [`MonitorController::refresh`] to
    /// enumerate displays; this is deliberately explicit so the worker thread
    /// performs enumeration.
    pub fn new(min_interval: Duration) -> Self {
        Self {
            entries: Vec::new(),
            min_interval: min_interval.max(Duration::from_millis(20)),
        }
    }

    /// Drop all handles and enumerate displays again. Returns the number of
    /// physical monitors found.
    pub fn refresh(&mut self) -> usize {
        self.entries.clear();
        let displays = Display::enumerate();
        info!("DDC/CI enumeration found {} display(s)", displays.len());
        for (position, display) in displays.into_iter().enumerate() {
            let index = position as u32;
            let description = describe(&display, index);
            let backend = display.info.backend.to_string();
            debug!("display {index}: {description} ({backend})");
            self.entries.push(Entry {
                display,
                index,
                description,
                backend,
                last_command: None,
                failures: 0,
                cooldown_until: None,
                maximum: None,
            });
        }
        self.entries.len()
    }

    /// Read the current brightness in percent. `Ok(None)` means the monitor
    /// reports no usable range.
    pub fn read(&mut self, index: u32) -> Result<Option<u8>, MonitorError> {
        let position = self.position(index)?;

        if let Some(until) = self.entries[position].cooldown_until
            && Instant::now() < until
        {
            return Err(MonitorError::Command {
                index,
                message:
                    "reads paused after repeated DDC/CI failures; use Refresh monitors to retry"
                        .to_owned(),
            });
        }

        match self.read_feature(position) {
            Ok((maximum, current)) => {
                self.entries[position].maximum = Some(maximum);
                self.note_success(position);
                Ok(scale::raw_to_percent(current, maximum))
            }
            Err(error) => {
                self.note_failure(position);
                Err(error)
            }
        }
    }

    /// Write brightness as a percentage, scaled to the monitor's raw range.
    pub fn set(&mut self, index: u32, percent: u8) -> Result<(), MonitorError> {
        let position = self.position(index)?;
        let maximum = self.maximum(position)?;
        let raw = scale::percent_to_raw(percent, maximum);

        self.wait_for_slot(position);
        let result = self.entries[position]
            .display
            .handle
            .set_vcp_feature(BRIGHTNESS_VCP, raw);

        match result {
            Ok(()) => {
                self.note_success(position);
                debug!("monitor {index}: set brightness to {percent}% (raw {raw}/{maximum})");
                Ok(())
            }
            Err(error) => {
                self.note_failure(position);
                Err(MonitorError::Command {
                    index,
                    message: format!("{error:#}"),
                })
            }
        }
    }

    /// Read brightness for every monitor, isolating per-monitor failures.
    pub fn snapshot(&mut self) -> Vec<MonitorSnapshot> {
        let mut snapshots = Vec::with_capacity(self.entries.len());

        for position in 0..self.entries.len() {
            let index = self.entries[position].index;
            let description = self.entries[position].description.clone();
            let backend = self.entries[position].backend.clone();

            let brightness = match self.read(index) {
                Ok(value) => value,
                Err(error) => {
                    debug!("monitor {index}: brightness unavailable: {error}");
                    None
                }
            };

            snapshots.push(MonitorSnapshot {
                index,
                description,
                backend,
                brightness,
                brightness_max: self.entries[position].maximum,
            });
        }

        snapshots
    }

    /// Human readable diagnostic dump including the raw VCP value.
    ///
    /// Used by the `--dump` command line switch. It bypasses the failure
    /// backoff so repeated diagnostics always attempt every read.
    pub fn dump(&mut self) -> String {
        let mut output = String::new();
        output.push_str(&format!(
            "DDC/CI controller: {} monitor(s)\n",
            self.entries.len()
        ));

        if self.entries.is_empty() {
            output.push_str("No displays reported by the backend.\n");
        }

        for position in 0..self.entries.len() {
            let index = self.entries[position].index;
            let description = self.entries[position].description.clone();
            let backend = self.entries[position].backend.clone();
            output.push_str(&format!(
                "Display {} [{backend}] {description}\n",
                index + 1
            ));

            match self.read_feature(position) {
                Ok((maximum, current)) => {
                    let percent = scale::raw_to_percent(current, maximum)
                        .map(|value| format!("{value}%"))
                        .unwrap_or_else(|| "unavailable".to_owned());
                    output.push_str(&format!(
                        "  Brightness: raw {current}/{maximum} ({percent})\n"
                    ));
                }
                Err(error) => output.push_str(&format!("  Brightness: error: {error}\n")),
            }
        }

        output
    }

    fn position(&self, index: u32) -> Result<usize, MonitorError> {
        self.entries
            .iter()
            .position(|entry| entry.index == index)
            .ok_or(MonitorError::NotFound { index })
    }

    /// Enforce the per-monitor command interval.
    fn wait_for_slot(&mut self, position: usize) {
        if let Some(last) = self.entries[position].last_command {
            let elapsed = last.elapsed();
            if elapsed < self.min_interval {
                thread::sleep(self.min_interval - elapsed);
            }
        }
        self.entries[position].last_command = Some(Instant::now());
    }

    fn read_feature(&mut self, position: usize) -> Result<(u16, u16), MonitorError> {
        self.wait_for_slot(position);
        let index = self.entries[position].index;

        let value = self.entries[position]
            .display
            .handle
            .get_vcp_feature(BRIGHTNESS_VCP)
            .map_err(|error| MonitorError::Command {
                index,
                message: format!("{error:#}"),
            })?;

        let maximum = u16::from_be_bytes([value.mh, value.ml]);
        let current = u16::from_be_bytes([value.sh, value.sl]);
        Ok((maximum, current))
    }

    fn maximum(&mut self, position: usize) -> Result<u16, MonitorError> {
        if let Some(maximum) = self.entries[position].maximum
            && maximum > 0
        {
            return Ok(maximum);
        }

        let (maximum, _) = self.read_feature(position)?;
        self.entries[position].maximum = Some(maximum);

        if maximum == 0 {
            let index = self.entries[position].index;
            return Err(MonitorError::NoRange { index });
        }

        Ok(maximum)
    }

    fn note_success(&mut self, position: usize) {
        let entry = &mut self.entries[position];
        entry.failures = 0;
        entry.cooldown_until = None;
    }

    fn note_failure(&mut self, position: usize) {
        let entry = &mut self.entries[position];
        entry.failures += 1;
        if entry.failures >= 3 {
            entry.cooldown_until = Some(Instant::now() + Duration::from_secs(20));
            warn!(
                "monitor {}: pausing reads for 20s after {} consecutive DDC/CI failures",
                entry.index, entry.failures
            );
        }
    }
}

fn describe(display: &Display, index: u32) -> String {
    if let Some(model) = display.info.model_name.as_ref() {
        let model = model.trim();
        if !model.is_empty() {
            return model.to_owned();
        }
    }

    let id = display.info.id.trim();
    if !id.is_empty() {
        return id.to_owned();
    }

    format!("Display {}", index + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exercises the real DDC/CI write path without changing what the user
    /// sees: it reads the current brightness and writes the same percentage
    /// back. Run explicitly with:
    ///
    /// `cargo test hardware_write_roundtrip -- --ignored --nocapture`
    #[test]
    #[ignore = "requires a real DDC/CI capable monitor"]
    fn hardware_write_roundtrip() {
        let mut controller = MonitorController::new(Duration::from_millis(50));
        let found = controller.refresh();
        assert!(found > 0, "no displays were enumerated");

        let snapshot = controller.snapshot();
        let monitor = snapshot
            .iter()
            .find(|monitor| monitor.brightness.is_some())
            .expect("no monitor reported a readable brightness");

        let percent = monitor.brightness.expect("checked above");
        controller
            .set(monitor.index, percent)
            .expect("writing the current brightness back must succeed");

        let confirmed = controller
            .read(monitor.index)
            .expect("read after write")
            .expect("range available");
        assert_eq!(
            confirmed, percent,
            "brightness should remain unchanged after writing the same value"
        );
    }
}

//! Tray menu construction and pure formatting helpers.

use tray_icon::menu::{CheckMenuItem, Menu, MenuId, MenuItem, PredefinedMenuItem, Submenu};

use crate::monitor::MonitorSnapshot;

/// Percentage step between consecutive brightness entries.
pub const VALUE_STEP: u8 = 10;

/// Brightness values offered by the menu: 0, 10, ..., 100.
pub fn brightness_values() -> Vec<u8> {
    (0..=100).step_by(usize::from(VALUE_STEP)).collect()
}

/// A resolved menu action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Refresh,
    Restore,
    Quit,
    SetBrightness { index: u32, percent: u8 },
}

/// Handles for one monitor's submenu and its brightness entries.
pub struct MonitorMenu {
    pub index: u32,
    pub submenu: Submenu,
    /// Percentage paired with the check item that selects it.
    pub values: Vec<(u8, CheckMenuItem)>,
}

/// Handles for all menu items, kept so labels and actions can be resolved
/// after the [`Menu`] itself has been handed to the tray icon.
pub struct MenuHandles {
    pub monitors: Vec<MonitorMenu>,
    pub refresh: MenuItem,
    pub restore: MenuItem,
    pub quit: MenuItem,
}

/// A built native menu plus the handles needed to update it later.
pub struct BuiltMenu {
    pub menu: Menu,
    pub handles: MenuHandles,
}

/// Build the tray menu for the given monitors.
///
/// Each monitor gets a submenu holding a flat list of brightness values; the
/// value matching the current brightness carries a checkmark. `reading` is
/// used for the initial placeholder menu while enumeration runs.
pub fn build(monitors: &[MonitorSnapshot], reading: bool) -> BuiltMenu {
    let menu = Menu::new();
    let mut entries = Vec::with_capacity(monitors.len());

    for (position, monitor) in monitors.iter().enumerate() {
        let submenu = Submenu::new(monitor_title(monitor, position, reading), true);
        let available = monitor.brightness.is_some();
        let mut values = Vec::with_capacity(brightness_values().len());

        for percent in brightness_values() {
            let item = CheckMenuItem::new(
                format!("{percent}%"),
                available,
                monitor.brightness == Some(percent),
                None,
            );
            submenu.append(&item).expect("append brightness value");
            values.push((percent, item));
        }

        menu.append(&submenu).expect("append monitor submenu");

        entries.push(MonitorMenu {
            index: monitor.index,
            submenu,
            values,
        });
    }

    if monitors.is_empty() {
        let placeholder = MenuItem::new(
            if reading {
                "Detecting monitors..."
            } else {
                "No DDC/CI monitors found"
            },
            false,
            None,
        );
        menu.append(&placeholder).expect("append placeholder");
    }

    menu.append(&PredefinedMenuItem::separator())
        .expect("append menu separator");
    let refresh = MenuItem::new("Refresh monitors", true, None);
    let restore = MenuItem::new("Restore saved brightness", !monitors.is_empty(), None);
    menu.append_items(&[&refresh, &restore])
        .expect("append actions");
    menu.append(&PredefinedMenuItem::separator())
        .expect("append menu separator");
    let quit = MenuItem::new("Quit", true, None);
    menu.append(&quit).expect("append quit");

    BuiltMenu {
        menu,
        handles: MenuHandles {
            monitors: entries,
            refresh,
            restore,
            quit,
        },
    }
}

/// Map a menu event id back to an action.
pub fn resolve(handles: &MenuHandles, id: &MenuId) -> Option<Action> {
    if handles.refresh.id() == id {
        return Some(Action::Refresh);
    }
    if handles.restore.id() == id {
        return Some(Action::Restore);
    }
    if handles.quit.id() == id {
        return Some(Action::Quit);
    }

    for monitor in &handles.monitors {
        for (percent, item) in &monitor.values {
            if item.id() == id {
                return Some(Action::SetBrightness {
                    index: monitor.index,
                    percent: *percent,
                });
            }
        }
    }

    None
}

/// Title of a monitor submenu, including its current brightness.
pub fn monitor_title(monitor: &MonitorSnapshot, position: usize, reading: bool) -> String {
    let description = monitor.description.trim();
    let base = if description.is_empty() {
        format!("Display {}", position + 1)
    } else {
        format!("Display {}: {description}", position + 1)
    };

    match monitor.brightness {
        Some(value) => format!("{base} — {value}%"),
        None if reading => format!("{base} — reading..."),
        None => format!("{base} — unavailable"),
    }
}

/// Short tooltip summarising every monitor's brightness.
pub fn tooltip_text(monitors: &[MonitorSnapshot]) -> String {
    if monitors.is_empty() {
        return "DDC/CI brightness: no monitors detected".to_owned();
    }

    let parts: Vec<String> = monitors
        .iter()
        .enumerate()
        .map(|(position, monitor)| {
            let value = monitor
                .brightness
                .map(|value| format!("{value}%"))
                .unwrap_or_else(|| "n/a".to_owned());
            format!("{}: {value}", position + 1)
        })
        .collect();

    format!("Brightness {}", parts.join(" | "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(index: u32, brightness: Option<u8>) -> MonitorSnapshot {
        MonitorSnapshot {
            index,
            description: "Test Panel".to_owned(),
            backend: "test".to_owned(),
            brightness,
            brightness_max: Some(100),
        }
    }

    #[test]
    fn brightness_values_cover_zero_to_hundred_in_tens() {
        assert_eq!(
            brightness_values(),
            vec![0, 10, 20, 30, 40, 50, 60, 70, 80, 90, 100]
        );
    }

    #[test]
    fn monitor_title_shows_current_value() {
        let monitor = snapshot(0, Some(70));
        assert_eq!(
            monitor_title(&monitor, 0, false),
            "Display 1: Test Panel — 70%"
        );

        let unavailable = snapshot(0, None);
        assert_eq!(
            monitor_title(&unavailable, 0, true),
            "Display 1: Test Panel — reading..."
        );
        assert_eq!(
            monitor_title(&unavailable, 0, false),
            "Display 1: Test Panel — unavailable"
        );

        let mut unnamed = snapshot(1, Some(0));
        unnamed.description.clear();
        assert_eq!(monitor_title(&unnamed, 1, false), "Display 2 — 0%");
    }

    #[test]
    fn tooltip_handles_empty_and_multiple_monitors() {
        assert_eq!(tooltip_text(&[]), "DDC/CI brightness: no monitors detected");

        let monitors = vec![snapshot(0, Some(45)), snapshot(1, None)];
        assert_eq!(tooltip_text(&monitors), "Brightness 1: 45% | 2: n/a");
    }
}

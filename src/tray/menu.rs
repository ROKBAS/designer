//! Tray menu construction and pure formatting helpers.

use tray_icon::menu::{Menu, MenuId, MenuItem, PredefinedMenuItem, Submenu};

use crate::monitor::{Field, MonitorSnapshot};

/// A resolved menu action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Refresh,
    Restore,
    Quit,
    Adjust {
        index: u32,
        field: Field,
        delta: i16,
    },
}

/// Handles for one monitor's submenu items.
pub struct MonitorMenu {
    pub index: u32,
    pub brightness_label: MenuItem,
    pub brightness_up: MenuItem,
    pub brightness_down: MenuItem,
    pub contrast_label: MenuItem,
    pub contrast_up: MenuItem,
    pub contrast_down: MenuItem,
    pub step: u8,
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
pub fn build(monitors: &[MonitorSnapshot], step: u8, reading: bool) -> BuiltMenu {
    let menu = Menu::new();
    let mut entries = Vec::with_capacity(monitors.len());

    for (position, monitor) in monitors.iter().enumerate() {
        let submenu = Submenu::new(monitor_title(monitor, position), true);

        let brightness_label =
            MenuItem::new(brightness_text(monitor.brightness, reading), false, None);
        let brightness_up = MenuItem::new(
            format!("Brightness +{step}%"),
            monitor.brightness.is_some(),
            None,
        );
        let brightness_down = MenuItem::new(
            format!("Brightness -{step}%"),
            monitor.brightness.is_some(),
            None,
        );
        let contrast_label = MenuItem::new(contrast_text(monitor.contrast, reading), false, None);
        let contrast_up = MenuItem::new(
            format!("Contrast +{step}%"),
            monitor.contrast.is_some(),
            None,
        );
        let contrast_down = MenuItem::new(
            format!("Contrast -{step}%"),
            monitor.contrast.is_some(),
            None,
        );

        submenu
            .append_items(&[&brightness_label, &brightness_up, &brightness_down])
            .expect("append brightness items");
        submenu
            .append(&PredefinedMenuItem::separator())
            .expect("append monitor separator");
        submenu
            .append_items(&[&contrast_label, &contrast_up, &contrast_down])
            .expect("append contrast items");
        menu.append(&submenu).expect("append monitor submenu");

        entries.push(MonitorMenu {
            index: monitor.index,
            brightness_label,
            brightness_up,
            brightness_down,
            contrast_label,
            contrast_up,
            contrast_down,
            step,
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
        let delta = i16::from(monitor.step);
        if monitor.brightness_up.id() == id {
            return Some(Action::Adjust {
                index: monitor.index,
                field: Field::Brightness,
                delta,
            });
        }
        if monitor.brightness_down.id() == id {
            return Some(Action::Adjust {
                index: monitor.index,
                field: Field::Brightness,
                delta: -delta,
            });
        }
        if monitor.contrast_up.id() == id {
            return Some(Action::Adjust {
                index: monitor.index,
                field: Field::Contrast,
                delta,
            });
        }
        if monitor.contrast_down.id() == id {
            return Some(Action::Adjust {
                index: monitor.index,
                field: Field::Contrast,
                delta: -delta,
            });
        }
    }

    None
}

/// Text for the brightness status item.
pub fn brightness_text(value: Option<u8>, reading: bool) -> String {
    field_text("Brightness", value, reading)
}

/// Text for the contrast status item.
pub fn contrast_text(value: Option<u8>, reading: bool) -> String {
    field_text("Contrast", value, reading)
}

fn field_text(name: &str, value: Option<u8>, reading: bool) -> String {
    match (value, reading) {
        (Some(value), _) => format!("{name}: {value}%"),
        (None, true) => format!("{name}: reading..."),
        (None, false) => format!("{name}: unavailable"),
    }
}

/// Title of a monitor submenu.
pub fn monitor_title(monitor: &MonitorSnapshot, position: usize) -> String {
    let description = monitor.description.trim();
    if description.is_empty() {
        format!("Display {}", position + 1)
    } else {
        format!("Display {}: {description}", position + 1)
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

    fn snapshot(index: u32, brightness: Option<u8>, contrast: Option<u8>) -> MonitorSnapshot {
        MonitorSnapshot {
            index,
            description: "Test Panel".to_owned(),
            backend: "test".to_owned(),
            brightness,
            brightness_max: Some(100),
            contrast,
            contrast_max: Some(100),
        }
    }

    #[test]
    fn field_text_formats_all_states() {
        assert_eq!(brightness_text(Some(45), false), "Brightness: 45%");
        assert_eq!(brightness_text(Some(100), true), "Brightness: 100%");
        assert_eq!(brightness_text(None, true), "Brightness: reading...");
        assert_eq!(brightness_text(None, false), "Brightness: unavailable");
        assert_eq!(contrast_text(Some(60), false), "Contrast: 60%");
    }

    #[test]
    fn monitor_title_is_numbered_and_falls_back() {
        let mut monitor = snapshot(0, Some(50), Some(50));
        assert_eq!(monitor_title(&monitor, 0), "Display 1: Test Panel");
        monitor.description.clear();
        assert_eq!(monitor_title(&monitor, 1), "Display 2");
    }

    #[test]
    fn tooltip_handles_empty_and_multiple_monitors() {
        assert_eq!(tooltip_text(&[]), "DDC/CI brightness: no monitors detected");

        let monitors = vec![snapshot(0, Some(45), Some(60)), snapshot(1, None, Some(10))];
        assert_eq!(tooltip_text(&monitors), "Brightness 1: 45% | 2: n/a");
    }
}

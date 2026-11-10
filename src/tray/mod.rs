//! Tray icon lifecycle: creation, menu rebuilds, checkmarks and tooltip.

pub mod icon;
pub mod menu;

use log::debug;
use tray_icon::menu::MenuId;
use tray_icon::{TrayIcon, TrayIconBuilder};

use crate::error::AppResult;
use crate::monitor::MonitorSnapshot;
use menu::{Action, MenuHandles};

const BASE_TOOLTIP: &str = "DDC/CI monitor brightness";

/// Owns the tray icon and the handles needed to update its menu.
pub struct Tray {
    icon: TrayIcon,
    handles: MenuHandles,
    monitors: Vec<MonitorSnapshot>,
    /// False until the first monitor snapshot arrives, so the initial
    /// "Detecting monitors..." placeholder is always replaced even when no
    /// displays were found.
    initialized: bool,
}

impl Tray {
    /// Create the tray icon with a placeholder menu while monitors are being
    /// detected.
    pub fn new() -> AppResult<Self> {
        let built = menu::build(&[], true);
        let icon = TrayIconBuilder::new()
            .with_icon(icon::app_icon()?)
            .with_tooltip(BASE_TOOLTIP)
            .with_menu(Box::new(built.menu))
            .build()?;

        Ok(Self {
            icon,
            handles: built.handles,
            monitors: Vec::new(),
            initialized: false,
        })
    }

    /// Current displayed brightness, including optimistic updates.
    pub fn value(&self, index: u32) -> Option<u8> {
        self.monitors
            .iter()
            .find(|monitor| monitor.index == index)
            .and_then(|monitor| monitor.brightness)
    }

    /// Resolve a native menu event id to an action.
    pub fn resolve(&self, id: &MenuId) -> Option<Action> {
        menu::resolve(&self.handles, id)
    }

    /// Replace the monitor list. The menu is rebuilt only when the set of
    /// monitors changed; otherwise checkmarks and titles are updated in place.
    pub fn set_monitors(&mut self, monitors: Vec<MonitorSnapshot>) {
        let rebuild = !self.initialized || signatures(&self.monitors) != signatures(&monitors);
        self.initialized = true;
        self.monitors = monitors;

        if rebuild {
            let built = menu::build(&self.monitors, false);
            self.icon.set_menu(Some(Box::new(built.menu)));
            self.handles = built.handles;
        } else {
            self.refresh_checks();
        }

        self.refresh_tooltip();
    }

    /// Update one monitor's brightness optimistically (on click) or with a
    /// confirmed value.
    pub fn set_brightness(&mut self, index: u32, percent: Option<u8>) {
        if let Some(monitor) = self.monitors.iter_mut().find(|m| m.index == index) {
            monitor.brightness = percent;
        }
        self.refresh_checks();
        self.refresh_tooltip();
    }

    fn refresh_checks(&self) {
        for (position, monitor) in self.monitors.iter().enumerate() {
            let Some(entry) = self
                .handles
                .monitors
                .iter()
                .find(|entry| entry.index == monitor.index)
            else {
                continue;
            };

            entry
                .submenu
                .set_text(menu::monitor_title(monitor, position, false));

            let available = monitor.brightness.is_some();
            for (percent, item) in &entry.values {
                item.set_checked(monitor.brightness == Some(*percent));
                item.set_enabled(available);
            }
        }
    }

    fn refresh_tooltip(&self) {
        let text = menu::tooltip_text(&self.monitors);
        if let Err(error) = self.icon.set_tooltip(Some(text)) {
            debug!("failed to update tray tooltip: {error}");
        }
    }
}

fn signatures(monitors: &[MonitorSnapshot]) -> Vec<(u32, &str)> {
    monitors
        .iter()
        .map(|monitor| (monitor.index, monitor.description.as_str()))
        .collect()
}

//! Application wiring: tao event loop, tray menu dispatch, worker events.

use std::sync::Arc;
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

use log::{debug, info, warn};
use tao::event::Event;
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::TrayIconEvent;
use tray_icon::menu::{MenuEvent, MenuId};

use crate::config::Config;
use crate::error::{AppError, AppResult};
use crate::monitor::MonitorSnapshot;
use crate::tray::Tray;
use crate::tray::menu::Action;
use crate::worker::{self, Command, EventSink, WorkerEvent};

/// Events delivered to the main (UI) thread.
#[derive(Debug)]
pub enum UserEvent {
    Menu(MenuId),
    Tray(TrayIconEvent),
    Worker(WorkerEvent),
}

/// Build the event loop, tray icon and worker, then run until the user quits.
pub fn run() -> AppResult<()> {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();

    // Forward native menu and tray callbacks into the event loop so the
    // closure below wakes up immediately on interaction.
    let menu_proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let _ = menu_proxy.send_event(UserEvent::Menu(event.id));
    }));
    let tray_proxy = event_loop.create_proxy();
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        let _ = tray_proxy.send_event(UserEvent::Tray(event));
    }));

    let config = Config::load();
    // Write defaults on first launch so the file exists and can be edited.
    if !Config::path().exists()
        && let Err(error) = config.save()
    {
        warn!("could not write the initial configuration: {error}");
    }
    let tray = Tray::new()?;

    let (command_tx, command_rx) = mpsc::channel();
    let worker_proxy = event_loop.create_proxy();
    let sink: EventSink = Arc::new(move |event| {
        let _ = worker_proxy.send_event(UserEvent::Worker(event));
    });
    worker::spawn_worker(
        command_rx,
        sink,
        Duration::from_millis(config.min_interval_ms),
    )
    .map_err(|error| AppError::Worker(error.to_string()))?;

    // Kick off monitor detection before entering the loop.
    let _ = command_tx.send(Command::Refresh);

    let mut app = App {
        tray: Some(tray),
        config,
        commands: command_tx,
        confirmed: Vec::new(),
        loaded_once: false,
        restored_once: false,
    };

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::UserEvent(UserEvent::Menu(id)) => {
                if app.handle_menu(&id) {
                    *control_flow = ControlFlow::Exit;
                }
            }
            Event::UserEvent(UserEvent::Worker(event)) => app.handle_worker(event),
            Event::UserEvent(UserEvent::Tray(TrayIconEvent::DoubleClick { .. })) => {
                let _ = app.commands.send(Command::Refresh);
            }
            Event::LoopDestroyed => {
                // Dropping the tray icon removes it from the notification area.
                app.tray = None;
            }
            _ => {}
        }
    });
}

struct App {
    tray: Option<Tray>,
    config: Config,
    commands: Sender<Command>,
    /// Last values confirmed by the hardware.
    confirmed: Vec<MonitorSnapshot>,
    /// Whether the first non-empty monitor list has been received.
    loaded_once: bool,
    /// Whether `restore_on_start` already ran this session.
    restored_once: bool,
}

impl App {
    /// Handle a menu click. Returns `true` when the event loop should exit.
    fn handle_menu(&mut self, id: &MenuId) -> bool {
        let Some(action) = self.tray.as_ref().and_then(|tray| tray.resolve(id)) else {
            return false;
        };

        match action {
            Action::Quit => {
                self.shutdown();
                true
            }
            Action::Refresh => {
                let _ = self.commands.send(Command::Refresh);
                false
            }
            Action::Restore => {
                self.restore_saved();
                false
            }
            Action::SetBrightness { index, percent } => {
                let Some(current) = self.tray.as_ref().and_then(|tray| tray.value(index)) else {
                    return false;
                };

                // Normalise the checkmarks even when the value did not change,
                // so a stale checkmark cannot survive the click.
                if let Some(tray) = self.tray.as_mut() {
                    tray.set_brightness(index, Some(percent));
                }

                if current != percent {
                    let _ = self
                        .commands
                        .send(Command::SetBrightness { index, percent });
                }
                false
            }
        }
    }

    fn handle_worker(&mut self, event: WorkerEvent) {
        match event {
            WorkerEvent::Monitors { monitors, warning } => {
                if let Some(warning) = warning {
                    warn!("{warning}");
                }

                self.confirmed = monitors.clone();
                if let Some(tray) = self.tray.as_mut() {
                    tray.set_monitors(monitors.clone());
                }

                // Persist whatever the hardware actually reports.
                let mut changed = false;
                for monitor in &monitors {
                    if let Some(brightness) = monitor.brightness {
                        changed |= self.config.update_monitor(
                            monitor.index,
                            &monitor.description,
                            brightness,
                        );
                    }
                }
                if changed {
                    self.persist();
                }

                let first_load = !self.loaded_once && !monitors.is_empty();
                if first_load {
                    self.loaded_once = true;
                    if self.config.restore_on_start && !self.restored_once {
                        self.restore_saved();
                    }
                }
            }
            WorkerEvent::BrightnessConfirmed { index, percent } => {
                if let Some(tray) = self.tray.as_mut() {
                    tray.set_brightness(index, Some(percent));
                }

                let description = match self.confirmed.iter_mut().find(|m| m.index == index) {
                    Some(monitor) => {
                        monitor.brightness = Some(percent);
                        monitor.description.clone()
                    }
                    None => return,
                };

                if self.config.update_monitor(index, &description, percent) {
                    self.persist();
                }
            }
            WorkerEvent::WriteFailed { index, message } => {
                warn!(
                    "monitor {}: writing brightness failed: {message}",
                    index + 1
                );
                // Revert the optimistic checkmark to the last confirmed value.
                let fallback = self
                    .confirmed
                    .iter()
                    .find(|monitor| monitor.index == index)
                    .and_then(|monitor| monitor.brightness);
                if let Some(tray) = self.tray.as_mut() {
                    tray.set_brightness(index, fallback);
                }
            }
            WorkerEvent::Info(message) => info!("{message}"),
        }
    }

    /// Re-apply persisted brightness to every monitor currently connected.
    fn restore_saved(&mut self) {
        self.restored_once = true;

        let config = &self.config;
        let targets: Vec<(u32, u8)> = self
            .confirmed
            .iter()
            .filter_map(|monitor| {
                let saved = config.saved_for(monitor.index, &monitor.description)?;
                let percent = saved.brightness?;
                (monitor.brightness != Some(percent)).then_some((monitor.index, percent))
            })
            .collect();

        if targets.is_empty() {
            debug!("nothing to restore");
            return;
        }

        for (index, percent) in targets {
            if let Some(tray) = self.tray.as_mut() {
                tray.set_brightness(index, Some(percent));
            }
            let _ = self
                .commands
                .send(Command::SetBrightness { index, percent });
        }
    }

    fn persist(&self) {
        if let Err(error) = self.config.save() {
            warn!("failed to save configuration: {error}");
        }
    }

    fn shutdown(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        // Drop before the loop exits so the notification area is cleaned up.
        if let Some(tray) = self.tray.take() {
            drop(tray);
        }
        debug!("shutdown requested");
    }
}

//! Worker thread that owns the DDC/CI controller and serialises requests.
//!
//! The UI thread never blocks on hardware. It sends [`Command`]s over a
//! channel; the worker replies with [`WorkerEvent`]s through an [`EventSink`]
//! which the application wires to the tao event loop proxy.

use std::io;
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use log::debug;

use crate::monitor::{Field, MonitorController, MonitorError, MonitorSnapshot};

/// Requests sent from the UI thread to the worker.
#[derive(Clone, Debug)]
pub enum Command {
    /// Drop handles, re-enumerate displays, then read every field.
    Refresh,
    /// Set a field to an absolute percentage.
    Set {
        index: u32,
        field: Field,
        percent: u8,
    },
    /// Stop the worker loop.
    Shutdown,
}

/// Replies sent from the worker to the UI thread.
#[derive(Clone, Debug)]
pub enum WorkerEvent {
    /// Full monitor list after enumeration or a resync.
    Monitors {
        monitors: Vec<MonitorSnapshot>,
        warning: Option<String>,
    },
    /// A write succeeded and was verified (or fall back to the requested value
    /// when verification itself failed).
    ValueConfirmed {
        index: u32,
        field: Field,
        percent: Option<u8>,
    },
    /// A write failed; the UI should revert its optimistic label.
    WriteFailed {
        index: u32,
        field: Field,
        message: String,
    },
    /// Informational message worth logging, such as a failed verification.
    Info(String),
}

/// Thread-safe callback used to deliver worker events.
pub type EventSink = Arc<dyn Fn(WorkerEvent) + Send + Sync + 'static>;

/// Hardware-facing operations the worker needs. Implemented by
/// [`MonitorController`] and by a mock backend in tests.
pub trait DisplayBackend: 'static {
    fn refresh(&mut self) -> usize;
    fn snapshot(&mut self) -> Vec<MonitorSnapshot>;
    fn read(&mut self, index: u32, field: Field) -> Result<Option<u8>, MonitorError>;
    fn set(&mut self, index: u32, field: Field, percent: u8) -> Result<(), MonitorError>;
}

impl DisplayBackend for MonitorController {
    fn refresh(&mut self) -> usize {
        MonitorController::refresh(self)
    }

    fn snapshot(&mut self) -> Vec<MonitorSnapshot> {
        MonitorController::snapshot(self)
    }

    fn read(&mut self, index: u32, field: Field) -> Result<Option<u8>, MonitorError> {
        MonitorController::read(self, index, field)
    }

    fn set(&mut self, index: u32, field: Field, percent: u8) -> Result<(), MonitorError> {
        MonitorController::set(self, index, field, percent)
    }
}

/// Spawn the production worker owning a [`MonitorController`].
pub fn spawn_worker(
    command_rx: Receiver<Command>,
    sink: EventSink,
    min_interval: Duration,
) -> io::Result<JoinHandle<()>> {
    spawn_worker_with(command_rx, sink, move || {
        MonitorController::new(min_interval)
    })
}

/// Spawn a worker with a custom backend factory. The backend is constructed
/// inside the worker thread, so display handles never move between threads.
pub fn spawn_worker_with<F, B>(
    command_rx: Receiver<Command>,
    sink: EventSink,
    make_backend: F,
) -> io::Result<JoinHandle<()>>
where
    F: FnOnce() -> B + Send + 'static,
    B: DisplayBackend,
{
    thread::Builder::new()
        .name("ddc-worker".to_owned())
        .spawn(move || run(make_backend(), command_rx, sink))
}

fn run<B: DisplayBackend>(mut backend: B, rx: Receiver<Command>, sink: EventSink) {
    let mut pending: Option<Command> = None;

    loop {
        let first = match pending.take() {
            Some(command) => command,
            None => match rx.recv() {
                Ok(command) => command,
                Err(_) => break,
            },
        };

        // Drain everything already queued into one batch. Rapid menu clicks
        // arrive here as a burst and are collapsed below.
        let mut batch = vec![first];
        while let Ok(command) = rx.try_recv() {
            batch.push(command);
        }

        if batch
            .iter()
            .any(|command| matches!(command, Command::Shutdown))
        {
            debug!("worker shutting down");
            break;
        }

        let refresh = batch
            .iter()
            .any(|command| matches!(command, Command::Refresh));

        // Collapse repeated writes for the same field: only the final target
        // matters, so a burst of clicks costs one DDC command, not one per
        // click.
        let mut writes: Vec<(u32, Field, u8)> = Vec::new();
        for command in &batch {
            if let Command::Set {
                index,
                field,
                percent,
            } = *command
            {
                if let Some(slot) = writes.iter_mut().find(|(entry_index, entry_field, _)| {
                    *entry_index == index && *entry_field == field
                }) {
                    slot.2 = percent;
                } else {
                    writes.push((index, field, percent));
                }
            }
        }

        let mut warning = None;
        if refresh {
            let found = backend.refresh();
            if found == 0 {
                warning = Some(
                    "No DDC/CI capable monitors detected. Enable DDC/CI in the monitor OSD; \
                     see the log for details."
                        .to_owned(),
                );
            }
        }

        if refresh {
            let monitors = backend.snapshot();
            sink(WorkerEvent::Monitors { monitors, warning });
        }

        if writes.is_empty() {
            continue;
        }

        let mut dirty: Vec<(u32, Field, u8)> = Vec::new();
        for (index, field, percent) in writes {
            match backend.set(index, field, percent) {
                Ok(()) => dirty.push((index, field, percent)),
                Err(error) => sink(WorkerEvent::WriteFailed {
                    index,
                    field,
                    message: error.to_string(),
                }),
            }
        }

        if dirty.is_empty() {
            continue;
        }

        // Verify only once the queue is idle: during a click burst the UI
        // already shows the optimistic value and extra reads would only add
        // latency and DDC traffic.
        match rx.try_recv() {
            Ok(command) => pending = Some(command),
            Err(_) => {
                for (index, field, requested) in dirty {
                    let percent = match backend.read(index, field) {
                        Ok(Some(value)) => Some(value),
                        Ok(None) => Some(requested),
                        Err(error) => {
                            sink(WorkerEvent::Info(format!(
                                "could not verify {} on monitor {}: {error}",
                                field.name(),
                                index + 1
                            )));
                            Some(requested)
                        }
                    };
                    sink(WorkerEvent::ValueConfirmed {
                        index,
                        field,
                        percent,
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::{self, Sender};
    use std::sync::{Arc, Mutex};

    use super::*;

    #[derive(Default)]
    struct MockState {
        sets: Vec<(u32, Field, u8)>,
        reads: Vec<(u32, Field)>,
        monitors: Vec<MonitorSnapshot>,
        refresh_count: usize,
        value: Option<u8>,
        fail_sets: bool,
    }

    struct MockBackend {
        state: Arc<Mutex<MockState>>,
    }

    impl DisplayBackend for MockBackend {
        fn refresh(&mut self) -> usize {
            let mut state = self.state.lock().expect("lock");
            state.refresh_count += 1;
            state.monitors.len()
        }

        fn snapshot(&mut self) -> Vec<MonitorSnapshot> {
            self.state.lock().expect("lock").monitors.clone()
        }

        fn read(&mut self, index: u32, field: Field) -> Result<Option<u8>, MonitorError> {
            let mut state = self.state.lock().expect("lock");
            state.reads.push((index, field));
            Ok(state.value)
        }

        fn set(&mut self, index: u32, field: Field, percent: u8) -> Result<(), MonitorError> {
            let mut state = self.state.lock().expect("lock");
            state.sets.push((index, field, percent));
            if state.fail_sets {
                Err(MonitorError::Command {
                    index,
                    message: "mock failure".to_owned(),
                })
            } else {
                state.value = Some(percent);
                Ok(())
            }
        }
    }

    fn mock_state(fail_sets: bool) -> Arc<Mutex<MockState>> {
        Arc::new(Mutex::new(MockState {
            fail_sets,
            ..MockState::default()
        }))
    }

    fn sink_and_events() -> (EventSink, mpsc::Receiver<WorkerEvent>) {
        let (event_tx, event_rx) = mpsc::channel();
        let sink: EventSink = Arc::new(move |event| {
            let _ = event_tx.send(event);
        });
        (sink, event_rx)
    }

    fn start(
        commands: Vec<Command>,
        state: Arc<Mutex<MockState>>,
    ) -> (Sender<Command>, mpsc::Receiver<WorkerEvent>, JoinHandle<()>) {
        let (command_tx, command_rx) = mpsc::channel();
        // Preload commands so the worker sees them as one deterministic batch.
        for command in commands {
            command_tx.send(command).expect("queue command");
        }

        let (sink, event_rx) = sink_and_events();
        let backend_state = state;
        let handle = spawn_worker_with(command_rx, sink, move || MockBackend {
            state: backend_state,
        })
        .expect("spawn worker");

        (command_tx, event_rx, handle)
    }

    #[test]
    fn collapses_burst_of_writes_into_one_command() {
        let state = mock_state(false);
        let (commands, events, handle) = start(
            vec![
                Command::Set {
                    index: 0,
                    field: Field::Brightness,
                    percent: 45,
                },
                Command::Set {
                    index: 0,
                    field: Field::Brightness,
                    percent: 50,
                },
                Command::Set {
                    index: 0,
                    field: Field::Brightness,
                    percent: 55,
                },
            ],
            state.clone(),
        );

        let event = events
            .recv_timeout(Duration::from_secs(2))
            .expect("confirmation event");
        match event {
            WorkerEvent::ValueConfirmed {
                index,
                field,
                percent,
            } => {
                assert_eq!(index, 0);
                assert_eq!(field, Field::Brightness);
                assert_eq!(percent, Some(55));
            }
            other => panic!("unexpected event: {other:?}"),
        }

        {
            let state = state.lock().expect("lock");
            assert_eq!(state.sets, vec![(0, Field::Brightness, 55)]);
            assert_eq!(state.reads, vec![(0, Field::Brightness)]);
        }

        commands.send(Command::Shutdown).expect("shutdown");
        handle.join().expect("join worker");
    }

    #[test]
    fn refresh_without_monitors_reports_warning() {
        let state = mock_state(false);
        let (commands, events, handle) = start(vec![Command::Refresh], state);

        let event = events
            .recv_timeout(Duration::from_secs(2))
            .expect("monitors event");
        match event {
            WorkerEvent::Monitors { monitors, warning } => {
                assert!(monitors.is_empty());
                assert!(warning.is_some(), "expected a diagnostic warning");
            }
            other => panic!("unexpected event: {other:?}"),
        }

        commands.send(Command::Shutdown).expect("shutdown");
        handle.join().expect("join worker");
    }

    #[test]
    fn failed_write_is_reported_without_confirmation() {
        let state = mock_state(true);
        let (commands, events, handle) = start(
            vec![Command::Set {
                index: 2,
                field: Field::Contrast,
                percent: 30,
            }],
            state,
        );

        let event = events
            .recv_timeout(Duration::from_secs(2))
            .expect("failure event");
        match event {
            WorkerEvent::WriteFailed {
                index,
                field,
                message,
            } => {
                assert_eq!(index, 2);
                assert_eq!(field, Field::Contrast);
                assert!(message.contains("mock failure"));
            }
            other => panic!("unexpected event: {other:?}"),
        }

        assert!(
            events.recv_timeout(Duration::from_millis(200)).is_err(),
            "a failed write must not be confirmed"
        );

        commands.send(Command::Shutdown).expect("shutdown");
        handle.join().expect("join worker");
    }
}

//! Monitor abstraction and DDC/CI controller.

pub mod controller;
pub mod scale;

pub use controller::{MonitorController, MonitorError, MonitorSnapshot};

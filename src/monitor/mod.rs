//! Monitor abstraction and DDC/CI controller.

pub mod controller;
pub mod scale;

pub use controller::{MonitorController, MonitorError, MonitorSnapshot};

/// A VCP feature this application can control.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Field {
    Brightness,
    Contrast,
}

impl Field {
    /// All controllable fields, used for diagnostics and snapshots.
    pub const ALL: [Field; 2] = [Field::Brightness, Field::Contrast];

    /// VESA MCCS VCP code for the feature.
    pub fn vcp_code(self) -> u8 {
        match self {
            Field::Brightness => 0x10,
            Field::Contrast => 0x12,
        }
    }

    /// Human readable name used in logs and menu labels.
    pub fn name(self) -> &'static str {
        match self {
            Field::Brightness => "Brightness",
            Field::Contrast => "Contrast",
        }
    }
}

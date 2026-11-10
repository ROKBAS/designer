//! Conversion between user-facing percentages and raw VCP values.
//!
//! DDC/CI features report a maximum and current value. Monitors commonly use
//! 0..=100 or 0..=255, but any range must be handled. Conversions round to the
//! nearest value and clamp, so a requested percentage always maps to a value
//! the panel itself accepts.

/// Highest percentage we expose.
pub const MAX_PERCENT: u8 = 100;

/// Convert a percentage to a raw VCP value for the given maximum.
///
/// A maximum of zero means the feature has no usable range; callers treat that
/// case as "unsupported" before calling this function, and zero is returned
/// defensively.
pub fn percent_to_raw(percent: u8, maximum: u16) -> u16 {
    if maximum == 0 {
        return 0;
    }
    let percent = u32::from(percent.min(MAX_PERCENT));
    let maximum = u32::from(maximum);
    let raw = (percent * maximum + 50) / 100;
    raw.min(maximum) as u16
}

/// Convert a raw VCP value to a percentage, or `None` when the maximum is zero.
pub fn raw_to_percent(current: u16, maximum: u16) -> Option<u8> {
    if maximum == 0 {
        return None;
    }
    let maximum = u32::from(maximum);
    let current = u32::from(current.min(maximum as u16));
    let percent = (current * 100 + maximum / 2) / maximum;
    Some(percent.min(u32::from(MAX_PERCENT)) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_to_raw_on_hundred_range() {
        assert_eq!(percent_to_raw(0, 100), 0);
        assert_eq!(percent_to_raw(1, 100), 1);
        assert_eq!(percent_to_raw(45, 100), 45);
        assert_eq!(percent_to_raw(100, 100), 100);
    }

    #[test]
    fn percent_to_raw_rounds_on_255_range() {
        assert_eq!(percent_to_raw(0, 255), 0);
        assert_eq!(percent_to_raw(50, 255), 128);
        assert_eq!(percent_to_raw(100, 255), 255);
        // 33% of 255 is 84.15 -> 84
        assert_eq!(percent_to_raw(33, 255), 84);
    }

    #[test]
    fn percent_to_raw_never_exceeds_maximum() {
        for maximum in [1u16, 7, 100, 255, 1000] {
            for percent in (0..=100).step_by(7) {
                assert!(percent_to_raw(percent, maximum) <= maximum);
            }
        }
    }

    #[test]
    fn percent_to_raw_handles_zero_maximum() {
        assert_eq!(percent_to_raw(50, 0), 0);
    }

    #[test]
    fn raw_to_percent_rounds() {
        assert_eq!(raw_to_percent(0, 100), Some(0));
        assert_eq!(raw_to_percent(50, 100), Some(50));
        assert_eq!(raw_to_percent(100, 100), Some(100));
        assert_eq!(raw_to_percent(128, 255), Some(50));
        assert_eq!(raw_to_percent(255, 255), Some(100));
    }

    #[test]
    fn raw_to_percent_handles_zero_maximum() {
        assert_eq!(raw_to_percent(10, 0), None);
    }

    #[test]
    fn raw_to_percent_clamps_over_range_values() {
        assert_eq!(raw_to_percent(500, 255), Some(100));
    }
}

use gpui::Pixels;
use std::sync::atomic::{AtomicU32, Ordering};

pub(crate) const MIN_PERCENT: u32 = 75;
pub(crate) const MAX_PERCENT: u32 = 200;
pub(crate) const STEP_PERCENT: u32 = 25;

static PERCENT: AtomicU32 = AtomicU32::new(100);

pub(crate) fn normalize_percent(percent: u32) -> u32 {
    let clamped = percent.clamp(MIN_PERCENT, MAX_PERCENT);
    let snapped =
        MIN_PERCENT + ((clamped - MIN_PERCENT + STEP_PERCENT / 2) / STEP_PERCENT) * STEP_PERCENT;
    snapped.min(MAX_PERCENT)
}

pub(crate) fn set_percent(percent: u32) {
    PERCENT.store(normalize_percent(percent), Ordering::Relaxed);
}

pub(crate) fn percent() -> u32 {
    PERCENT.load(Ordering::Relaxed)
}

pub(crate) fn factor() -> f32 {
    percent() as f32 / 100.0
}

pub(crate) fn px(value: f32) -> Pixels {
    gpui::px(value * factor())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_is_clamped_and_snapped_to_supported_steps() {
        assert_eq!(normalize_percent(0), MIN_PERCENT);
        assert_eq!(normalize_percent(123), 125);
        assert_eq!(normalize_percent(138), 150);
        assert_eq!(normalize_percent(999), MAX_PERCENT);
    }
}

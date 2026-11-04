const FLOOR_DECIBELS: f32 = -60.0;

/// A linear amplitude on a 60 dB scale, `0.0` at the floor and `1.0` at full scale, the way
/// ears weigh it.
pub fn perceived_level(amplitude: f32) -> f32 {
    let decibels = 20.0 * amplitude.max(f32::MIN_POSITIVE).log10();
    (1.0 - decibels / FLOOR_DECIBELS).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_the_decibel_range_onto_the_unit_interval() {
        assert_eq!(perceived_level(0.0), 0.0);
        assert_eq!(perceived_level(1.0), 1.0);
        assert!((perceived_level(0.001) - 0.0).abs() < 1e-5);
        assert!((perceived_level(0.1) - 2.0 / 3.0).abs() < 1e-5);
    }
}

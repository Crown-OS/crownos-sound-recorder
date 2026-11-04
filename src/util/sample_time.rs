use std::time::Duration;

use crate::encode::SAMPLE_RATE;

const NANOS_PER_SECOND: u128 = 1_000_000_000;

pub fn samples_to_duration(samples: u64) -> Duration {
    let nanos = u128::from(samples) * NANOS_PER_SECOND / u128::from(SAMPLE_RATE);
    Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
}

pub fn duration_to_samples(duration: Duration) -> u64 {
    let samples = duration.as_nanos() * u128::from(SAMPLE_RATE) / NANOS_PER_SECOND;
    u64::try_from(samples).unwrap_or(u64::MAX)
}

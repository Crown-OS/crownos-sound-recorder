use std::time::Duration;

const SECONDS_PER_MINUTE: u64 = 60;
const SECONDS_PER_HOUR: u64 = 60 * SECONDS_PER_MINUTE;

pub fn duration_text(duration: Duration) -> String {
    let total = duration.as_secs();
    let hours = total / SECONDS_PER_HOUR;
    let minutes = total % SECONDS_PER_HOUR / SECONDS_PER_MINUTE;
    let seconds = total % SECONDS_PER_MINUTE;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_minutes_and_hours() {
        assert_eq!(duration_text(Duration::ZERO), "0:00");
        assert_eq!(duration_text(Duration::from_millis(65_900)), "1:05");
        assert_eq!(duration_text(Duration::from_secs(600)), "10:00");
        assert_eq!(duration_text(Duration::from_secs(3723)), "1:02:03");
    }
}

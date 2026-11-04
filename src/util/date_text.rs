use std::time::SystemTime;

use chrono::{DateTime, Local};

const DATE_FORMAT: &str = "%b %-d, %Y, %H:%M";

/// When a recording was made, in local time: "Nov 4, 2026, 20:15".
pub fn date_text(time: SystemTime) -> String {
    DateTime::<Local>::from(time)
        .format(DATE_FORMAT)
        .to_string()
}

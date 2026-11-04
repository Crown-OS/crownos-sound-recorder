use std::sync::Once;

use ffmpeg_next::util::log;

pub fn init_ffmpeg() -> Result<(), ffmpeg_next::Error> {
    static QUIET_LOGS: Once = Once::new();
    QUIET_LOGS.call_once(|| log::set_level(log::Level::Error));
    ffmpeg_next::init()
}

use std::borrow::Cow;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};

use crate::encode::RecordingFormat;

const PEAKS_EXTENSION: &str = ".peaks";
const ENHANCED_SUFFIX: &str = "enhanced";

pub fn recording_file_name(created: DateTime<Local>, format: RecordingFormat) -> String {
    format!(
        "Recording {}.{}",
        created.format("%Y-%m-%d %H.%M.%S"),
        format.extension()
    )
}

pub fn peaks_sidecar(audio: &Path) -> PathBuf {
    let mut sidecar = OsString::from(audio.as_os_str());
    sidecar.push(PEAKS_EXTENSION);
    sidecar.into()
}

pub fn title_of(path: &Path) -> Cow<'_, str> {
    path.file_stem()
        .map_or(Cow::Borrowed(""), |stem| stem.to_string_lossy())
}

pub fn enhanced_path(source: &Path, format: RecordingFormat) -> PathBuf {
    let title = title_of(source);
    let directory = source.parent().unwrap_or(Path::new(""));
    let candidate =
        |suffix: String| directory.join(format!("{title} ({suffix}).{}", format.extension()));
    std::iter::once(ENHANCED_SUFFIX.to_owned())
        .chain((2..).map(|attempt: u32| format!("{ENHANCED_SUFFIX} {attempt}")))
        .map(candidate)
        .find(|path| !path.exists())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn names_new_recordings_by_local_time() {
        let created = Local.with_ymd_and_hms(2026, 11, 4, 20, 15, 3).single();
        let name = created.map(|time| recording_file_name(time, RecordingFormat::Opus));
        assert_eq!(name.as_deref(), Some("Recording 2026-11-04 20.15.03.ogg"));
    }

    #[test]
    fn sidecar_appends_to_the_full_file_name() {
        assert_eq!(
            peaks_sidecar(Path::new("/music/a.ogg")),
            Path::new("/music/a.ogg.peaks")
        );
    }
}

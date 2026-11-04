use std::cmp::Reverse;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use chrono::Local;

use super::{RenameError, load_or_compute_peaks};
use crate::decode::probe_audio;
use crate::encode::RecordingFormat;
use crate::util::Trash;
use crate::util::paths::{peaks_sidecar, recording_file_name, title_of};

const RECORDINGS_FOLDER: &str = "Recordings";
const MUSIC_FALLBACK: &str = "Music";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recording {
    pub path: PathBuf,
    pub title: String,
    pub created: SystemTime,
    pub duration: Duration,
    pub format: RecordingFormat,
}

impl Recording {
    fn read(path: PathBuf, format: RecordingFormat) -> Result<Self> {
        let metadata = fs::metadata(&path)?;
        let created = metadata.created().or_else(|_| metadata.modified())?;
        let duration = probe_audio(&path).map_or(Duration::ZERO, |probe| probe.duration);
        Ok(Self {
            title: title_of(&path).into_owned(),
            path,
            created,
            duration,
            format,
        })
    }
}

/// The recordings folder, `$XDG_MUSIC_DIR/Recordings` by default.
#[derive(Debug, Clone)]
pub struct Library {
    root: PathBuf,
    trash: Trash,
}

impl Library {
    pub fn open_default() -> Result<Self> {
        let music = dirs::audio_dir()
            .or_else(|| dirs::home_dir().map(|home| home.join(MUSIC_FALLBACK)))
            .context("no music or home directory")?;
        Self::open(music.join(RECORDINGS_FOLDER), Trash::home()?)
    }

    pub fn open(root: impl Into<PathBuf>, trash: Trash) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root).with_context(|| format!("creating {}", root.display()))?;
        Ok(Self { root, trash })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Every recording in the folder, newest first.
    pub fn list(&self) -> Result<Vec<Recording>> {
        let mut recordings = fs::read_dir(&self.root)?
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                let format = RecordingFormat::of_path(&path)?;
                Recording::read(path, format).ok()
            })
            .collect::<Vec<_>>();
        recordings.sort_unstable_by_key(|recording| Reverse(recording.created));
        Ok(recordings)
    }

    /// Where a recording started now should be written.
    pub fn new_recording_path(&self, format: RecordingFormat) -> PathBuf {
        self.root.join(recording_file_name(Local::now(), format))
    }

    pub fn rename(&self, recording: &Recording, title: &str) -> Result<Recording, RenameError> {
        let title = title.trim();
        if title.is_empty() {
            return Err(RenameError::EmptyTitle);
        }
        if title.contains(['/', '\0']) || title.starts_with('.') {
            return Err(RenameError::InvalidTitle);
        }
        let target = self
            .root
            .join(format!("{title}.{}", recording.format.extension()));
        if target.exists() {
            return Err(RenameError::AlreadyExists(target));
        }
        fs::rename(&recording.path, &target)?;
        let sidecar = peaks_sidecar(&recording.path);
        if sidecar.exists() {
            fs::rename(sidecar, peaks_sidecar(&target))?;
        }
        Ok(Recording {
            path: target,
            title: title.to_owned(),
            ..recording.clone()
        })
    }

    /// Moves the recording and its peaks to the trash.
    pub fn delete(&self, recording: &Recording) -> Result<()> {
        self.trash.put(&recording.path)?;
        let sidecar = peaks_sidecar(&recording.path);
        if sidecar.exists() {
            self.trash.put(&sidecar)?;
        }
        Ok(())
    }

    pub fn peaks(&self, recording: &Recording) -> Result<Vec<u8>> {
        load_or_compute_peaks(&recording.path)
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use super::*;
    use crate::recording::{RecordingSession, ToneGenerator, read_peaks};

    fn record(library: &Library, name: &str, seconds: f32) -> Result<PathBuf> {
        let path = library.root().join(name);
        RecordingSession::start(
            &path,
            RecordingFormat::Opus,
            ToneGenerator::with_seconds(220.0, seconds),
        )?
        .stop()?;
        Ok(path)
    }

    #[test]
    fn lists_renames_and_trashes() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let library = Library::open(
            directory.path().join("Recordings"),
            Trash::at(directory.path().join("Trash")),
        )?;
        record(&library, "older.ogg", 1.0)?;
        thread::sleep(Duration::from_millis(20));
        record(&library, "newer.ogg", 0.5)?;
        fs::write(library.root().join("notes.txt"), b"ignored")?;

        let listed = library.list()?;
        let titles: Vec<_> = listed
            .iter()
            .map(|recording| recording.title.as_str())
            .collect();
        assert_eq!(titles, ["newer", "older"]);
        assert!(listed[1].duration.abs_diff(Duration::from_secs(1)) < Duration::from_millis(50));

        let renamed = library.rename(&listed[0], "Interview")?;
        assert_eq!(renamed.path, library.root().join("Interview.ogg"));
        assert!(read_peaks(&renamed.path).is_ok());
        assert!(matches!(
            library.rename(&renamed, "older"),
            Err(RenameError::AlreadyExists(_))
        ));
        assert!(matches!(
            library.rename(&renamed, "../x"),
            Err(RenameError::InvalidTitle)
        ));

        library.delete(&renamed)?;
        assert_eq!(library.list()?.len(), 1);
        let trashed = directory.path().join("Trash/files");
        assert!(trashed.join("Interview.ogg").exists());
        assert!(trashed.join("Interview.ogg.peaks").exists());
        Ok(())
    }

    #[test]
    fn computes_missing_peaks_from_audio() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let library = Library::open(directory.path(), Trash::at(directory.path().join("Trash")))?;
        let path = record(&library, "take.ogg", 1.0)?;
        let written = read_peaks(&path)?;
        fs::remove_file(peaks_sidecar(&path))?;
        let recording = library.list()?.remove(0);
        let computed = library.peaks(&recording)?;
        assert_eq!(computed.len(), written.len());
        let largest_difference = computed
            .iter()
            .zip(&written)
            .map(|(left, right)| left.abs_diff(*right))
            .max();
        assert!(largest_difference <= Some(8), "{largest_difference:?}");
        assert_eq!(read_peaks(&path)?, computed);
        Ok(())
    }
}

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;

use super::PeaksWriter;
use crate::encode::{AudioFileWriter, RecordingFormat};
use crate::util::paths::peaks_sidecar;

/// An audio file and its peaks sidecar, written together.
pub struct RecordingWriter {
    path: PathBuf,
    audio: AudioFileWriter,
    peaks: PeaksWriter<BufWriter<File>>,
}

impl RecordingWriter {
    pub fn create(path: impl Into<PathBuf>, format: RecordingFormat) -> Result<Self> {
        let path = path.into();
        let audio = AudioFileWriter::create(&path, format)?;
        let peaks = PeaksWriter::create(&path)?;
        Ok(Self { path, audio, peaks })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn write(&mut self, samples: &[f32]) -> Result<()> {
        self.audio.write(samples)?;
        self.peaks.push(samples)?;
        Ok(())
    }

    pub fn samples_written(&self) -> u64 {
        self.audio.samples_written()
    }

    pub fn finish(self) -> Result<Duration> {
        self.peaks.finish()?;
        self.audio.finish()
    }

    /// Drops what was written and removes both files.
    pub fn discard(self) -> Result<()> {
        let Self { path, audio, peaks } = self;
        drop((audio, peaks));
        std::fs::remove_file(peaks_sidecar(&path))?;
        std::fs::remove_file(&path)?;
        Ok(())
    }
}

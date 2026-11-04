use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};

use super::RecordingFormat;
use super::container::Muxer;
use super::encoder::AudioEncoder;
use crate::util::ffmpeg::init_ffmpeg;
use crate::util::sample_time::samples_to_duration;

/// Writes mono 48 kHz samples to an audio file in a [`RecordingFormat`].
pub struct AudioFileWriter {
    encoder: AudioEncoder,
    muxer: Muxer,
    samples_written: u64,
}

impl AudioFileWriter {
    pub fn create(path: &Path, format: RecordingFormat) -> Result<Self> {
        init_ffmpeg()?;
        let encoder = AudioEncoder::open(format)?;
        let muxer = Muxer::create(path, format, encoder.context())
            .with_context(|| format!("creating {}", path.display()))?;
        Ok(Self {
            encoder,
            muxer,
            samples_written: 0,
        })
    }

    pub fn write(&mut self, samples: &[f32]) -> Result<()> {
        self.encoder.write(samples, &mut self.muxer)?;
        self.samples_written += samples.len() as u64;
        Ok(())
    }

    pub fn samples_written(&self) -> u64 {
        self.samples_written
    }

    /// Flushes the encoder, writes the trailer and returns the recorded duration.
    pub fn finish(mut self) -> Result<Duration> {
        self.encoder.finish(&mut self.muxer)?;
        self.muxer.finish()?;
        Ok(samples_to_duration(self.samples_written))
    }
}

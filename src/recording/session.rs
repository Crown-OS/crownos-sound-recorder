use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};

use super::{FrameSource, RecordingWriter};
use crate::encode::RecordingFormat;
use crate::util::sample_time::samples_to_duration;

const IDLE_WAIT: Duration = Duration::from_millis(20);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SessionState {
    Recording,
    Paused,
    Stopping,
}

impl SessionState {
    const fn from_raw(raw: u8) -> Self {
        match raw {
            0 => Self::Recording,
            1 => Self::Paused,
            _ => Self::Stopping,
        }
    }
}

struct SessionControl {
    state: AtomicU8,
    samples_written: AtomicU64,
}

impl SessionControl {
    fn state(&self) -> SessionState {
        SessionState::from_raw(self.state.load(Ordering::Acquire))
    }
}

/// Records frames into a file on its own thread until stopped; dropping it stops it too.
pub struct RecordingSession {
    path: PathBuf,
    control: Arc<SessionControl>,
    worker: Option<JoinHandle<Result<Duration>>>,
}

impl RecordingSession {
    pub fn start<Source>(
        path: impl Into<PathBuf>,
        format: RecordingFormat,
        frames: Source,
    ) -> Result<Self>
    where
        Source: FrameSource + Send + 'static,
    {
        let path = path.into();
        let control = Arc::new(SessionControl {
            state: AtomicU8::new(SessionState::Recording as u8),
            samples_written: AtomicU64::new(0),
        });
        let (opened_sender, opened) = mpsc::sync_channel(1);
        let worker = thread::Builder::new().name("recording".into()).spawn({
            let path = path.clone();
            let control = Arc::clone(&control);
            move || match RecordingWriter::create(path, format) {
                Ok(writer) => {
                    let _ = opened_sender.send(Ok(()));
                    record(writer, frames, &control)
                }
                Err(error) => {
                    let _ = opened_sender.send(Err(error));
                    Err(anyhow!("recording did not start"))
                }
            }
        })?;
        opened
            .recv()
            .context("recording thread exited")?
            .with_context(|| format!("starting {}", path.display()))?;
        Ok(Self {
            path,
            control,
            worker: Some(worker),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn state(&self) -> SessionState {
        self.control.state()
    }

    pub fn pause(&self) {
        self.transition(SessionState::Recording, SessionState::Paused);
    }

    pub fn resume(&self) {
        self.transition(SessionState::Paused, SessionState::Recording);
    }

    /// Recorded time, excluding pauses.
    pub fn elapsed(&self) -> Duration {
        samples_to_duration(self.control.samples_written.load(Ordering::Relaxed))
    }

    /// Drains what the source still holds, finalizes the file and returns its duration.
    pub fn stop(mut self) -> Result<Duration> {
        self.finish()
    }

    fn finish(&mut self) -> Result<Duration> {
        let worker = self.worker.take().context("recording already stopped")?;
        self.control
            .state
            .store(SessionState::Stopping as u8, Ordering::Release);
        worker.thread().unpark();
        worker
            .join()
            .map_err(|_| anyhow!("recording thread panicked"))?
    }

    fn transition(&self, from: SessionState, to: SessionState) {
        let _ = self.control.state.compare_exchange(
            from as u8,
            to as u8,
            Ordering::AcqRel,
            Ordering::Relaxed,
        );
    }
}

impl Drop for RecordingSession {
    fn drop(&mut self) {
        if self.worker.is_some() {
            let _ = self.finish();
        }
    }
}

fn record(
    mut writer: RecordingWriter,
    mut frames: impl FrameSource,
    control: &SessionControl,
) -> Result<Duration> {
    loop {
        let state = control.state();
        while let Some(frame) = frames.pop_frame() {
            if state != SessionState::Paused {
                writer.write(&frame)?;
            }
        }
        control
            .samples_written
            .store(writer.samples_written(), Ordering::Relaxed);
        if state == SessionState::Stopping {
            return writer.finish();
        }
        thread::park_timeout(IDLE_WAIT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::{PcmDecoder, probe_audio};
    use crate::encode::SAMPLE_RATE;
    use crate::recording::{ToneGenerator, read_peaks};

    const TONE_SECONDS: f32 = 2.0;
    const TOLERANCE: Duration = Duration::from_millis(50);

    fn record_tone(format: RecordingFormat) -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory
            .path()
            .join(format!("tone.{}", format.extension()));
        let session = RecordingSession::start(
            &path,
            format,
            ToneGenerator::with_seconds(440.0, TONE_SECONDS),
        )?;
        let recorded = session.stop()?;
        let expected = Duration::from_secs_f32(TONE_SECONDS);
        assert_eq!(recorded, expected);
        let probe = probe_audio(&path)?;
        assert!(
            probe.duration.abs_diff(expected) <= TOLERANCE,
            "{format:?} lasts {:?}",
            probe.duration
        );
        assert_eq!(probe.sample_rate, SAMPLE_RATE);
        assert_eq!(probe.channels, 1);
        assert_eq!(read_peaks(&path)?.len(), 40);
        Ok(())
    }

    #[test]
    fn records_opus_in_ogg() -> Result<()> {
        record_tone(RecordingFormat::Opus)
    }

    #[test]
    fn records_flac() -> Result<()> {
        record_tone(RecordingFormat::Flac)
    }

    #[test]
    fn flac_is_lossless_at_24_bits() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("tone.flac");
        let tone = ToneGenerator::with_seconds(440.0, 1.0);
        RecordingSession::start(&path, RecordingFormat::Flac, tone.clone())?.stop()?;
        let expected: Vec<f32> = std::iter::from_fn({
            let mut tone = tone;
            move || tone.pop_frame()
        })
        .flatten()
        .collect();
        let mut decoded = vec![0.0; expected.len() + 1];
        let count = PcmDecoder::open(&path)?.read(&mut decoded)?;
        assert_eq!(count, expected.len());
        let largest_error = expected
            .iter()
            .zip(&decoded)
            .map(|(left, right)| (left - right).abs())
            .fold(0.0, f32::max);
        assert!(largest_error < 1e-6, "{largest_error}");
        Ok(())
    }

    #[test]
    fn paused_frames_are_dropped() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let session = RecordingSession::start(
            directory.path().join("paused.ogg"),
            RecordingFormat::Opus,
            ToneGenerator::new(440.0, 0),
        )?;
        session.pause();
        assert_eq!(session.state(), SessionState::Paused);
        session.resume();
        assert_eq!(session.state(), SessionState::Recording);
        assert_eq!(session.stop()?, Duration::ZERO);
        Ok(())
    }
}

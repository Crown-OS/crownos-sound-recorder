use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use voice_protocol::{Enhancement, JobId, voice};

use super::{EnhanceError, RecordingWriter};
use crate::decode::{PcmDecoder, probe_audio};
use crate::encode::RecordingFormat;
use crate::util::paths::{enhanced_path, peaks_sidecar};
use crate::util::sample_time::duration_to_samples;

const REPLY_TIMEOUT: Duration = Duration::from_secs(10);
const SUPERVISE_INTERVAL: Duration = Duration::from_millis(50);
const BLOCK_SAMPLES: usize = 4800;
const SAMPLE_BYTES: usize = size_of::<f32>();

/// Where `crown-voiced` listens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceService {
    /// `$XDG_RUNTIME_DIR/crownos`.
    Session,
    Directory(PathBuf),
}

impl VoiceService {
    fn connect(&self) -> Result<voice::Client, crownos_ipc::Error> {
        match self {
            Self::Session => voice::Client::connect(),
            Self::Directory(directory) => voice::Client::connect_in(directory),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnhanceOutcome {
    Finished(PathBuf),
    Cancelled,
}

#[derive(Default)]
struct JobProgress {
    cancelled: AtomicBool,
    samples_done: AtomicU64,
    samples_total: AtomicU64,
}

impl JobProgress {
    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

/// Offline enhancement of a recording into `<title> (enhanced).<ext>`, keeping the original.
/// Dropping an unfinished job cancels it.
pub struct EnhanceJob {
    progress: Arc<JobProgress>,
    worker: Option<JoinHandle<Result<EnhanceOutcome, EnhanceError>>>,
}

impl EnhanceJob {
    pub fn start(
        source: &Path,
        enhancement: Enhancement,
        service: VoiceService,
    ) -> Result<Self, EnhanceError> {
        let progress = Arc::new(JobProgress::default());
        let source = source.to_path_buf();
        let worker = thread::Builder::new().name("enhance".into()).spawn({
            let progress = Arc::clone(&progress);
            move || run(source, enhancement, &service, &progress)
        })?;
        Ok(Self {
            progress,
            worker: Some(worker),
        })
    }

    /// Fraction of the source enhanced so far, 0.0 to 1.0.
    pub fn progress(&self) -> f32 {
        let total = self.progress.samples_total.load(Ordering::Relaxed);
        let done = self.progress.samples_done.load(Ordering::Relaxed);
        if total == 0 {
            0.0
        } else {
            (done as f64 / total as f64).min(1.0) as f32
        }
    }

    pub fn cancel(&self) {
        self.progress.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_finished(&self) -> bool {
        self.worker.as_ref().is_none_or(JoinHandle::is_finished)
    }

    pub fn wait(mut self) -> Result<EnhanceOutcome, EnhanceError> {
        self.worker
            .take()
            .ok_or(EnhanceError::Panicked)?
            .join()
            .map_err(|_| EnhanceError::Panicked)?
    }
}

impl Drop for EnhanceJob {
    fn drop(&mut self) {
        if self.worker.is_some() {
            self.cancel();
        }
    }
}

fn run(
    source: PathBuf,
    enhancement: Enhancement,
    service: &VoiceService,
    progress: &Arc<JobProgress>,
) -> Result<EnhanceOutcome, EnhanceError> {
    let total = duration_to_samples(probe_audio(&source)?.duration);
    progress.samples_total.store(total, Ordering::Relaxed);
    let format = RecordingFormat::of_path(&source).unwrap_or_default();
    let target = enhanced_path(&source, format);

    let mut client = service
        .connect()
        .map_err(EnhanceError::ServiceUnavailable)?;
    client.subscribe_blocking::<voice::JobFailed>(Some(REPLY_TIMEOUT))?;
    let (input_reader, input_writer) = io::pipe()?;
    let (output_reader, output_writer) = io::pipe()?;
    let pending = client.enhance_pcm(enhancement, input_reader.into(), output_writer.into())?;
    let job = client.wait(pending, Some(REPLY_TIMEOUT))?;

    let feeder = thread::Builder::new().name("enhance-feed".into()).spawn({
        let progress = Arc::clone(progress);
        move || feed(&source, input_writer, &progress)
    })?;
    let receiver = thread::Builder::new()
        .name("enhance-receive".into())
        .spawn({
            let progress = Arc::clone(progress);
            let target = target.clone();
            move || receive(&target, format, output_reader, &progress)
        })?;
    let failure = supervise(client, job, &receiver, progress);
    let received = receiver.join().map_err(|_| EnhanceError::Panicked)?;
    let fed = feeder.join().map_err(|_| EnhanceError::Panicked)?;
    if let Some(reason) = failure {
        remove_output(&target);
        return Err(EnhanceError::JobFailed(reason));
    }
    if progress.is_cancelled() {
        remove_output(&target);
        return Ok(EnhanceOutcome::Cancelled);
    }
    received?;
    fed?;
    Ok(EnhanceOutcome::Finished(target))
}

/// Watches the job until the output is complete; dropping the client ends the job early.
fn supervise<T>(
    mut client: voice::Client,
    job: JobId,
    receiver: &JoinHandle<T>,
    progress: &JobProgress,
) -> Option<String> {
    while !receiver.is_finished() {
        if progress.is_cancelled() {
            let _ = client.cancel_job(job);
            return None;
        }
        if let Some(reason) = job_failure(&mut client, job) {
            return Some(reason);
        }
        thread::sleep(SUPERVISE_INTERVAL);
    }
    job_failure(&mut client, job)
}

fn job_failure(client: &mut voice::Client, job: JobId) -> Option<String> {
    if client.handle_readable().is_err() {
        return Some("the voice service disconnected".into());
    }
    std::iter::from_fn(|| client.next_event()).find_map(|event| match event {
        voice::Event::JobFailed(failed) if failed.job == job => Some(failed.reason),
        _ => None,
    })
}

fn feed(
    source: &Path,
    mut input: io::PipeWriter,
    progress: &JobProgress,
) -> Result<(), EnhanceError> {
    let mut decoder = PcmDecoder::open(source)?;
    let mut samples = [0.0; BLOCK_SAMPLES];
    let mut bytes = [0; BLOCK_SAMPLES * SAMPLE_BYTES];
    while !progress.is_cancelled() {
        let count = decoder.read(&mut samples)?;
        let (slots, _) = bytes.as_chunks_mut::<SAMPLE_BYTES>();
        slots
            .iter_mut()
            .zip(&samples[..count])
            .for_each(|(slot, sample)| *slot = sample.to_le_bytes());
        input.write_all(&bytes[..count * SAMPLE_BYTES])?;
        if count < BLOCK_SAMPLES {
            break;
        }
    }
    Ok(())
}

fn receive(
    target: &Path,
    format: RecordingFormat,
    output: io::PipeReader,
    progress: &JobProgress,
) -> Result<(), EnhanceError> {
    let mut writer = RecordingWriter::create(target, format)?;
    match pump(output, &mut writer, progress) {
        Ok(()) if !progress.is_cancelled() => Ok(writer.finish().map(drop)?),
        outcome => {
            writer.discard()?;
            outcome
        }
    }
}

fn pump(
    mut output: io::PipeReader,
    writer: &mut RecordingWriter,
    progress: &JobProgress,
) -> Result<(), EnhanceError> {
    let mut bytes = [0; BLOCK_SAMPLES * SAMPLE_BYTES];
    let mut samples = [0.0; BLOCK_SAMPLES];
    let mut pending_bytes = 0;
    while !progress.is_cancelled() {
        let read = match output.read(&mut bytes[pending_bytes..]) {
            Ok(0) => return Ok(()),
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.into()),
        };
        let available = pending_bytes + read;
        let (whole, _) = bytes[..available].as_chunks::<SAMPLE_BYTES>();
        let count = whole.len();
        samples
            .iter_mut()
            .zip(whole)
            .for_each(|(sample, sample_bytes)| *sample = f32::from_le_bytes(*sample_bytes));
        writer.write(&samples[..count])?;
        progress
            .samples_done
            .store(writer.samples_written(), Ordering::Relaxed);
        pending_bytes = available - count * SAMPLE_BYTES;
        bytes.copy_within(count * SAMPLE_BYTES..available, 0);
    }
    Ok(())
}

fn remove_output(target: &Path) {
    let _ = fs::remove_file(target);
    let _ = fs::remove_file(peaks_sidecar(target));
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::os::fd::OwnedFd;

    use crownos_ipc::{RemoteError, ServiceBuilder, blocking};
    use voice_protocol::{EnhancedNode, ServiceStatus, StreamId};

    use super::*;
    use crate::recording::{RecordingSession, ToneGenerator, read_peaks};

    #[derive(Clone, Copy)]
    enum FakeBehaviour {
        Echo,
        StallUntilCancelled,
    }

    struct FakeVoiceService {
        behaviour: FakeBehaviour,
        stalled: Vec<OwnedFd>,
    }

    fn unsupported<T>() -> Result<T, RemoteError> {
        Err(RemoteError::handler("not part of the fake"))
    }

    impl voice::Handler for FakeVoiceService {
        fn open_stream(
            &mut self,
            _: Option<String>,
            _: Enhancement,
        ) -> Result<EnhancedNode, RemoteError> {
            unsupported()
        }

        fn set_enhancement(
            &mut self,
            _: StreamId,
            _: Enhancement,
        ) -> Result<EnhancedNode, RemoteError> {
            unsupported()
        }

        fn close_stream(&mut self, _: StreamId) -> Result<(), RemoteError> {
            unsupported()
        }

        fn enhance_pcm(
            &mut self,
            _: Enhancement,
            input: OwnedFd,
            output: OwnedFd,
        ) -> Result<JobId, RemoteError> {
            match self.behaviour {
                FakeBehaviour::Echo => {
                    thread::spawn(move || {
                        io::copy(&mut File::from(input), &mut File::from(output))
                    });
                }
                FakeBehaviour::StallUntilCancelled => self.stalled.extend([input, output]),
            }
            Ok(JobId(7))
        }

        fn cancel_job(&mut self, _: JobId) -> Result<(), RemoteError> {
            self.stalled.clear();
            Ok(())
        }

        fn status(&mut self) -> Result<ServiceStatus, RemoteError> {
            unsupported()
        }
    }

    fn spawn_fake_service(directory: &Path, behaviour: FakeBehaviour) -> anyhow::Result<()> {
        let mut server = ServiceBuilder::new(voice::SERVICE)
            .directory(directory)
            .build()?;
        thread::spawn(move || {
            let mut handler = FakeVoiceService {
                behaviour,
                stalled: Vec::new(),
            };
            blocking::serve(&mut server, |server, peer, message| {
                voice::dispatch(&mut handler, server, peer, message).map(|()| true)
            })
        });
        Ok(())
    }

    fn record_memo(directory: &Path, seconds: f32) -> anyhow::Result<PathBuf> {
        let source = directory.join("Memo.ogg");
        let tone = ToneGenerator::with_seconds(330.0, seconds);
        RecordingSession::start(&source, RecordingFormat::Opus, tone)?.stop()?;
        Ok(source)
    }

    #[test]
    fn enhances_through_the_voice_service() -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let sockets = directory.path().join("sockets");
        spawn_fake_service(&sockets, FakeBehaviour::Echo)?;
        let source = record_memo(directory.path(), 3.0)?;

        let job = EnhanceJob::start(
            &source,
            Enhancement::NoiseSuppression,
            VoiceService::Directory(sockets),
        )?;
        let outcome = job.wait()?;

        let enhanced = directory.path().join("Memo (enhanced).ogg");
        assert_eq!(outcome, EnhanceOutcome::Finished(enhanced.clone()));
        let original = probe_audio(&source)?.duration;
        let result = probe_audio(&enhanced)?.duration;
        assert!(
            original.abs_diff(result) <= Duration::from_millis(50),
            "{original:?} vs {result:?}"
        );
        assert_eq!(read_peaks(&enhanced)?.len(), 60);
        Ok(())
    }

    #[test]
    fn cancels_a_running_job() -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let sockets = directory.path().join("sockets");
        spawn_fake_service(&sockets, FakeBehaviour::StallUntilCancelled)?;
        let source = record_memo(directory.path(), 1.0)?;

        let job = EnhanceJob::start(
            &source,
            Enhancement::StudioVoice,
            VoiceService::Directory(sockets),
        )?;
        thread::sleep(Duration::from_millis(200));
        assert!(!job.is_finished());
        job.cancel();

        assert_eq!(job.wait()?, EnhanceOutcome::Cancelled);
        assert!(!directory.path().join("Memo (enhanced).ogg").exists());
        Ok(())
    }

    #[test]
    fn reports_a_missing_service() -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let source = record_memo(directory.path(), 0.2)?;
        let job = EnhanceJob::start(
            &source,
            Enhancement::Light,
            VoiceService::Directory(directory.path().join("missing")),
        )?;
        assert!(matches!(
            job.wait(),
            Err(EnhanceError::ServiceUnavailable(_))
        ));
        Ok(())
    }
}

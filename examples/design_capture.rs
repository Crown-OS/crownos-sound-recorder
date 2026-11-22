//! Captures every recorder state for the CrownOS-Design Figma file. Run both appearances with
//! `../crownui/capture/scripts/capture-app.sh .`; output lands in `$CROWN_DESIGN_DIR`.

use std::cell::Cell;
use std::f32::consts::PI;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use audio::{Enhancement, Level, StreamState, UnenhancedReason};
use capture::{CaptureApp, Carrier};
use crownos_sound_recorder::app::{app_config, shell};
use crownos_sound_recorder::encode::RecordingFormat;
use crownos_sound_recorder::recording::{
    Library, PEAKS_PER_SECOND, RecordingSession, ToneGenerator,
};
use crownos_sound_recorder::settings::{Persistence, Preferences};
use crownos_sound_recorder::state::{AppState, Capture, Environment, MicrophoneInput, Selection};
use crownos_sound_recorder::util::Trash;
use crowntest::{Role, role};
use crownui::kit::provide_configured_theme;
use crownui::prelude::{Signal, component};
use tempfile::TempDir;
use view::{TaskExecutor, TaskFuture, TaskId};

const ENHANCER: &str = "cpu: DeepFilterNet3";
const PEAKS_MAGIC: &[u8] = b"CRP1";
const TONE_HZ: f32 = 220.0;
const LIVE_SPEECH_PER_UPDATE: f32 = 30.0;
const METER_FLOOR_DECADES: f32 = 3.0;
const LIVE_UPDATES: usize = 230;
const LIVE_TICK_MILLIS: u64 = 16;
const TASK_DEADLINE: Duration = Duration::from_millis(250);
const RMS_SHARE: f32 = 0.45;
const RECORDED_SO_FAR: Duration = Duration::from_secs(42);
const CREATION_GAP: Duration = Duration::from_millis(15);
const FEATURED: &str = "Interview with Priya";

struct SampleRecording {
    title: &'static str,
    seconds: f32,
    format: RecordingFormat,
}

const SAMPLE_RECORDINGS: [SampleRecording; 6] = [
    SampleRecording {
        title: "Guitar idea in D minor",
        seconds: 47.0,
        format: RecordingFormat::Flac,
    },
    SampleRecording {
        title: "Lecture — Heat engines",
        seconds: 326.0,
        format: RecordingFormat::Opus,
    },
    SampleRecording {
        title: "Podcast intro, take 3",
        seconds: 78.0,
        format: RecordingFormat::Opus,
    },
    SampleRecording {
        title: FEATURED,
        seconds: 222.0,
        format: RecordingFormat::Opus,
    },
    SampleRecording {
        title: "Grocery list",
        seconds: 12.0,
        format: RecordingFormat::Opus,
    },
    SampleRecording {
        title: "Weekly design sync",
        seconds: 125.0,
        format: RecordingFormat::Opus,
    },
];

/// Holds every spawned task until the harness runs it.
struct HeldTasks(mpsc::Sender<(TaskId, TaskFuture)>);

impl TaskExecutor for HeldTasks {
    fn spawn(&mut self, id: TaskId, future: TaskFuture) {
        let _ = self.0.send((id, future));
    }

    fn cancel(&mut self, _id: TaskId) {}
}

struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 >> 8) as f32 / (1 << 24) as f32
    }
}

/// Peak amplitudes that rise and fall like speech: phrases of syllables between short pauses.
fn speech_levels(count: usize, per_second: f32, seed: u32) -> Vec<f32> {
    let mut noise = Noise(seed | 1);
    let mut levels = Vec::with_capacity(count);
    while levels.len() < count {
        let phrase = (per_second * (1.2 + 2.8 * noise.next())) as usize;
        let loudness = 0.12 + 0.75 * noise.next();
        let syllables_per_second = 3.5 + 2.0 * noise.next();
        let fade_steps = per_second * 0.15;
        levels.extend((0..phrase).map(|step| {
            let seconds = step as f32 / per_second;
            let syllable = (seconds * syllables_per_second * PI).sin().abs();
            let fade = (step.min(phrase - step) as f32 / fade_steps).min(1.0);
            loudness * fade * (0.3 + 0.7 * syllable) * (0.75 + 0.25 * noise.next())
        }));
        let pause = (per_second * (0.3 + 0.9 * noise.next())) as usize;
        levels.extend((0..pause).map(|_| 0.002 + 0.004 * noise.next()));
    }
    levels.truncate(count);
    levels
}

fn write_speech_peaks(audio: &Path, seconds: f32, seed: u32) -> Result<()> {
    let count = (seconds * PEAKS_PER_SECOND as f32).ceil() as usize;
    let peaks = PEAKS_MAGIC.iter().copied().chain(
        speech_levels(count, PEAKS_PER_SECOND as f32, seed)
            .into_iter()
            .map(|level| (level.min(1.0) * f32::from(u8::MAX)).round() as u8),
    );
    fs::write(peaks_of(audio), peaks.collect::<Vec<_>>())?;
    Ok(())
}

/// Records every sample once, oldest first, and returns their files in that order.
fn record_samples(folder: &Path) -> Result<Vec<PathBuf>> {
    SAMPLE_RECORDINGS
        .iter()
        .zip(1..)
        .map(|(sample, seed)| {
            let path = folder.join(format!("{}.{}", sample.title, sample.format.extension()));
            let tone = ToneGenerator::with_seconds(TONE_HZ, sample.seconds);
            RecordingSession::start(&path, sample.format, tone)?.stop()?;
            write_speech_peaks(&path, sample.seconds, seed)?;
            Ok(path)
        })
        .collect()
}

fn peaks_of(audio: &Path) -> PathBuf {
    let mut sidecar = audio.as_os_str().to_owned();
    sidecar.push(".peaks");
    sidecar.into()
}

/// A library of its own holding copies of `samples`, created in order so the newest lists first.
fn sample_library(samples: &[PathBuf]) -> Result<(TempDir, Library)> {
    let home = tempfile::tempdir()?;
    let library = Library::open(
        home.path().join("Recordings"),
        Trash::at(home.path().join("Trash")),
    )?;
    for sample in samples {
        let name = sample.file_name().context("sample without a name")?;
        let copy = library.root().join(name);
        fs::copy(sample, &copy)?;
        fs::copy(peaks_of(sample), peaks_of(&copy))?;
        thread::sleep(CREATION_GAP);
    }
    Ok((home, library))
}

fn preferences(enhancement: Enhancement) -> Preferences {
    Preferences {
        format: RecordingFormat::Opus,
        enhancement,
        input: None,
    }
}

fn enhanced() -> StreamState {
    StreamState::Live {
        enhanced_by: Some(ENHANCER.to_owned()),
    }
}

struct Window {
    app: CaptureApp,
    state: AppState,
    level: Signal<Level>,
    _home: TempDir,
}

impl Window {
    fn open(
        samples: &[PathBuf],
        enhancement: Enhancement,
        microphone: StreamState,
    ) -> Result<Self> {
        let (home, library) = sample_library(samples)?;
        let environment = Environment {
            library,
            preferences: preferences(enhancement),
            capture: Capture::Tone,
            persistence: Persistence::Memory,
        };
        let mounted = Rc::new(Cell::new(None));
        let root = component({
            let mounted = Rc::clone(&mounted);
            move |cx| {
                provide_configured_theme(cx);
                let level = cx.signal(Level::default());
                let state = cx.signal(microphone);
                cx.provide(MicrophoneInput::Synthetic { level, state });
                let state = AppState::new(cx, environment);
                mounted.set(Some((state, level)));
                shell(cx, state)
            }
        });
        let app = capture::mount_app(root, &app_config(), None)?;
        let (state, level) = mounted.get().context("the recorder did not mount")?;
        Ok(Self {
            app,
            state,
            level,
            _home: home,
        })
    }

    fn standard(samples: &[PathBuf]) -> Result<Self> {
        Self::open(samples, Enhancement::NoiseSuppression, enhanced())
    }

    fn show_featured(&mut self) -> &mut Self {
        self.app.click(role(Role::ListItem).name(FEATURED));
        self.app.settle();
        self
    }

    /// Presses Record and runs the take's level ticks by hand while feeding speech-like levels,
    /// then leaves the last tick pending so the 1-second tone stops driving the clock.
    fn record_take(&mut self) -> Result<()> {
        let (spawned, tasks) = mpsc::channel();
        self.app
            .ui()
            .set_task_executor(Box::new(HeldTasks(spawned)));
        let timers = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()?;
        self.app.click(role(Role::Button).name("Record"));
        for shown in speech_levels(LIVE_UPDATES, LIVE_SPEECH_PER_UPDATE, 7) {
            let peak = 10f32.powf(METER_FLOOR_DECADES * (shown - 1.0));
            self.level.set(
                self.app.ui().cx(),
                Level {
                    peak,
                    rms: peak * RMS_SHARE,
                },
            );
            let ready: Vec<_> = tasks.try_iter().collect();
            for (id, future) in ready {
                if let Ok(output) =
                    timers.block_on(async { tokio::time::timeout(TASK_DEADLINE, future).await })
                {
                    self.app.ui().complete_task(id, output);
                }
            }
            self.app.advance_ms(LIVE_TICK_MILLIS);
        }
        let cx = self.app.ui().cx();
        self.state.take.elapsed.set(cx, RECORDED_SO_FAR);
        self.level.set(
            cx,
            Level {
                peak: 0.32,
                rms: 0.32 * RMS_SHARE,
            },
        );
        self.app.settle();
        Ok(())
    }
}

fn main() -> Result<()> {
    let mut carrier = Carrier::new(
        "Sound Recorder",
        "crownos-sound-recorder: the Voice Recorder window with its library, recorder, playback, rename and delete flows, pickers and error states, captured headlessly from the current source.",
    );
    let fixtures = tempfile::tempdir()?;
    let samples = record_samples(fixtures.path())?;

    {
        let mut window = Window::standard(&samples)?;
        carrier.capture_state(
            &mut window.app,
            "Recorder",
            "Ready to record",
            "New Recording selected, microphone live and enhanced",
        )?;
    }
    {
        let mut window = Window::standard(&samples)?;
        window.record_take()?;
        carrier.capture_state(
            &mut window.app,
            "Recorder",
            "Recording",
            "A take in progress: live waveform, level meter, Pause and Stop",
        )?;
        window.app.click(role(Role::Button).name("Pause"));
        window.app.settle();
        carrier.capture_state(
            &mut window.app,
            "Recorder",
            "Paused",
            "The take paused: Resume on the record button",
        )?;
    }
    {
        let mut window = Window::open(
            &samples,
            Enhancement::NoiseSuppression,
            StreamState::Starting,
        )?;
        carrier.capture_state(
            &mut window.app,
            "Recorder",
            "Starting microphone",
            "The input stream is still connecting",
        )?;
    }
    {
        let raw = StreamState::Live { enhanced_by: None };
        let mut window = Window::open(&samples, Enhancement::Off, raw)?;
        carrier.capture_state(
            &mut window.app,
            "Recorder",
            "Enhancement off",
            "Recording the raw microphone",
        )?;
    }

    {
        let mut window = Window::standard(&samples)?;
        window.show_featured();
        carrier.capture_state(
            &mut window.app,
            "Recording",
            "Playback",
            "A saved recording: waveform seek bar, play, enhance",
        )?;
        window.app.click(role(Role::Button).name("Rename"));
        window.app.click(role(Role::TextField).name("Title"));
        window.app.press_key("Ctrl+A");
        window.app.type_text("Interview with Priya — final");
        window.app.settle();
        carrier.capture_state(
            &mut window.app,
            "Recording",
            "Rename",
            "Renaming the recording in place",
        )?;
        window.app.press_key("Ctrl+A");
        window.app.type_text("Interview/Priya");
        window.app.press_key("Enter");
        window.app.settle();
        carrier.capture_state(
            &mut window.app,
            "Recording",
            "Rename error",
            "The new name was refused",
        )?;
    }
    {
        let mut window = Window::standard(&samples)?;
        window.show_featured();
        window.app.click(role(Role::Button).name("Delete"));
        window.app.settle();
        carrier.capture_state(
            &mut window.app,
            "Recording",
            "Delete confirmation",
            "Asks before moving the recording to the trash",
        )?;
    }

    for picker in ["Input", "Enhancement", "Format"] {
        let mut window = Window::standard(&samples)?;
        window.app.click(role(Role::ComboBox).name(picker));
        window.app.settle();
        carrier.capture_state(
            &mut window.app,
            "Menus & popovers",
            &format!("{picker} picker"),
            &format!("The recorder's {picker} dropdown open"),
        )?;
    }
    {
        let mut window = Window::standard(&samples)?;
        window.show_featured();
        window.app.click(role(Role::ComboBox).name("Enhancement"));
        window.app.settle();
        carrier.capture_state(
            &mut window.app,
            "Menus & popovers",
            "Enhance copy picker",
            "Choosing how to enhance a saved recording",
        )?;
    }

    {
        let mut window = Window::standard(&[])?;
        carrier.capture_state(
            &mut window.app,
            "Empty & error states",
            "Empty library",
            "First launch: no recordings yet",
        )?;
    }
    {
        let failed = StreamState::Failed("no input device is connected".to_owned());
        let mut window = Window::open(&samples, Enhancement::NoiseSuppression, failed)?;
        carrier.capture_state(
            &mut window.app,
            "Empty & error states",
            "No microphone",
            "The microphone could not be opened",
        )?;
        let state = window.state;
        state.report(window.app.ui().cx(), "The microphone is not available.");
        window.app.settle();
        carrier.capture_state(
            &mut window.app,
            "Empty & error states",
            "Record without microphone",
            "Pressing Record with no microphone raises a notice",
        )?;
    }
    {
        let unavailable = UnenhancedReason::Unavailable("connection refused".to_owned());
        let mut window = Window::open(
            &samples,
            Enhancement::NoiseSuppression,
            StreamState::Unenhanced(unavailable),
        )?;
        carrier.capture_state(
            &mut window.app,
            "Empty & error states",
            "Enhancement unavailable",
            "crown-voiced is down, so the take records raw",
        )?;
    }
    {
        let mut window = Window::standard(&samples)?;
        let state = window.state;
        state.report(
            window.app.ui().cx(),
            "Preferences were not saved: Permission denied (os error 13)",
        );
        window.app.settle();
        carrier.capture_state(
            &mut window.app,
            "Empty & error states",
            "Error notice",
            "A dismissable warning above the content",
        )?;
    }
    {
        let mut window = Window::standard(&samples)?;
        let state = window.state;
        let gone = PathBuf::from("/home/user/Music/Recordings/Old memo.ogg");
        state
            .selection
            .set(window.app.ui().cx(), Selection::Recording(gone));
        window.app.settle();
        carrier.capture_state(
            &mut window.app,
            "Empty & error states",
            "Missing recording",
            "The selected file left the library",
        )?;
    }

    let path = carrier.write(None)?;
    println!("{}", path.display());
    capture::finish()
}

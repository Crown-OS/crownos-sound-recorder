use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use crownui::prelude::{Cx, Runtime, Signal, Store};

use super::FrameFeed;
use crate::encode::RecordingFormat;
use crate::recording::RecordingSession;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TakePhase {
    Idle,
    Recording,
    Paused,
}

impl TakePhase {
    /// What pressing the record button does next.
    pub const fn action(self) -> &'static str {
        match self {
            Self::Idle => "Record",
            Self::Recording => "Pause",
            Self::Paused => "Resume",
        }
    }
}

/// The recording in progress, if any.
#[derive(Debug, Clone, Copy)]
pub struct Take {
    session: Store<Option<RecordingSession>>,
    pub phase: Signal<TakePhase>,
    pub elapsed: Signal<Duration>,
}

impl Take {
    pub fn new(cx: &mut Cx) -> Self {
        Self {
            session: cx.store(None),
            phase: cx.signal(TakePhase::Idle),
            elapsed: cx.signal(Duration::ZERO),
        }
    }

    pub fn is_active(self, runtime: &mut Runtime) -> bool {
        self.phase.get(runtime) != TakePhase::Idle
    }

    pub fn begin(
        self,
        runtime: &mut Runtime,
        path: PathBuf,
        format: RecordingFormat,
        feed: FrameFeed,
    ) -> Result<()> {
        let session = RecordingSession::start(path, format, feed)?;
        self.session.update(runtime, |slot| *slot = Some(session));
        self.elapsed.set(runtime, Duration::ZERO);
        self.phase.set(runtime, TakePhase::Recording);
        Ok(())
    }

    pub fn toggle_pause(self, runtime: &mut Runtime) {
        let next = match self.phase.get(runtime) {
            TakePhase::Recording => TakePhase::Paused,
            TakePhase::Paused => TakePhase::Recording,
            TakePhase::Idle => return,
        };
        self.session.with(runtime, |session| match (session, next) {
            (Some(session), TakePhase::Paused) => session.pause(),
            (Some(session), _) => session.resume(),
            (None, _) => {}
        });
        self.phase.set(runtime, next);
    }

    /// Copies the recorded time, excluding pauses, into [`Self::elapsed`].
    pub fn sample_elapsed(self, runtime: &mut Runtime) {
        if let Some(elapsed) = self.session.with(runtime, |session| {
            session.as_ref().map(RecordingSession::elapsed)
        }) {
            self.elapsed.set(runtime, elapsed);
        }
    }

    /// Finalizes the file and returns where it is; `None` when nothing was recording.
    pub fn finish(self, runtime: &mut Runtime) -> Option<Result<PathBuf>> {
        let session = self.session.try_update(runtime, Option::take).flatten()?;
        self.phase.try_set(runtime, TakePhase::Idle);
        let path = session.path().to_path_buf();
        Some(session.stop().map(|_| path))
    }
}

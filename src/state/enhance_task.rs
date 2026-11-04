use std::ops::ControlFlow;
use std::path::Path;
use std::time::Duration;

use audio::Enhancement;
use crownui::prelude::{Cx, Signal, Store};

use super::{AppState, Selection};
use crate::recording::{EnhanceJob, EnhanceOutcome, VoiceService};
use crate::util::repeat_every;

const POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnhancePhase {
    Idle,
    Running,
    Failed(String),
}

/// An offline enhancement of the recording on screen; dropped with it, which cancels it.
#[derive(Debug, Clone, Copy)]
pub struct EnhanceTask {
    job: Store<Option<EnhanceJob>>,
    pub phase: Signal<EnhancePhase>,
    pub progress: Signal<f32>,
}

impl EnhanceTask {
    pub fn new(cx: &mut Cx) -> Self {
        Self {
            job: cx.store(None),
            phase: cx.signal(EnhancePhase::Idle),
            progress: cx.signal(0.0),
        }
    }

    pub fn start(self, cx: &mut Cx, state: AppState, source: &Path, enhancement: Enhancement) {
        match EnhanceJob::start(source, enhancement, VoiceService::Session) {
            Ok(job) => {
                self.job.update(cx, |slot| *slot = Some(job));
                self.progress.set(cx, 0.0);
                self.phase.set(cx, EnhancePhase::Running);
                repeat_every(cx, POLL_INTERVAL, move |cx| self.poll(cx, state));
            }
            Err(error) => self.phase.set(cx, EnhancePhase::Failed(error.to_string())),
        }
    }

    pub fn cancel(self, cx: &mut Cx) {
        self.job
            .with(cx, |job| job.as_ref().map(EnhanceJob::cancel));
    }

    fn poll(self, cx: &mut Cx, state: AppState) -> ControlFlow<()> {
        let Some(Some(finished)) = self
            .job
            .try_update(cx, |job| job.as_ref().map(EnhanceJob::is_finished))
        else {
            return ControlFlow::Break(());
        };
        if !finished {
            let progress = self
                .job
                .with(cx, |job| job.as_ref().map_or(0.0, EnhanceJob::progress));
            self.progress.set(cx, progress);
            return ControlFlow::Continue(());
        }
        match self.job.update(cx, Option::take).map(EnhanceJob::wait) {
            Some(Ok(EnhanceOutcome::Finished(enhanced))) => {
                state.show(cx, Selection::Recording(enhanced));
            }
            Some(Ok(EnhanceOutcome::Cancelled)) | None => self.phase.set(cx, EnhancePhase::Idle),
            Some(Err(error)) => self.phase.set(cx, EnhancePhase::Failed(error.to_string())),
        }
        ControlFlow::Break(())
    }
}

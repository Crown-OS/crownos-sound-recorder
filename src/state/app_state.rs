use std::fmt::Display;
use std::path::Path;

use crownui::prelude::{Cx, Runtime, Signal, Store};

use super::{Capture, Environment, Selection, Take};
use crate::recording::{Library, Recording, RenameError};
use crate::settings::{Persistence, Preferences};

/// The window's state, shared by every view.
#[derive(Debug, Clone, Copy)]
pub struct AppState {
    library: Store<Library>,
    pub recordings: Signal<Vec<Recording>>,
    pub selection: Signal<Selection>,
    pub preferences: Signal<Preferences>,
    pub notice: Signal<Option<String>>,
    pub take: Take,
    pub capture: Capture,
    persistence: Persistence,
}

impl AppState {
    pub fn new(cx: &mut Cx, environment: Environment) -> Self {
        let Environment {
            library,
            preferences,
            capture,
            persistence,
        } = environment;
        let state = Self {
            recordings: cx.signal(Vec::new()),
            library: cx.store(library),
            selection: cx.signal(Selection::Recorder),
            preferences: cx.signal(preferences),
            notice: cx.signal(None),
            take: Take::new(cx),
            capture,
            persistence,
        };
        state.refresh(cx);
        state
    }

    pub fn with_library<R>(self, runtime: &Runtime, read: impl FnOnce(&Library) -> R) -> R {
        self.library.with(runtime, read)
    }

    pub fn recording(self, runtime: &Runtime, path: &Path) -> Option<Recording> {
        self.recordings.with_untracked(runtime, |recordings| {
            recordings
                .iter()
                .find(|recording| recording.path == path)
                .cloned()
        })
    }

    pub fn refresh(self, runtime: &mut Runtime) {
        match self.with_library(runtime, Library::list) {
            Ok(recordings) => self.recordings.set(runtime, recordings),
            Err(error) => self.report(runtime, format!("{error:#}")),
        }
    }

    /// Switches the content pane, saving a take in progress first.
    pub fn show(self, runtime: &mut Runtime, selection: Selection) {
        if self
            .selection
            .with(runtime, |current| *current == selection)
        {
            return;
        }
        self.notice.set(runtime, None);
        if let Some(Err(error)) = self.take.finish(runtime) {
            self.report(runtime, format!("{error:#}"));
        }
        self.refresh(runtime);
        self.selection.set(runtime, selection);
    }

    pub fn rename(
        self,
        runtime: &mut Runtime,
        recording: &Recording,
        title: &str,
    ) -> Result<(), RenameError> {
        let renamed = self.with_library(runtime, |library| library.rename(recording, title))?;
        self.show(runtime, Selection::Recording(renamed.path));
        Ok(())
    }

    /// Moves `recording` to the trash and selects its neighbour.
    pub fn delete(self, runtime: &mut Runtime, recording: &Recording) {
        if let Err(error) = self.with_library(runtime, |library| library.delete(recording)) {
            self.report(runtime, format!("{error:#}"));
            return;
        }
        let neighbour = self.recordings.with(runtime, |recordings| {
            let index = recordings
                .iter()
                .position(|listed| listed.path == recording.path)?;
            recordings
                .get(index + 1)
                .or_else(|| {
                    index
                        .checked_sub(1)
                        .and_then(|before| recordings.get(before))
                })
                .map(|listed| listed.path.clone())
        });
        self.show(
            runtime,
            neighbour.map_or(Selection::Recorder, Selection::Recording),
        );
    }

    /// Saves the take in progress and selects it in the library.
    pub fn finish_take(self, runtime: &mut Runtime) {
        match self.take.finish(runtime) {
            Some(Ok(path)) => self.show(runtime, Selection::Recording(path)),
            Some(Err(error)) => self.report(runtime, format!("{error:#}")),
            None => {}
        }
    }

    pub fn update_preferences(self, runtime: &mut Runtime, change: impl FnOnce(&mut Preferences)) {
        self.preferences.update(runtime, change);
        if self.persistence == Persistence::Config
            && let Err(error) = self.preferences.with(runtime, Preferences::save)
        {
            self.report(runtime, format!("Preferences were not saved: {error}"));
        }
    }

    pub fn report(self, runtime: &mut Runtime, message: impl Display) {
        self.notice.set(runtime, Some(message.to_string()));
    }
}

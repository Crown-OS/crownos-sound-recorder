use audio::StreamState;
use crownui::kit::{StatusOptions, Tone, status};
use crownui::prelude::{Signal, View, keyed};

const UNAVAILABLE: &str = "Enhancement unavailable — recording raw";

/// How the microphone is being enhanced, as the stream reports it.
pub fn microphone_status(state: Signal<StreamState>) -> impl View {
    keyed(
        move |runtime| state.with(runtime, describe),
        |_, (tone, label)| status(label).tone(tone),
    )
}

fn describe(state: &StreamState) -> (Tone, String) {
    match state {
        StreamState::Starting => (Tone::Neutral, "Starting the microphone…".into()),
        StreamState::Live {
            enhanced_by: Some(backend),
        } => (Tone::Success, format!("Enhanced by {backend}")),
        StreamState::Live { enhanced_by: None } => {
            (Tone::Neutral, "Enhancement off — recording raw".into())
        }
        StreamState::Unenhanced(_) => (Tone::Warning, UNAVAILABLE.into()),
        StreamState::Failed(reason) => (Tone::Danger, format!("Microphone unavailable: {reason}")),
    }
}

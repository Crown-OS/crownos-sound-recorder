use audio::{Enhancement, Level, MicrophoneOptions, StreamState};
use crownui::kit::{MicrophoneSignals, use_microphone};
use crownui::prelude::{Cx, Signal};

use super::{Capture, FrameFeed};

/// The recorder's input: a live microphone, open while the calling component is mounted, or a
/// synthetic tone.
#[derive(Debug, Clone, Copy)]
pub enum MicrophoneInput {
    Live(MicrophoneSignals),
    Synthetic {
        level: Signal<Level>,
        state: Signal<StreamState>,
    },
}

impl MicrophoneInput {
    pub fn open(cx: &mut Cx, capture: Capture, options: MicrophoneOptions) -> Self {
        match capture {
            Capture::Microphone => Self::Live(use_microphone(cx, options)),
            Capture::Tone => Self::Synthetic {
                level: cx.signal(Level::default()),
                state: cx.signal(StreamState::Live { enhanced_by: None }),
            },
        }
    }

    pub fn level(self) -> Signal<Level> {
        match self {
            Self::Live(microphone) => microphone.level,
            Self::Synthetic { level, .. } => level,
        }
    }

    pub fn state(self) -> Signal<StreamState> {
        match self {
            Self::Live(microphone) => microphone.state,
            Self::Synthetic { state, .. } => state,
        }
    }

    /// The frames for one take; a live microphone hands its ring over once.
    pub fn take_feed(self, cx: &mut Cx) -> Option<FrameFeed> {
        match self {
            Self::Live(microphone) => microphone.take_frames(cx).map(FrameFeed::microphone),
            Self::Synthetic { .. } => Some(FrameFeed::tone()),
        }
    }

    pub fn set_enhancement(self, cx: &mut Cx, enhancement: Enhancement) {
        if let Self::Live(microphone) = self {
            microphone.set_enhancement(cx, enhancement);
        }
    }
}

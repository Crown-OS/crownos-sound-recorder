use audio::FrameConsumer;

use crate::recording::{Frame, FrameSource, ToneGenerator};

const TONE_HZ: f32 = 440.0;
const TONE_SECONDS: f32 = 1.0;

/// Where the recorder's audio comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capture {
    Microphone,
    /// A short sine tone and a silent level, so the window runs without PipeWire.
    Tone,
}

/// The frames one take records.
pub enum FrameFeed {
    Microphone(FrameConsumer),
    Tone(ToneGenerator),
}

impl FrameFeed {
    pub fn tone() -> Self {
        Self::Tone(ToneGenerator::with_seconds(TONE_HZ, TONE_SECONDS))
    }

    /// Starts from the live edge, dropping what queued while nobody was recording.
    pub fn microphone(mut frames: FrameConsumer) -> Self {
        if let Ok(queued) = frames.read_chunk(frames.slots()) {
            queued.commit_all();
        }
        Self::Microphone(frames)
    }
}

impl FrameSource for FrameFeed {
    fn pop_frame(&mut self) -> Option<Frame> {
        match self {
            Self::Microphone(frames) => frames.pop_frame(),
            Self::Tone(tone) => tone.pop_frame(),
        }
    }
}

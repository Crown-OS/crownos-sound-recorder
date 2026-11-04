use std::f32::consts::TAU;

use super::{FRAME_SAMPLES, Frame, FrameSource};
use crate::encode::SAMPLE_RATE;

const AMPLITUDE: f32 = 0.5;

/// A finite sine tone, for smoke tests and examples.
#[derive(Debug, Clone)]
pub struct ToneGenerator {
    phase: f32,
    phase_step: f32,
    frames_left: usize,
}

impl ToneGenerator {
    pub fn new(frequency_hz: f32, frames: usize) -> Self {
        Self {
            phase: 0.0,
            phase_step: TAU * frequency_hz / SAMPLE_RATE as f32,
            frames_left: frames,
        }
    }

    pub fn with_seconds(frequency_hz: f32, seconds: f32) -> Self {
        let frames = seconds * SAMPLE_RATE as f32 / FRAME_SAMPLES as f32;
        Self::new(frequency_hz, frames.round() as usize)
    }
}

impl FrameSource for ToneGenerator {
    fn pop_frame(&mut self) -> Option<Frame> {
        self.frames_left = self.frames_left.checked_sub(1)?;
        Some(std::array::from_fn(|_| {
            let sample = self.phase.sin() * AMPLITUDE;
            self.phase = (self.phase + self.phase_step) % TAU;
            sample
        }))
    }
}

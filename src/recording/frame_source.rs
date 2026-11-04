pub const FRAME_SAMPLES: usize = audio::FRAME_SAMPLES;

/// 10 ms of mono audio at 48 kHz.
pub type Frame = [f32; FRAME_SAMPLES];

/// Where a recording session pulls microphone frames from, without blocking.
pub trait FrameSource {
    fn pop_frame(&mut self) -> Option<Frame>;
}

impl FrameSource for audio::FrameConsumer {
    fn pop_frame(&mut self) -> Option<Frame> {
        self.pop().ok().map(|frame| frame.samples)
    }
}

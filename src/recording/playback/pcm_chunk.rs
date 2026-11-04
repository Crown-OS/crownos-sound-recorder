use std::sync::atomic::{AtomicU32, AtomicU64};

pub const CHUNK_SAMPLES: usize = 480;

/// Decoded samples tagged with the seek epoch they belong to and their place in the track.
#[derive(Clone, Copy)]
pub struct PcmChunk {
    pub epoch: u32,
    pub start: u64,
    pub len: usize,
    pub samples: [f32; CHUNK_SAMPLES],
}

/// Shared between the decoder and the real-time callback without locks.
#[derive(Default)]
pub struct PlaybackClock {
    pub epoch: AtomicU32,
    pub position: AtomicU64,
}

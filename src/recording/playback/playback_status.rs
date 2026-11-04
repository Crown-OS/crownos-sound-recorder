use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Paused,
    Playing,
    Ended,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaybackStatus {
    pub state: PlaybackState,
    pub position: Duration,
    pub duration: Duration,
}

impl PlaybackStatus {
    pub fn fraction(&self) -> f64 {
        if self.duration.is_zero() {
            0.0
        } else {
            (self.position.as_secs_f64() / self.duration.as_secs_f64()).min(1.0)
        }
    }
}

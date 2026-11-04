use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{Context, Result};
use rtrb::RingBuffer;
use tokio::sync::watch;

use super::output_stream::OutputStream;
use super::pcm_chunk::PlaybackClock;
use super::player_worker::{PlayerCommand, PlayerWorker};
use super::{PlaybackState, PlaybackStatus};
use crate::decode::PcmDecoder;

const RING_CHUNKS: usize = 50;

/// Plays one audio file through PipeWire; the UI drives it and watches [`PlaybackStatus`].
pub struct Player {
    commands: mpsc::Sender<PlayerCommand>,
    status: watch::Receiver<PlaybackStatus>,
    worker: Option<JoinHandle<()>>,
}

impl Player {
    /// Opens `path` paused at the start.
    pub fn open(path: &Path) -> Result<Self> {
        let (producer, consumer) = RingBuffer::new(RING_CHUNKS);
        let clock = Arc::new(PlaybackClock::default());
        let output = OutputStream::start(consumer, Arc::clone(&clock))?;
        let (status_sender, status) = watch::channel(PlaybackStatus {
            state: PlaybackState::Paused,
            position: Duration::ZERO,
            duration: Duration::ZERO,
        });
        let (commands, command_receiver) = mpsc::channel();
        let (opened_sender, opened) = mpsc::sync_channel(1);
        let path = PathBuf::from(path);
        let worker = thread::Builder::new()
            .name("playback".into())
            .spawn(move || {
                let decoder = match PcmDecoder::open(&path) {
                    Ok(decoder) => decoder,
                    Err(error) => {
                        let _ = opened_sender.send(Err(error));
                        return;
                    }
                };
                status_sender.send_modify(|status| status.duration = decoder.duration());
                let _ = opened_sender.send(Ok(()));
                PlayerWorker::new(decoder, producer, clock, output, status_sender)
                    .run(&command_receiver);
            })?;
        opened.recv().context("playback thread exited")??;
        Ok(Self {
            commands,
            status,
            worker: Some(worker),
        })
    }

    pub fn play(&self) {
        self.send(PlayerCommand::Play);
    }

    pub fn pause(&self) {
        self.send(PlayerCommand::Pause);
    }

    /// Jumps to `fraction` (0.0 to 1.0) of the duration.
    pub fn seek(&self, fraction: f64) {
        self.send(PlayerCommand::Seek(fraction));
    }

    /// A receiver the UI can poll or `.changed().await`.
    pub fn status(&self) -> watch::Receiver<PlaybackStatus> {
        self.status.clone()
    }

    pub fn state(&self) -> PlaybackState {
        self.status.borrow().state
    }

    pub fn position(&self) -> Duration {
        self.status.borrow().position
    }

    pub fn duration(&self) -> Duration {
        self.status.borrow().duration
    }

    pub fn is_ended(&self) -> bool {
        self.state() == PlaybackState::Ended
    }

    fn send(&self, command: PlayerCommand) {
        let _ = self.commands.send(command);
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.send(PlayerCommand::Quit);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;
    use crate::encode::RecordingFormat;
    use crate::recording::{RecordingSession, ToneGenerator};

    fn wait_until(
        player: &Player,
        limit: Duration,
        done: impl Fn(&PlaybackStatus) -> bool,
    ) -> bool {
        let deadline = Instant::now() + limit;
        while Instant::now() < deadline {
            if done(&player.status().borrow()) {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        false
    }

    #[test]
    #[ignore = "needs a running PipeWire session"]
    fn plays_pauses_and_seeks_through_pipewire() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("tone.flac");
        RecordingSession::start(
            &path,
            RecordingFormat::Flac,
            ToneGenerator::with_seconds(440.0, 4.0),
        )?
        .stop()?;
        let player = Player::open(&path)?;
        assert_eq!(player.duration(), Duration::from_secs(4));
        player.play();
        assert!(wait_until(&player, Duration::from_secs(2), |status| status
            .position
            > Duration::from_millis(300)));
        player.pause();
        thread::sleep(Duration::from_millis(100));
        let paused_at = player.position();
        thread::sleep(Duration::from_millis(300));
        assert_eq!(player.position(), paused_at);
        player.seek(0.75);
        assert!(wait_until(&player, Duration::from_secs(1), |status| status
            .position
            == Duration::from_secs(3)));
        player.play();
        assert!(wait_until(&player, Duration::from_secs(3), |status| status
            .state
            == PlaybackState::Ended));
        Ok(())
    }
}

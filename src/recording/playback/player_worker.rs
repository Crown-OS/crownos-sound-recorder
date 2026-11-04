use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use anyhow::Result;
use rtrb::Producer;
use tokio::sync::watch;

use super::output_stream::OutputStream;
use super::pcm_chunk::{CHUNK_SAMPLES, PcmChunk, PlaybackClock};
use super::{PlaybackState, PlaybackStatus};
use crate::decode::PcmDecoder;
use crate::util::sample_time::{duration_to_samples, samples_to_duration};

const IDLE_WAIT: Duration = Duration::from_millis(10);

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlayerCommand {
    Play,
    Pause,
    Seek(f64),
    Quit,
}

/// Keeps the chunk ring full, applies commands and publishes status.
pub struct PlayerWorker {
    decoder: PcmDecoder,
    chunks: Producer<PcmChunk>,
    clock: Arc<PlaybackClock>,
    output: OutputStream,
    status: watch::Sender<PlaybackStatus>,
    state: PlaybackState,
    epoch: u32,
    next_start: u64,
    decoded_to_end: bool,
}

impl PlayerWorker {
    pub fn new(
        decoder: PcmDecoder,
        chunks: Producer<PcmChunk>,
        clock: Arc<PlaybackClock>,
        output: OutputStream,
        status: watch::Sender<PlaybackStatus>,
    ) -> Self {
        Self {
            decoder,
            chunks,
            clock,
            output,
            status,
            state: PlaybackState::Paused,
            epoch: 0,
            next_start: 0,
            decoded_to_end: false,
        }
    }

    pub fn run(mut self, commands: &Receiver<PlayerCommand>) {
        if let Err(error) = self.serve(commands) {
            eprintln!("playback failed: {error:#}");
            self.state = PlaybackState::Failed;
            self.publish();
        }
    }

    fn serve(&mut self, commands: &Receiver<PlayerCommand>) -> Result<()> {
        loop {
            let wait = if self.wants_samples() {
                Duration::ZERO
            } else {
                IDLE_WAIT
            };
            match commands.recv_timeout(wait) {
                Ok(PlayerCommand::Quit) | Err(RecvTimeoutError::Disconnected) => return Ok(()),
                Ok(command) => self.apply(command)?,
                Err(RecvTimeoutError::Timeout) => {}
            }
            self.fill_ring()?;
            self.detect_end();
            self.publish();
        }
    }

    fn apply(&mut self, command: PlayerCommand) -> Result<()> {
        match command {
            PlayerCommand::Play => {
                if self.state == PlaybackState::Ended {
                    self.seek_to(Duration::ZERO)?;
                }
                self.state = PlaybackState::Playing;
                self.output.resume();
            }
            PlayerCommand::Pause => {
                if self.state == PlaybackState::Playing {
                    self.state = PlaybackState::Paused;
                }
                self.output.pause();
            }
            PlayerCommand::Seek(fraction) => {
                let duration = self.status.borrow().duration;
                self.seek_to(duration.mul_f64(fraction.clamp(0.0, 1.0)))?;
                if self.state == PlaybackState::Ended {
                    self.state = PlaybackState::Paused;
                }
            }
            PlayerCommand::Quit => {}
        }
        Ok(())
    }

    fn seek_to(&mut self, position: Duration) -> Result<()> {
        self.decoder.seek(position)?;
        self.epoch = self.epoch.wrapping_add(1);
        self.next_start = duration_to_samples(position);
        self.decoded_to_end = false;
        self.clock
            .position
            .store(self.next_start, Ordering::Relaxed);
        self.clock.epoch.store(self.epoch, Ordering::Release);
        Ok(())
    }

    fn wants_samples(&self) -> bool {
        !self.decoded_to_end && self.chunks.slots() > 0
    }

    fn fill_ring(&mut self) -> Result<()> {
        while self.wants_samples() {
            let mut chunk = PcmChunk {
                epoch: self.epoch,
                start: self.next_start,
                len: 0,
                samples: [0.0; CHUNK_SAMPLES],
            };
            chunk.len = self.decoder.read(&mut chunk.samples)?;
            self.decoded_to_end = chunk.len < CHUNK_SAMPLES;
            if chunk.len > 0 {
                self.next_start += chunk.len as u64;
                let _ = self.chunks.push(chunk);
            }
        }
        Ok(())
    }

    fn detect_end(&mut self) {
        let ring_drained = self.chunks.slots() == self.chunks.buffer().capacity();
        if self.state == PlaybackState::Playing && self.decoded_to_end && ring_drained {
            self.state = PlaybackState::Ended;
            self.output.pause();
        }
    }

    fn publish(&self) {
        let position = match self.state {
            PlaybackState::Ended => self.status.borrow().duration,
            _ => samples_to_duration(self.clock.position.load(Ordering::Relaxed)),
        };
        self.status.send_if_modified(|status| {
            let changed = status.state != self.state || status.position != position;
            status.state = self.state;
            status.position = position;
            changed
        });
    }
}

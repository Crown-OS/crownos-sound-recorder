use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use crownui::prelude::{Cx, Signal, Store};

use crate::recording::{PlaybackState, PlaybackStatus, Player};
use crate::util::follow_watch;

/// One recording's player, opened on the first play or seek.
#[derive(Debug, Clone, Copy)]
pub struct Playback {
    player: Store<Option<Player>>,
    source: Store<PathBuf>,
    pub status: Signal<PlaybackStatus>,
}

impl Playback {
    pub fn new(cx: &mut Cx, source: PathBuf, duration: Duration) -> Self {
        Self {
            player: cx.store(None),
            source: cx.store(source),
            status: cx.signal(PlaybackStatus {
                state: PlaybackState::Paused,
                position: Duration::ZERO,
                duration,
            }),
        }
    }

    pub fn toggle(self, cx: &mut Cx) -> Result<()> {
        self.open(cx)?;
        let state = self.status.with(cx, |status| status.state);
        self.with_player(cx, |player| match state {
            PlaybackState::Playing => player.pause(),
            PlaybackState::Ended => {
                player.seek(0.0);
                player.play();
            }
            PlaybackState::Paused | PlaybackState::Failed => player.play(),
        });
        Ok(())
    }

    pub fn seek(self, cx: &mut Cx, fraction: f32) -> Result<()> {
        self.open(cx)?;
        self.with_player(cx, |player| player.seek(f64::from(fraction)));
        Ok(())
    }

    fn open(self, cx: &mut Cx) -> Result<()> {
        if self.player.with(cx, Option::is_some) {
            return Ok(());
        }
        let player = self.source.with(cx, |source| Player::open(source))?;
        let status = player.status();
        self.status.set(cx, *status.borrow());
        self.player.update(cx, |slot| *slot = Some(player));
        follow_watch(cx, self.status, status);
        Ok(())
    }

    fn with_player(self, cx: &mut Cx, act: impl FnOnce(&Player)) {
        self.player.with(cx, |player| player.as_ref().map(act));
    }
}

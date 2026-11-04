use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use ffmpeg_next::{format, media};

use crate::util::ffmpeg::init_ffmpeg;

const MICROS_PER_SECOND: f64 = 1_000_000.0;

/// What the container says about a file's audio, read without decoding it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioProbe {
    pub duration: Duration,
    pub sample_rate: u32,
    pub channels: u16,
}

pub fn probe_audio(path: &Path) -> Result<AudioProbe> {
    init_ffmpeg()?;
    let input = format::input(path).with_context(|| format!("opening {}", path.display()))?;
    let stream = input
        .streams()
        .best(media::Type::Audio)
        .with_context(|| format!("{} has no audio", path.display()))?;
    let parameters = stream.parameters();
    let decoder = ffmpeg_next::codec::Context::from_parameters(parameters)?
        .decoder()
        .audio()?;
    Ok(AudioProbe {
        duration: container_duration(&input),
        sample_rate: decoder.rate(),
        channels: decoder.channels(),
    })
}

pub(super) fn container_duration(input: &format::context::Input) -> Duration {
    let micros = input.duration();
    if micros > 0 {
        return Duration::from_secs_f64(micros as f64 / MICROS_PER_SECOND);
    }
    input
        .streams()
        .best(media::Type::Audio)
        .filter(|stream| stream.duration() > 0)
        .map_or(Duration::ZERO, |stream| {
            Duration::from_secs_f64(stream.duration() as f64 * f64::from(stream.time_base()))
        })
}

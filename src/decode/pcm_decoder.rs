use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use ffmpeg_next::format::{Sample, sample};
use ffmpeg_next::software::resampling;
use ffmpeg_next::util::error::EAGAIN;
use ffmpeg_next::{ChannelLayout, Packet, codec, decoder, format, frame, media};

use super::probe::container_duration;
use crate::encode::SAMPLE_RATE;
use crate::util::ffmpeg::init_ffmpeg;

const PACKED_FLOAT: Sample = Sample::F32(sample::Type::Packed);
const RESAMPLED_CAPACITY: usize = 16_384;

/// Decodes any audio file to mono 48 kHz `f32`, read like a stream.
pub struct PcmDecoder {
    input: format::context::Input,
    decoder: decoder::Audio,
    stream_index: usize,
    resampler: Option<resampling::Context>,
    packet: Packet,
    decoded: frame::Audio,
    resampled: frame::Audio,
    read_offset: usize,
    end_of_input_sent: bool,
    duration: Duration,
}

impl PcmDecoder {
    pub fn open(path: &Path) -> Result<Self> {
        init_ffmpeg()?;
        let input = format::input(path).with_context(|| format!("opening {}", path.display()))?;
        let stream = input
            .streams()
            .best(media::Type::Audio)
            .with_context(|| format!("{} has no audio", path.display()))?;
        let stream_index = stream.index();
        let decoder = codec::Context::from_parameters(stream.parameters())?
            .decoder()
            .audio()?;
        let duration = container_duration(&input);
        let mut resampled =
            frame::Audio::new(PACKED_FLOAT, RESAMPLED_CAPACITY, ChannelLayout::MONO);
        resampled.set_samples(0);
        Ok(Self {
            input,
            decoder,
            stream_index,
            resampler: None,
            packet: Packet::empty(),
            decoded: frame::Audio::empty(),
            resampled,
            read_offset: 0,
            end_of_input_sent: false,
            duration,
        })
    }

    pub fn duration(&self) -> Duration {
        self.duration
    }

    /// Fills `samples` and returns how many were written; fewer than asked means the end.
    pub fn read(&mut self, samples: &mut [f32]) -> Result<usize> {
        let mut written = 0;
        while written < samples.len() {
            let buffered = self.resampled.samples();
            if self.read_offset == buffered {
                if !self.refill()? {
                    break;
                }
                continue;
            }
            let available = &self.resampled.plane::<f32>(0)[self.read_offset..buffered];
            let count = available.len().min(samples.len() - written);
            samples[written..written + count].copy_from_slice(&available[..count]);
            written += count;
            self.read_offset += count;
        }
        Ok(written)
    }

    pub fn seek(&mut self, position: Duration) -> Result<()> {
        let timestamp = i64::try_from(position.as_micros()).unwrap_or(i64::MAX);
        self.input.seek(timestamp, ..timestamp)?;
        self.decoder.flush();
        self.resampler = None;
        self.resampled.set_samples(0);
        self.read_offset = 0;
        self.end_of_input_sent = false;
        Ok(())
    }

    fn refill(&mut self) -> Result<bool> {
        self.read_offset = 0;
        loop {
            match self.decoder.receive_frame(&mut self.decoded) {
                Ok(()) => {
                    self.resample_decoded()?;
                    if self.resampled.samples() > 0 {
                        return Ok(true);
                    }
                }
                Err(ffmpeg_next::Error::Other { errno: EAGAIN }) => self.send_next_packet()?,
                Err(ffmpeg_next::Error::Eof) => return self.flush_resampler(),
                Err(error) => return Err(error.into()),
            }
        }
    }

    fn send_next_packet(&mut self) -> Result<()> {
        loop {
            match self.packet.read(&mut self.input) {
                Ok(()) if self.packet.stream() == self.stream_index => {
                    return Ok(self.decoder.send_packet(&self.packet)?);
                }
                Ok(()) => {}
                Err(ffmpeg_next::Error::Eof) if !self.end_of_input_sent => {
                    self.end_of_input_sent = true;
                    return Ok(self.decoder.send_eof()?);
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    fn resample_decoded(&mut self) -> Result<()> {
        let decoded = &self.decoded;
        let resampler = match &mut self.resampler {
            Some(resampler) => resampler,
            empty => empty.insert(resampling::Context::get(
                decoded.format(),
                source_layout(decoded),
                decoded.rate(),
                PACKED_FLOAT,
                ChannelLayout::MONO,
                SAMPLE_RATE,
            )?),
        };
        self.resampled.set_samples(0);
        resampler.run(decoded, &mut self.resampled)?;
        Ok(())
    }

    fn flush_resampler(&mut self) -> Result<bool> {
        let Some(resampler) = &mut self.resampler else {
            return Ok(false);
        };
        self.resampled.set_samples(0);
        resampler.flush(&mut self.resampled)?;
        Ok(self.resampled.samples() > 0)
    }
}

fn source_layout(decoded: &frame::Audio) -> ChannelLayout {
    let layout = decoded.channel_layout();
    if layout.is_empty() {
        ChannelLayout::default(i32::from(decoded.channels()))
    } else {
        layout
    }
}

use anyhow::{Context, Result};
use ffmpeg_next::util::error::EAGAIN;
use ffmpeg_next::{ChannelLayout, Packet, Rational, codec, encoder, frame};

use super::container::Muxer;
use super::{RecordingFormat, SAMPLE_RATE};

const FALLBACK_FRAME_SAMPLES: usize = 4096;

/// Encodes mono samples in codec-sized frames, buffering partial frames in place.
pub struct AudioEncoder {
    encoder: encoder::Audio,
    frame: frame::Audio,
    packet: Packet,
    encode_sample: fn(f32) -> [u8; 4],
    frame_samples: usize,
    buffered: usize,
    next_pts: i64,
}

impl AudioEncoder {
    pub fn open(format: RecordingFormat) -> Result<Self> {
        let codec_name = format.codec_name();
        let codec = encoder::find_by_name(codec_name)
            .with_context(|| format!("ffmpeg has no {codec_name} encoder"))?;
        let mut audio = codec::Context::new_with_codec(codec).encoder().audio()?;
        audio.set_rate(SAMPLE_RATE as i32);
        audio.set_format(format.sample_format());
        audio.set_channel_layout(ChannelLayout::MONO);
        audio.set_time_base(Rational(1, SAMPLE_RATE as i32));
        audio.set_flags(codec::Flags::GLOBAL_HEADER);
        if let Some(bit_rate) = format.bit_rate() {
            audio.set_bit_rate(bit_rate);
        }
        let encoder = audio.open()?;
        let frame_samples = match encoder.frame_size() {
            0 => FALLBACK_FRAME_SAMPLES,
            size => size as usize,
        };
        let mut frame =
            frame::Audio::new(format.sample_format(), frame_samples, ChannelLayout::MONO);
        frame.set_rate(SAMPLE_RATE);
        Ok(Self {
            encoder,
            frame,
            packet: Packet::empty(),
            encode_sample: format.sample_encoder(),
            frame_samples,
            buffered: 0,
            next_pts: 0,
        })
    }

    pub fn context(&self) -> &codec::Context {
        &self.encoder
    }

    pub fn write(&mut self, mut samples: &[f32], muxer: &mut Muxer) -> Result<()> {
        while !samples.is_empty() {
            let count = samples.len().min(self.frame_samples - self.buffered);
            let (head, rest) = samples.split_at(count);
            let (slots, _) = self.frame.data_mut(0).as_chunks_mut::<4>();
            slots[self.buffered..self.buffered + count]
                .iter_mut()
                .zip(head)
                .for_each(|(slot, &sample)| *slot = (self.encode_sample)(sample));
            self.buffered += count;
            samples = rest;
            if self.buffered == self.frame_samples {
                self.send_buffered(muxer)?;
            }
        }
        Ok(())
    }

    pub fn finish(&mut self, muxer: &mut Muxer) -> Result<()> {
        if self.buffered > 0 {
            self.frame.set_samples(self.buffered);
            self.send_buffered(muxer)?;
        }
        self.encoder.send_eof()?;
        self.drain(muxer)
    }

    fn send_buffered(&mut self, muxer: &mut Muxer) -> Result<()> {
        self.frame.set_pts(Some(self.next_pts));
        self.encoder.send_frame(&self.frame)?;
        self.next_pts += self.buffered as i64;
        self.buffered = 0;
        self.drain(muxer)
    }

    fn drain(&mut self, muxer: &mut Muxer) -> Result<()> {
        loop {
            match self.encoder.receive_packet(&mut self.packet) {
                Ok(()) => muxer.write(&mut self.packet)?,
                Err(ffmpeg_next::Error::Other { errno: EAGAIN } | ffmpeg_next::Error::Eof) => {
                    return Ok(());
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
}

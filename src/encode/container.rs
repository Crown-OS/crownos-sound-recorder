use std::path::Path;

use ffmpeg_next::{Packet, Rational, codec, format};

use super::RecordingFormat;

/// One audio stream muxed into a file.
pub struct Muxer {
    output: format::context::Output,
    stream_index: usize,
    encoder_time_base: Rational,
    stream_time_base: Rational,
}

impl Muxer {
    pub fn create(
        path: &Path,
        format: RecordingFormat,
        encoder: &codec::Context,
    ) -> Result<Self, ffmpeg_next::Error> {
        let mut output = format::output_as(path, format.container_name())?;
        let encoder_time_base = encoder.time_base();
        let stream_index = {
            let mut stream = output.add_stream_with(encoder)?;
            stream.set_time_base(encoder_time_base);
            stream.index()
        };
        output.write_header()?;
        let stream_time_base = output
            .stream(stream_index)
            .map_or(encoder_time_base, |stream| stream.time_base());
        Ok(Self {
            output,
            stream_index,
            encoder_time_base,
            stream_time_base,
        })
    }

    pub fn write(&mut self, packet: &mut Packet) -> Result<(), ffmpeg_next::Error> {
        packet.set_stream(self.stream_index);
        packet.rescale_ts(self.encoder_time_base, self.stream_time_base);
        packet.write_interleaved(&mut self.output)
    }

    pub fn finish(mut self) -> Result<(), ffmpeg_next::Error> {
        self.output.write_trailer()
    }
}

use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::Path;

use anyhow::Result;

use crate::decode::PcmDecoder;
use crate::encode::SAMPLE_RATE;
use crate::util::paths::peaks_sidecar;

pub const PEAKS_PER_SECOND: u32 = 20;

const SAMPLES_PER_PEAK: usize = (SAMPLE_RATE / PEAKS_PER_SECOND) as usize;
const MAGIC: &[u8; 4] = b"CRP1";
const FLUSH_EVERY_SECONDS: usize = 5;
const DECODE_BLOCK_SAMPLES: usize = SAMPLES_PER_PEAK * 4;

/// Turns samples into one `u8` absolute peak per 1/20 s and writes them as they complete.
pub struct PeaksWriter<W: Write> {
    output: W,
    bucket_peak: f32,
    bucket_len: usize,
}

impl PeaksWriter<BufWriter<File>> {
    /// Starts the `<audio>.peaks` sidecar.
    pub fn create(audio: &Path) -> io::Result<Self> {
        let capacity = PEAKS_PER_SECOND as usize * FLUSH_EVERY_SECONDS;
        let mut output = BufWriter::with_capacity(capacity, File::create(peaks_sidecar(audio))?);
        output.write_all(MAGIC)?;
        Ok(Self::new(output))
    }
}

impl<W: Write> PeaksWriter<W> {
    pub fn new(output: W) -> Self {
        Self {
            output,
            bucket_peak: 0.0,
            bucket_len: 0,
        }
    }

    pub fn push(&mut self, mut samples: &[f32]) -> io::Result<()> {
        while !samples.is_empty() {
            let count = samples.len().min(SAMPLES_PER_PEAK - self.bucket_len);
            let (head, rest) = samples.split_at(count);
            self.bucket_peak = head
                .iter()
                .fold(self.bucket_peak, |peak, sample| peak.max(sample.abs()));
            self.bucket_len += count;
            samples = rest;
            if self.bucket_len == SAMPLES_PER_PEAK {
                self.emit_bucket()?;
            }
        }
        Ok(())
    }

    /// Writes the trailing partial bucket and returns the flushed output.
    pub fn finish(mut self) -> io::Result<W> {
        if self.bucket_len > 0 {
            self.emit_bucket()?;
        }
        self.output.flush()?;
        Ok(self.output)
    }

    fn emit_bucket(&mut self) -> io::Result<()> {
        let level = (self.bucket_peak.min(1.0) * f32::from(u8::MAX)).round() as u8;
        self.bucket_peak = 0.0;
        self.bucket_len = 0;
        self.output.write_all(&[level])
    }
}

/// Reads the sidecar written while recording.
pub fn read_peaks(audio: &Path) -> io::Result<Vec<u8>> {
    let mut contents = fs::read(peaks_sidecar(audio))?;
    if !contents.starts_with(MAGIC) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "not a peaks file",
        ));
    }
    contents.drain(..MAGIC.len());
    Ok(contents)
}

/// Decodes `audio` to compute its peaks, for files recorded elsewhere.
pub fn compute_peaks(audio: &Path) -> Result<Vec<u8>> {
    let mut decoder = PcmDecoder::open(audio)?;
    let expected = decoder.duration().as_secs() as usize * PEAKS_PER_SECOND as usize;
    let mut peaks = PeaksWriter::new(Vec::with_capacity(expected + 1));
    let mut block = [0.0; DECODE_BLOCK_SAMPLES];
    loop {
        let count = decoder.read(&mut block)?;
        peaks.push(&block[..count])?;
        if count < block.len() {
            return Ok(peaks.finish()?);
        }
    }
}

/// Reads the sidecar, or computes peaks and saves the sidecar when it is missing or damaged.
pub fn load_or_compute_peaks(audio: &Path) -> Result<Vec<u8>> {
    if let Ok(peaks) = read_peaks(audio) {
        return Ok(peaks);
    }
    let peaks = compute_peaks(audio)?;
    let mut sidecar = Vec::with_capacity(MAGIC.len() + peaks.len());
    sidecar.extend_from_slice(MAGIC);
    sidecar.extend_from_slice(&peaks);
    fs::write(peaks_sidecar(audio), sidecar)?;
    Ok(peaks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecar_round_trips() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let audio = directory.path().join("take.ogg");
        let mut writer = PeaksWriter::create(&audio)?;
        let quiet = vec![0.25_f32; SAMPLES_PER_PEAK];
        let loud = vec![-1.0_f32; SAMPLES_PER_PEAK / 2];
        writer.push(&quiet)?;
        writer.push(&loud)?;
        writer.finish()?;
        assert_eq!(read_peaks(&audio)?, [64, 255]);
        Ok(())
    }

    #[test]
    fn one_minute_is_about_1200_bytes() {
        let mut writer = PeaksWriter::new(Vec::new());
        let minute = vec![0.5_f32; SAMPLE_RATE as usize * 60];
        assert!(writer.push(&minute).is_ok());
        assert_eq!(writer.finish().map(|peaks| peaks.len()).ok(), Some(1200));
    }
}

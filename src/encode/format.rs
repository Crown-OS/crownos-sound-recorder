use std::path::Path;

use ffmpeg_next::format::{Sample, sample};

pub const SAMPLE_RATE: u32 = 48_000;

const VOICE_BIT_RATE: usize = 64_000;

/// The file formats recordings are written in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum RecordingFormat {
    /// Opus in Ogg at voice bit rate.
    #[default]
    Opus,
    /// Lossless 24-bit FLAC.
    Flac,
}

impl RecordingFormat {
    pub const ALL: [Self; 2] = [Self::Opus, Self::Flac];

    pub const fn extension(self) -> &'static str {
        match self {
            Self::Opus => "ogg",
            Self::Flac => "flac",
        }
    }

    pub fn of_path(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()? {
            "ogg" | "opus" => Some(Self::Opus),
            "flac" => Some(Self::Flac),
            _ => None,
        }
    }

    pub(super) const fn codec_name(self) -> &'static str {
        match self {
            Self::Opus => "libopus",
            Self::Flac => "flac",
        }
    }

    pub(super) const fn container_name(self) -> &'static str {
        match self {
            Self::Opus => "ogg",
            Self::Flac => "flac",
        }
    }

    pub(super) const fn sample_format(self) -> Sample {
        match self {
            Self::Opus => Sample::F32(sample::Type::Packed),
            Self::Flac => Sample::I32(sample::Type::Packed),
        }
    }

    pub(super) const fn bit_rate(self) -> Option<usize> {
        match self {
            Self::Opus => Some(VOICE_BIT_RATE),
            Self::Flac => None,
        }
    }

    pub(super) const fn sample_encoder(self) -> fn(f32) -> [u8; 4] {
        match self {
            Self::Opus => f32::to_ne_bytes,
            Self::Flac => |sample| full_scale_i32(sample).to_ne_bytes(),
        }
    }
}

fn full_scale_i32(sample: f32) -> i32 {
    (f64::from(sample) * f64::from(i32::MAX)) as i32
}

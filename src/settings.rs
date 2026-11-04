use audio::{Enhancement, SourceId};
use crownos_config::schema::{Recorder, RecorderEnhancement, RecorderFormat};

use crate::encode::RecordingFormat;

/// What new recordings are made with, kept in the `recorder` config section.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Preferences {
    pub format: RecordingFormat,
    pub enhancement: Enhancement,
    pub input: Option<SourceId>,
}

/// Whether preference changes are written back to `~/.config/crownos/recorder.ron`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Persistence {
    Config,
    Memory,
}

impl Preferences {
    pub fn load() -> Self {
        let recorder: Recorder = crownos_config::load(Recorder::SECTION);
        Self {
            format: match recorder.format {
                RecorderFormat::Opus => RecordingFormat::Opus,
                RecorderFormat::Flac => RecordingFormat::Flac,
            },
            enhancement: match recorder.enhancement {
                RecorderEnhancement::Off => Enhancement::Off,
                RecorderEnhancement::Light => Enhancement::Light,
                RecorderEnhancement::NoiseSuppression => Enhancement::NoiseSuppression,
                RecorderEnhancement::StudioVoice => Enhancement::StudioVoice,
            },
            input: recorder.input.map(SourceId::new),
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let recorder = Recorder {
            format: match self.format {
                RecordingFormat::Opus => RecorderFormat::Opus,
                RecordingFormat::Flac => RecorderFormat::Flac,
            },
            enhancement: match self.enhancement {
                Enhancement::Off => RecorderEnhancement::Off,
                Enhancement::Light => RecorderEnhancement::Light,
                Enhancement::NoiseSuppression => RecorderEnhancement::NoiseSuppression,
                Enhancement::StudioVoice => RecorderEnhancement::StudioVoice,
            },
            input: self.input.as_ref().map(|source| source.as_str().to_owned()),
        };
        crownos_config::save(Recorder::SECTION, &recorder)
    }
}

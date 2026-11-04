use std::path::PathBuf;

/// What the content pane shows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Selection {
    #[default]
    Recorder,
    Recording(PathBuf),
}

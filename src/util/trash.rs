use std::ffi::{OsStr, OsString};
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Local;

const INFO_EXTENSION: &str = ".trashinfo";
const UNESCAPED_BYTES: &[u8] = b"/-._~";

/// The freedesktop.org home trash (`$XDG_DATA_HOME/Trash`).
#[derive(Debug, Clone)]
pub struct Trash {
    root: PathBuf,
}

impl Trash {
    pub fn home() -> Result<Self> {
        let data = dirs::data_dir().context("no XDG data directory")?;
        Ok(Self::at(data.join("Trash")))
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Moves `path` into the trash and returns where it went.
    pub fn put(&self, path: &Path) -> io::Result<PathBuf> {
        let original = std::path::absolute(path)?;
        let name = original
            .file_name()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
        let files = self.root.join("files");
        let info = self.root.join("info");
        fs::create_dir_all(&files)?;
        fs::create_dir_all(&info)?;
        let record = trash_info(&original);
        for attempt in 1u32.. {
            let trashed_name = numbered(name, attempt);
            let trashed = files.join(&trashed_name);
            let mut info_name = trashed_name.clone();
            info_name.push(INFO_EXTENSION);
            let info_path = info.join(info_name);
            let mut info_file = match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&info_path)
            {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            };
            if trashed.exists() {
                fs::remove_file(&info_path)?;
                continue;
            }
            return info_file
                .write_all(record.as_bytes())
                .and_then(|()| fs::rename(&original, &trashed))
                .map(|()| trashed)
                .inspect_err(|_| {
                    let _ = fs::remove_file(&info_path);
                });
        }
        Err(io::ErrorKind::StorageFull.into())
    }
}

fn numbered(name: &OsStr, attempt: u32) -> OsString {
    let mut numbered = name.to_owned();
    if attempt > 1 {
        numbered.push(format!(".{attempt}"));
    }
    numbered
}

fn trash_info(original: &Path) -> String {
    format!(
        "[Trash Info]\nPath={}\nDeletionDate={}\n",
        percent_encoded(original),
        Local::now().format("%Y-%m-%dT%H:%M:%S")
    )
}

fn percent_encoded(path: &Path) -> String {
    path.as_os_str()
        .as_bytes()
        .iter()
        .fold(String::new(), |mut encoded, &byte| {
            if byte.is_ascii_alphanumeric() || UNESCAPED_BYTES.contains(&byte) {
                encoded.push(char::from(byte));
            } else {
                let _ = write!(encoded, "%{byte:02X}");
            }
            encoded
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trashes_with_info_and_unique_names() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let trash = Trash::at(directory.path().join("Trash"));
        let victim = directory.path().join("a b.ogg");
        fs::write(&victim, b"one")?;
        let first = trash.put(&victim)?;
        fs::write(&victim, b"two")?;
        let second = trash.put(&victim)?;
        assert_ne!(first, second);
        assert!(!victim.exists());
        assert_eq!(fs::read(&second)?, b"two");
        let info = fs::read_to_string(directory.path().join("Trash/info/a b.ogg.trashinfo"))?;
        assert!(info.contains("/a%20b.ogg\n"), "{info}");
        Ok(())
    }
}

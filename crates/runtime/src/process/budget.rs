use crate::{Result, RuntimeError};
use std::{fs, path::PathBuf};
#[derive(Clone)]
pub struct FileBudget {
    pub directory: PathBuf,
    pub prefix: String,
    pub file_bytes: u64,
    pub total_bytes: u64,
    pub files: usize,
}
impl FileBudget {
    pub(super) fn validate(&self) -> Result<()> {
        if !self.directory.is_absolute()
            || fs::canonicalize(&self.directory)? != self.directory
            || fs::symlink_metadata(&self.directory)?
                .file_type()
                .is_symlink()
            || self.prefix.is_empty()
            || self.prefix.len() > 40
            || !self
                .prefix
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            || self.file_bytes == 0
            || self.file_bytes > 1024 * 1024 * 1024
            || self.total_bytes < self.file_bytes
            || self.total_bytes > 4 * 1024 * 1024 * 1024
            || self.files == 0
            || self.files > 200
        {
            return Err(RuntimeError::InvalidProcess);
        }
        Ok(())
    }
    pub(super) fn exceeded(&self) -> Result<bool> {
        let mut bytes = 0u64;
        let mut count = 0;
        for entry in fs::read_dir(&self.directory)? {
            let entry = entry?;
            if !entry
                .file_name()
                .to_string_lossy()
                .starts_with(&self.prefix)
            {
                continue;
            }
            let m = fs::symlink_metadata(entry.path())?;
            if !m.is_file() || m.file_type().is_symlink() {
                return Ok(true);
            }
            count += 1;
            bytes = bytes.saturating_add(m.len());
            if m.len() > self.file_bytes || bytes > self.total_bytes || count > self.files {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_owned_prefix_is_counted_and_links_refused() {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let b = FileBudget {
            directory: root.clone(),
            prefix: "cachegrind.".into(),
            file_bytes: 10,
            total_bytes: 20,
            files: 2,
        };
        b.validate().unwrap();
        fs::write(root.join("unrelated"), vec![0; 100]).unwrap();
        assert!(!b.exceeded().unwrap());
        fs::write(root.join("cachegrind.1"), vec![0; 11]).unwrap();
        assert!(b.exceeded().unwrap());
    }
}

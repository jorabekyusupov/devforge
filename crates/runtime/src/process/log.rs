use crate::{Result, RuntimeError};
use serde::Serialize;
use std::{
    collections::VecDeque,
    fs::{self, File, OpenOptions},
    io::Write,
    path::PathBuf,
};

#[derive(Debug, Clone, Copy)]
pub struct LogPolicy {
    pub memory_bytes: usize,
    pub file_bytes: u64,
    pub files: usize,
}
impl Default for LogPolicy {
    fn default() -> Self {
        Self {
            memory_bytes: 64 * 1024,
            file_bytes: 1024 * 1024,
            files: 3,
        }
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct LogChunk {
    pub stream: &'static str,
    pub text: String,
}
pub(crate) struct Logs {
    pub chunks: VecDeque<LogChunk>,
    pub dropped_bytes: u64,
    bytes: usize,
    policy: LogPolicy,
    path: PathBuf,
    file: File,
    size: u64,
}
impl Logs {
    pub fn open(path: PathBuf, policy: LogPolicy) -> Result<Self> {
        if policy.memory_bytes < 4096
            || policy.file_bytes < 4096
            || !(1..=10).contains(&policy.files)
        {
            return Err(RuntimeError::InvalidProcess);
        }
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let size = file.metadata()?.len();
        Ok(Self {
            chunks: VecDeque::new(),
            dropped_bytes: 0,
            bytes: 0,
            policy,
            path,
            file,
            size,
        })
    }
    fn rotate(&mut self) -> Result<()> {
        // Close the writer before rename, required on Windows.
        self.file.flush()?;
        let placeholder = tempfile::tempfile_in(self.path.parent().unwrap())?;
        drop(std::mem::replace(&mut self.file, placeholder));
        for index in (1..self.policy.files).rev() {
            let to = self.path.with_extension(format!("log.{index}"));
            if to.exists() {
                fs::remove_file(&to)?;
            }
            let from = if index == 1 {
                self.path.clone()
            } else {
                self.path.with_extension(format!("log.{}", index - 1))
            };
            if from.exists() {
                fs::rename(from, to)?;
            }
        }
        self.file = File::create(&self.path)?;
        self.size = 0;
        Ok(())
    }
    pub fn push(&mut self, stream: &'static str, bytes: &[u8]) -> Result<()> {
        let text = String::from_utf8_lossy(bytes).into_owned();
        self.bytes += text.len();
        self.chunks.push_back(LogChunk { stream, text });
        while self.bytes > self.policy.memory_bytes {
            if let Some(old) = self.chunks.pop_front() {
                self.bytes -= old.text.len();
                self.dropped_bytes += old.text.len() as u64;
            }
        }
        let record = [format!("[{stream}] ").as_bytes(), bytes, b"\n"].concat();
        for part in record.chunks(4096) {
            if self.size + part.len() as u64 > self.policy.file_bytes {
                self.rotate()?;
            }
            self.file
                .write_all(part)
                .map_err(|_| RuntimeError::LogFailed)?;
            self.size += part.len() as u64;
        }
        Ok(())
    }
}

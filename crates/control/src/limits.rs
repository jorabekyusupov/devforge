//! Per-request bounds; adapters enforce deadline before and after reading a projection.
use crate::{ErrorCode, Id, ProjectBinding, Result};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Bounded<const MAX: u64>(u64);
impl<const MAX: u64> Bounded<MAX> {
    pub fn new(value: u64) -> Result<Self> {
        if value == 0 || value > MAX {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self(value))
    }
    pub fn get(self) -> u64 {
        self.0
    }
}
impl<'de, const MAX: u64> Deserialize<'de> for Bounded<MAX> {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Self::new(u64::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct SafeInt(u64);
impl SafeInt {
    pub const MAX: u64 = 9_007_199_254_740_991;
    pub fn new(value: u64) -> Result<Self> {
        if value > Self::MAX {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self(value))
    }
    pub fn get(self) -> u64 {
        self.0
    }
}
impl<'de> Deserialize<'de> for SafeInt {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Self::new(u64::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Text<const MAX: usize>(String);
impl<const MAX: usize> Text<MAX> {
    pub fn new(value: String) -> Result<Self> {
        if value.chars().count() > MAX || value.chars().any(|c| c.is_control()) {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl<'de, const MAX: usize> Deserialize<'de> for Text<MAX> {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Self::new(String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cursor {
    pub project_id: Id,
    pub tool: Id,
    pub revision: SafeInt,
    pub offset: SafeInt,
    /// Unix milliseconds, compared to manager clock; never an Instant value.
    pub expires_at: SafeInt,
}
impl Cursor {
    /// Cursor is a pagination hint, not an authority token. Replay outside the
    /// exact project/tool/revision or after expiry requires an explicit resync.
    pub fn validate(&self, binding: &ProjectBinding, tool: &str, now: u64) -> Result<()> {
        if &self.project_id != binding.project_id() {
            return Err(ErrorCode::ProjectMismatch);
        }
        if self.tool.as_str() != tool
            || self.revision.get() != binding.revision()
            || now >= self.expires_at.get()
        {
            return Err(ErrorCode::CursorExpired);
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageRequest {
    pub limit: Bounded<500>,
    pub max_bytes: Bounded<262144>,
    pub timeout_ms: Bounded<30000>,
    pub cursor: Option<Cursor>,
}
impl Default for PageRequest {
    fn default() -> Self {
        Self {
            limit: Bounded(100),
            max_bytes: Bounded(65536),
            timeout_ms: Bounded(5000),
            cursor: None,
        }
    }
}
#[derive(Debug)]
pub struct ReadBudget {
    request: PageRequest,
    started: Instant,
}
impl ReadBudget {
    pub fn start(request: PageRequest) -> Self {
        Self {
            request,
            started: Instant::now(),
        }
    }
    pub fn check_deadline(&self) -> Result<()> {
        self.check_elapsed(self.started.elapsed())
    }
    pub fn check_elapsed(&self, elapsed: Duration) -> Result<()> {
        if elapsed >= Duration::from_millis(self.request.timeout_ms.get()) {
            return Err(ErrorCode::TimedOut);
        }
        Ok(())
    }
    pub fn check_payload<T: Serialize>(&self, rows: usize, payload: &T) -> Result<()> {
        self.check_deadline()?;
        if rows as u64 > self.request.limit.get() {
            return Err(ErrorCode::OutputLimitExceeded);
        }
        let mut writer = BudgetWriter {
            count: 0,
            maximum: self.request.max_bytes.get(),
            started: self.started,
            timeout: Duration::from_millis(self.request.timeout_ms.get()),
            failure: None,
        };
        if serde_json::to_writer(&mut writer, payload).is_err() {
            return Err(writer.failure.unwrap_or(ErrorCode::InvalidInput));
        }
        self.check_deadline()
    }
}

/// No payload-sized buffer: a serializer is stopped at the first write exceeding
/// the byte budget. Deadlines are cooperative at writer calls and before/after
/// serialization; arbitrary producer CPU work cannot be preempted here.
struct BudgetWriter {
    count: u64,
    maximum: u64,
    started: Instant,
    timeout: Duration,
    failure: Option<ErrorCode>,
}
impl std::io::Write for BudgetWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let failure = if self.started.elapsed() >= self.timeout {
            Some(ErrorCode::TimedOut)
        } else if buffer.len() as u64 > self.maximum.saturating_sub(self.count) {
            Some(ErrorCode::OutputLimitExceeded)
        } else {
            None
        };
        if let Some(failure) = failure {
            self.failure = Some(failure);
            return Err(std::io::Error::other("projection budget exceeded"));
        }
        self.count += buffer.len() as u64;
        Ok(buffer.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

use crate::{Error, Result};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// One big-endian u32 length followed by exactly one JSON value. Limits are
/// applied before allocation; a transport connection carries one request only.
pub(crate) async fn read<R: AsyncRead + Unpin>(reader: &mut R, limit: usize) -> Result<Value> {
    let size = reader.read_u32().await? as usize;
    if size == 0 {
        return Err(Error::InvalidFrame);
    }
    if size > limit {
        return Err(Error::FrameTooLarge);
    }
    let mut bytes = vec![0; size];
    reader.read_exact(&mut bytes).await?;
    serde_json::from_slice(&bytes).map_err(|_| Error::InvalidFrame)
}

pub(crate) async fn write<W: AsyncWrite + Unpin>(
    writer: &mut W,
    value: &Value,
    limit: usize,
) -> Result<()> {
    // Value already exists in the trusted handler. The counting writer stops
    // serialization at the bound, avoiding a second payload-sized allocation.
    let mut bytes = BoundedBuffer {
        bytes: Vec::new(),
        limit,
        exceeded: false,
    };
    if serde_json::to_writer(&mut bytes, value).is_err() {
        return Err(if bytes.exceeded {
            Error::FrameTooLarge
        } else {
            Error::InvalidFrame
        });
    }
    writer.write_u32(bytes.bytes.len() as u32).await?;
    writer.write_all(&bytes.bytes).await?;
    writer.flush().await?;
    Ok(())
}
struct BoundedBuffer {
    bytes: Vec<u8>,
    limit: usize,
    exceeded: bool,
}
impl std::io::Write for BoundedBuffer {
    fn write(&mut self, value: &[u8]) -> std::io::Result<usize> {
        if value.len() > self.limit.saturating_sub(self.bytes.len()) {
            self.exceeded = true;
            return Err(std::io::Error::other("frame limit"));
        }
        self.bytes.extend_from_slice(value);
        Ok(value.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

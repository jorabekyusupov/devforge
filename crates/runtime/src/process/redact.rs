use zeroize::Zeroizing;
/// Secrets are withheld across read boundaries; line flushing is safe only when
/// none of the secrets contains a newline. Raw chunks never reach log storage.
pub(super) struct Redactor {
    secrets: Vec<Zeroizing<String>>,
    pending: Vec<u8>,
    longest: usize,
    line_safe: bool,
}
impl Redactor {
    pub fn new(secrets: &[Zeroizing<String>]) -> Self {
        Self {
            secrets: secrets.to_vec(),
            pending: Vec::new(),
            longest: secrets.iter().map(|s| s.len()).max().unwrap_or(1),
            line_safe: secrets.iter().all(|s| !s.contains('\n')),
        }
    }
    pub fn push(&mut self, bytes: &[u8], eof: bool) -> Vec<u8> {
        self.pending.extend_from_slice(bytes);
        let end = if eof {
            self.pending.len()
        } else if self.line_safe {
            self.pending
                .iter()
                .rposition(|b| *b == b'\n')
                .map(|p| p + 1)
                .unwrap_or_else(|| self.pending.len().saturating_sub(self.longest - 1))
        } else {
            self.pending.len().saturating_sub(self.longest - 1)
        };
        let mut out = Vec::new();
        let mut pos = 0;
        while pos < end {
            if let Some(length) = self
                .secrets
                .iter()
                .filter(|s| !s.is_empty() && self.pending[pos..].starts_with(s.as_bytes()))
                .map(|s| s.len())
                .max()
            {
                out.extend_from_slice(b"[REDACTED]");
                pos += length;
            } else {
                out.push(self.pending[pos]);
                pos += 1;
            }
        }
        self.pending.drain(..pos);
        out
    }
}
impl Drop for Redactor {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.pending.zeroize();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn secrets_split_across_reads_and_overlapping_prefixes_never_escape() {
        let mut r = Redactor::new(&[
            Zeroizing::new("password123".into()),
            Zeroizing::new("password".into()),
        ]);
        let mut out = r.push(b"hello pass", false);
        out.extend(r.push(b"word123 world\npass", false));
        out.extend(r.push(b"word", true));
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "hello [REDACTED] world\n[REDACTED]"
        );
    }
}

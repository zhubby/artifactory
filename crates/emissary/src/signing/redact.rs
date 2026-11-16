//! [INPUT]: 依赖本任务的秘密字节和进程输出流。
//! [OUTPUT]: 提供跨日志分块的秘密替换。
//! [POS]: Worker 写入事件管道前的日志脱敏边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use anyhow::Result;
use tokio::io::{AsyncRead, AsyncReadExt};
use zeroize::Zeroizing;

#[derive(Clone, Default)]
pub struct Secrets(pub Vec<Zeroizing<Vec<u8>>>);
impl Secrets {
    pub fn add(&mut self, value: &str) {
        if !value.is_empty() {
            self.0.push(Zeroizing::new(value.as_bytes().to_vec()));
            // JSON-escaped passwords can appear in structured tool output.
            if let Ok(escaped) = serde_json::to_string(value) {
                let escaped = &escaped[1..escaped.len() - 1];
                if escaped != value {
                    self.0.push(Zeroizing::new(escaped.as_bytes().to_vec()));
                }
            }
        }
    }
}
struct Redactor {
    secrets: Secrets,
    pending: Vec<u8>,
    max: usize,
}
impl Redactor {
    fn new(secrets: Secrets) -> Self {
        let max = secrets.0.iter().map(|s| s.len()).max().unwrap_or(1);
        Self {
            secrets,
            pending: Vec::new(),
            max,
        }
    }
    fn push(&mut self, bytes: &[u8], eof: bool) -> Vec<u8> {
        self.pending.extend_from_slice(bytes);
        let mut output = Vec::new();
        let mut index = 0;
        while index < self.pending.len() {
            let rest = &self.pending[index..];
            if !eof && rest.len() < self.max {
                break;
            }
            if let Some(secret) = self
                .secrets
                .0
                .iter()
                .filter(|s| rest.starts_with(s))
                .max_by_key(|s| s.len())
            {
                output.extend_from_slice(b"[REDACTED]");
                index += secret.len();
            } else {
                output.push(rest[0]);
                index += 1;
            }
        }
        self.pending.drain(..index);
        output
    }
}
pub async fn read_log(
    mut reader: impl AsyncRead + Unpin,
    step: &str,
    emit: crate::worker::Emitter,
    secrets: Secrets,
) -> Result<()> {
    let mut redact = Redactor::new(secrets);
    let mut buffer = vec![0; 32768];
    loop {
        let count = reader.read(&mut buffer).await?;
        let data = redact.push(&buffer[..count], count == 0);
        if !data.is_empty() {
            emit.send(crate::worker::Event {
                kind: "log".into(),
                step: step.into(),
                data,
                ..Default::default()
            })
            .await?;
        }
        if count == 0 {
            return Ok(());
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn redacts_across_every_chunk_boundary_and_at_eof() {
        for split in 0..=23 {
            let bytes = b"before-secret123-after-x";
            let mut secrets = Secrets::default();
            secrets.add("secret123");
            secrets.add("x");
            let mut r = Redactor::new(secrets);
            let mut output = r.push(&bytes[..split], false);
            output.extend(r.push(&bytes[split..], true));
            assert_eq!(output, b"before-[REDACTED]-after-[REDACTED]");
        }
    }
}

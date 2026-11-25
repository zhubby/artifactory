//! [INPUT]: 依赖父进程的有界私有管道、固定 PTY 配置和取消信号。
//! [OUTPUT]: 执行隐藏 terminal-worker，输出字节事件和真实进程状态。
//! [POS]: 隔离设备网络与 PTY；管道有界背压，EOF 和拥塞截止触发回收。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use anyhow::{Result, ensure};
use artifactory_execution_support::terminal::{TerminalConfig, TerminalEvent, TerminalInput};
use serde::de::DeserializeOwned;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

// 16 KiB 字节数组的 JSON 编码低于 128 KiB；不得无限读到换行。
pub(super) const MAX_WORKER_LINE: usize = 128 * 1024;

pub(super) async fn read_record<T: DeserializeOwned>(
    reader: &mut (impl AsyncBufRead + Unpin),
) -> Result<Option<T>> {
    let mut bytes = Vec::new();
    let size = (&mut *reader)
        .take((MAX_WORKER_LINE + 1) as u64)
        .read_until(b'\n', &mut bytes)
        .await?;
    if size == 0 {
        return Ok(None);
    }
    ensure!(
        size <= MAX_WORKER_LINE && bytes.last() == Some(&b'\n'),
        "invalid terminal worker record"
    );
    Ok(Some(serde_json::from_slice(&bytes)?))
}

/// 只从父进程管道读取配置，不读取完整节点配置或设备私钥。
///
/// # Errors
/// PTY 创建、管道或回收失败时返回错误；不会降级为伪终端。
pub async fn stdin_worker(cancel: CancellationToken) -> Result<()> {
    let mut reader = BufReader::new(tokio::io::stdin());
    let config: TerminalConfig = read_record(&mut reader)
        .await?
        .ok_or_else(|| anyhow::anyhow!("terminal worker configuration missing"))?;
    let (input_tx, input_rx) = mpsc::channel::<TerminalInput>(16);
    let (event_tx, mut event_rx) = mpsc::channel::<TerminalEvent>(8);
    let input_stop = cancel.clone();
    let input = tokio::spawn(async move {
        loop {
            let record = tokio::select! {
                _ = input_stop.cancelled() => break Ok(()),
                record = read_record::<TerminalInput>(&mut reader) => record,
            };
            let record = match record {
                Ok(Some(record)) => record,
                Ok(None) => {
                    input_stop.cancel();
                    break Ok(());
                }
                Err(error) => {
                    input_stop.cancel();
                    break Err(error);
                }
            };
            let sent = tokio::select! {
                biased;
                _ = input_stop.cancelled() => break Ok(()),
                result = tokio::time::timeout(std::time::Duration::from_secs(3), input_tx.send(record)) => result,
            };
            if !matches!(sent, Ok(Ok(()))) {
                input_stop.cancel();
                break Err(anyhow::anyhow!("terminal worker input stalled or closed"));
            }
        }
    });
    let output_stop = cancel.clone();
    let mut writer = tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        while let Some(event) = event_rx.recv().await {
            if output_stop.is_cancelled() {
                break;
            }
            let mut bytes = serde_json::to_vec(&event)?;
            bytes.push(b'\n');
            tokio::select! {
                biased;
                _ = output_stop.cancelled() => break,
                result = async { stdout.write_all(&bytes).await?; stdout.flush().await } => result?,
            }
        }
        Ok::<(), anyhow::Error>(())
    });
    let result =
        artifactory_execution_support::terminal::run(config, input_rx, event_tx, cancel.clone())
            .await;
    let input_result = if input.is_finished() {
        input.await?
    } else {
        input.abort();
        let _ = input.await;
        Ok(())
    };
    if tokio::time::timeout(std::time::Duration::from_secs(2), &mut writer)
        .await
        .is_err()
    {
        writer.abort();
        let _ = writer.await;
    }
    if let Err(error) = result.map_err(anyhow::Error::from).and(input_result) {
        let mut stdout = tokio::io::stdout();
        let mut bytes =
            serde_json::to_vec(&serde_json::json!({"type":"error","message":error.to_string()}))?;
        bytes.push(b'\n');
        let _ = stdout.write_all(&bytes).await;
        let _ = stdout.flush().await;
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn worker_records_are_bounded_even_without_a_newline() {
        let bytes = vec![b'x'; MAX_WORKER_LINE + 1];
        let mut reader = BufReader::new(bytes.as_slice());
        assert!(read_record::<serde_json::Value>(&mut reader).await.is_err());
    }
}

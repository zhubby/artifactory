//! [INPUT]: 真实Runner、固定配置和取消信号。
//! [OUTPUT]: 提供有界停止、等待队列锁释放及超时任务回收。
//! [POS]: 普通恢复与完整main验收共用的Runner寿命边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use anyhow::{Context, Result, ensure};
use artifactory_emissary::{config::Config, runner};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub struct Running {
    cancel: CancellationToken,
    runner: Option<tokio::task::JoinHandle<Result<()>>>,
}
impl Running {
    pub fn new() -> Self {
        Self {
            cancel: CancellationToken::new(),
            runner: None,
        }
    }
    pub fn start(&mut self, cfg: Config) -> Result<()> {
        ensure!(self.runner.is_none(), "sender already running");
        self.cancel = CancellationToken::new();
        let token = self.cancel.clone();
        self.runner = Some(tokio::spawn(async move { runner::run(cfg, token).await }));
        Ok(())
    }
    pub async fn close(&mut self) -> Result<()> {
        self.cancel.cancel();
        if let Some(mut task) = self.runner.take() {
            match tokio::time::timeout(Duration::from_secs(20), &mut task).await {
                Ok(result) => result.context("runner task failed")?,
                Err(_) => {
                    task.abort();
                    let _ = task.await;
                    Err(anyhow::anyhow!("runner did not release queue locks"))
                }
            }
        } else {
            Ok(())
        }
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Some(task) = self.runner.take() {
            task.abort();
        }
    }
}

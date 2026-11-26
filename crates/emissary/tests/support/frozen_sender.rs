//! [INPUT]: 父驱动的旧发送请求与退出回执。
//! [OUTPUT]: 顺序请求冻结旧进程，发布完成标记并等待进程回收。
//! [POS]: Service门禁与完整main门禁共用的旧进程协调；不执行Shell。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use anyhow::{Context, Result, ensure};
use artifactory_emissary::config::{atomic_bytes, atomic_json};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

pub async fn request_sender(root: &Path, sequence: u8, test: &str) -> Result<()> {
    atomic_json(
        &root.join(format!("request-{sequence}.json")),
        &json!({"sequence":sequence,"test":test}),
    )
}
pub async fn wait_sender(root: &Path, sequence: u8) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let path = root.join(format!("result-{sequence}.json"));
            if path.try_exists()? {
                let result: Value = serde_json::from_slice(&fs::read(path)?)?;
                ensure!(
                    result["sequence"] == sequence
                        && result["success"] == true
                        && result["process_reaped"] == true,
                    "supervised frozen sender failed"
                );
                return Ok::<_, anyhow::Error>(());
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .context("frozen sender supervisor result timed out")?
}

pub struct Sender {
    root: PathBuf,
    sequence: u8,
    closed: bool,
}
impl Sender {
    pub async fn start(root: &Path, sequence: u8) -> Result<Self> {
        ensure!(
            !root.join("started").try_exists()?,
            "old sender invocation marker already exists"
        );
        request_sender(root, sequence, "frozen_sender_to_current_receiver").await?;
        Ok(Self {
            root: root.to_owned(),
            sequence,
            closed: false,
        })
    }
    pub fn running(&self) -> Result<()> {
        ensure!(
            !self
                .root
                .join(format!("result-{}.json", self.sequence))
                .try_exists()?,
            "frozen sender exited early"
        );
        Ok(())
    }
    pub async fn finish(&mut self, root: &Path) -> Result<()> {
        atomic_bytes(&root.join("finish"), b"receiver verified", false)?;
        self.close().await
    }
    pub async fn close(&mut self) -> Result<()> {
        if !self.closed {
            atomic_bytes(&self.root.join("finish"), b"receiver completed", false)?;
            wait_sender(&self.root, self.sequence).await?;
            self.closed = true;
        }
        Ok(())
    }
}
impl Drop for Sender {
    fn drop(&mut self) {
        if !self.closed {
            let _ = atomic_bytes(&self.root.join("finish"), b"receiver dropped", false);
        }
    }
}

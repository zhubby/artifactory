//! [INPUT]: 依赖序列化事件和有界异步通道。
//! [OUTPUT]: 提供 Worker 事件编码和背压发送端。
//! [POS]: 进程日志与各执行器报告之间的共享管道。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Event {
    pub kind: String,
    pub payload: Value,
    pub step: String,
    pub data: Vec<u8>,
}

#[derive(Clone)]
pub struct Emitter(mpsc::Sender<Event>);

impl Emitter {
    pub fn channel() -> (Self, mpsc::Receiver<Event>) {
        let (tx, rx) = mpsc::channel(64);
        (Self(tx), rx)
    }

    pub async fn send(&self, event: Event) -> Result<()> {
        self.0
            .send(event)
            .await
            .context("execution event pipe closed")
    }
}

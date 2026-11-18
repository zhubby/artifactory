//! [INPUT]: 依赖业务存储、启动配置和设备连接。
//! [OUTPUT]: 提供共享应用状态与审计入口。
//! [POS]: HTTP、gRPC 和后台调度共享的进程状态。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{protocol::Command, settings::Settings, storage::Storage};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{RwLock, mpsc};
use uuid::Uuid;

#[derive(Clone)]
pub struct Connection {
    pub session: Uuid,
    pub sender: mpsc::Sender<std::result::Result<Command, tonic::Status>>,
}

#[derive(Clone)]
pub struct AppState {
    pub coordination: Arc<crate::coordination::Coordination>,
    pub store: crate::store::ControlStore,
    pub settings: Arc<Settings>,
    pub storage: Arc<Storage>,
    pub connections: Arc<RwLock<HashMap<Uuid, Connection>>>,
}

impl AppState {
    pub async fn audit(
        &self,
        actor: impl ToString,
        action: &str,
        resource: impl ToString,
        details: serde_json::Value,
    ) -> anyhow::Result<()> {
        self.store
            .audit(actor.to_string(), action, resource.to_string(), details)
            .await?;
        Ok(())
    }
}

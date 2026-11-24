//! [INPUT]: 依赖受权构建身份和显式过程 SQL 投影。
//! [OUTPUT]: 提供最近过程事件与历史源码记录可用性。
//! [POS]: 统一观察入口；不读取凭据、原始日志和进程环境。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::ControlStore;
use artifactory_database::{params, sql};
use serde_json::Value;
use uuid::Uuid;

impl ControlStore {
    pub(crate) async fn build_activity(&self, build: Uuid) -> anyhow::Result<Value> {
        Ok(sql::scalar(
            &mut self.pool.clone(),
            include_str!("build_activity.sql"),
            params![build, self.tenant()?],
        )
        .await?)
    }
}

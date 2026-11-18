//! [INPUT]: 依赖节点记录和执行任务归属。
//! [OUTPUT]: 查询节点存在性和任务项目。
//! [POS]: HTTP 授权与设备调度共用的只读查询。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::ControlStore;
use crate::error::{ApiError, Result};
use artifactory_database::{params, sql};
use uuid::Uuid;
impl ControlStore {
    pub(crate) async fn node_exists(&self, id: Uuid) -> Result<bool> {
        let exists: bool = sql::scalar(
            &mut self.pool.clone(),
            "SELECT EXISTS(SELECT 1 FROM nodes WHERE id=$1)",
            params![id],
        )
        .await?;
        Ok(exists)
    }
    pub(crate) async fn attempt_project(&self, id: Uuid) -> Result<Uuid> {
        let project:Uuid=sql::optional_scalar(&mut self.pool.clone(), "SELECT b.project_id FROM attempts a JOIN jobs j ON j.id=a.job_id JOIN builds b ON b.id=j.build_id WHERE a.id=$1", params![id]).await?.ok_or(ApiError::NotFound)?;
        Ok(project)
    }
}

//! [INPUT]: 依赖租户范围、PostgreSQL 自然日和构建完整窗口。
//! [OUTPUT]: 提供受权概览统计、状态日期桶与近期项目活动。
//! [POS]: 聚合直接读取数据库，独立于截断的构建列表投影。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::{ControlStore, dto};
use crate::error::{ApiError, Result};
use artifactory_database::{params, sql};
use uuid::Uuid;

const DASHBOARD_SQL: &str = include_str!("dashboard.sql");

impl ControlStore {
    pub async fn dashboard(
        &self,
        project: Option<Uuid>,
        days: u32,
        timezone: &str,
    ) -> Result<dto::Dashboard> {
        self.dashboard_at(project, days, timezone, None).await
    }

    async fn dashboard_at(
        &self,
        project: Option<Uuid>,
        days: u32,
        timezone: &str,
        at: Option<String>,
    ) -> Result<dto::Dashboard> {
        if ![7, 30, 90].contains(&days) {
            return Err(ApiError::BadRequest("days must be 7, 30 or 90".into()));
        }
        if timezone.len() > 100 || timezone.is_empty() {
            return Err(ApiError::BadRequest("invalid dashboard timezone".into()));
        }
        let known: bool = sql::scalar(
            &mut self.pool.clone(),
            "SELECT EXISTS(SELECT 1 FROM pg_timezone_names WHERE name=$1)",
            params![timezone],
        )
        .await?;
        if !known {
            return Err(ApiError::BadRequest("invalid dashboard timezone".into()));
        }
        let tenant = self.tenant()?;
        let visible = self.scoped_projects();
        Ok(sql::scalar(
            &mut self.pool.clone(),
            DASHBOARD_SQL,
            params![tenant, visible, project, days as i32, timezone, at],
        )
        .await?)
    }
}

#[cfg(test)]
#[path = "dashboard_tests.rs"]
mod tests;

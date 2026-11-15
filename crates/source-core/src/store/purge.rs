use super::*;
use serde_json::json;

impl SourceStore {
    /// Freeze inventory before I/O; the tombstone blocks all later captures.
    pub async fn prepare_purge(&self, project: Uuid) -> Result<Value> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        let idle: bool = sql::scalar(
            &mut tx,
            "SELECT pg_try_advisory_xact_lock(hashtextextended($1,1))",
            params![project.to_string()],
        )
        .await?;
        ensure!(idle, "source capture still active");
        let inventory: Value = if let Some(v) = sql::optional_scalar(
            &mut tx,
            "SELECT inventory FROM source_deleted_projects WHERE project_id=$1",
            params![project],
        )
        .await?
        {
            v
        } else {
            let snapshots: Vec<Value> = sql::scalars(
                &mut tx,
                "SELECT manifest FROM source_snapshots WHERE project_id=$1",
                params![project],
            )
            .await?;
            let keys:Vec<String>=sql::scalars(&mut tx,"SELECT cache_key FROM source_repositories r WHERE project_id=$1 AND NOT EXISTS(SELECT 1 FROM source_snapshots s WHERE s.project_id<>$1 AND s.manifest->'repositories' @> jsonb_build_array(jsonb_build_object('repository_id',r.id)))",params![project]).await?;
            let credentials: Vec<Uuid> = if self.embedded {
                Vec::new()
            } else {
                sql::scalars(
                    &mut tx,
                    "SELECT credential_id FROM source_project_credentials WHERE project_id=$1",
                    params![project],
                )
                .await?
            };
            let v = json!({"snapshots":snapshots,"keys":keys,"credentials":credentials});
            sql::execute(
                &mut tx,
                "INSERT INTO source_deleted_projects(project_id,inventory) VALUES($1,$2)",
                params![project, &v],
            )
            .await?;
            v
        };
        tx.commit().await?;
        Ok(inventory)
    }
    pub async fn finish_purge(&self, project: Uuid, credentials: Vec<Uuid>) -> Result<()> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        if !self.embedded {
            for statement in [
                "DELETE FROM source_operation_repos WHERE operation_id IN (SELECT id FROM source_operations WHERE project_id=$1 OR snapshot_id IN (SELECT id FROM source_snapshots WHERE project_id=$1))",
                "DELETE FROM source_operations WHERE project_id=$1 OR snapshot_id IN (SELECT id FROM source_snapshots WHERE project_id=$1)",
                "DELETE FROM source_retentions WHERE snapshot_id IN (SELECT id FROM source_snapshots WHERE project_id=$1)",
                "DELETE FROM source_snapshots WHERE project_id=$1",
                "DELETE FROM source_resolutions WHERE repository_id IN (SELECT id FROM source_repositories r WHERE project_id=$1 AND NOT EXISTS(SELECT 1 FROM source_snapshots s WHERE s.root_repository_id=r.id OR s.manifest->'repositories' @> jsonb_build_array(jsonb_build_object('repository_id',r.id))))",
                "DELETE FROM source_repositories r WHERE project_id=$1 AND NOT EXISTS(SELECT 1 FROM source_snapshots s WHERE s.root_repository_id=r.id OR s.manifest->'repositories' @> jsonb_build_array(jsonb_build_object('repository_id',r.id)))",
                "DELETE FROM source_project_credentials WHERE project_id=$1",
            ] {
                sql::execute(&mut tx, statement, params![project]).await?;
            }
            sql::execute(&mut tx,"DELETE FROM credentials WHERE id=ANY($1) AND NOT EXISTS(SELECT 1 FROM source_repositories WHERE credential_id=credentials.id) AND NOT EXISTS(SELECT 1 FROM source_project_credentials WHERE credential_id=credentials.id)",params![credentials]).await?;
        }
        // Only the minimal identifier tombstone remains after cleanup.
        sql::execute(&mut tx,"UPDATE source_deleted_projects SET completed=true,inventory='{\"completed\":true}' WHERE project_id=$1",params![project]).await?;
        tx.commit().await?;
        Ok(())
    }
}

/// Remove only cache keys recorded by the service, with strict path validation.
pub async fn purge_cache(state: &crate::AppState, inventory: &Value) -> Result<()> {
    for key in inventory["keys"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        ensure!(
            key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid cache identity"
        );
        let path = state
            .settings
            .data_dir
            .join("sources/repos")
            .join(format!("{key}.git"));
        match tokio::fs::remove_dir_all(path).await {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

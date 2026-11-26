//! [INPUT]: 节点、attempt身份、清单正文外键与遥测事务。
//! [OUTPUT]: 删除授权身份的数据并持久保存重放围栏。
//! [POS]: 生命周期清扫边界；同锁事务先删清单引用再删正文。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
//! Delete identities directly from materializations so expired raw data belonging to
//! other tenants is never rebuilt or discarded. TimescaleDB 2.32 is required.
use super::TelemetryStore;
use anyhow::{Result, ensure};
use artifactory_database::{params, sql};
use uuid::Uuid;

impl TelemetryStore {
    pub async fn purge(&self, nodes: &[Uuid], attempts: &[Uuid]) -> Result<()> {
        let mut db = self.write.clone();
        let mut tx = db.transaction().await?;
        sql::execute(
            &mut tx,
            "SELECT pg_advisory_xact_lock(438284731022932)",
            params![],
        )
        .await?;
        for (kind, ids) in [("node", nodes), ("attempt", attempts)] {
            sql::execute(&mut tx,"INSERT INTO telemetry_deleted_identities(kind,id) SELECT $1,unnest($2::uuid[]) ON CONFLICT DO NOTHING",params![kind,ids]).await?;
        }
        for table in ["process_samples", "build_samples"] {
            sql::execute(
                &mut tx,
                &format!("DELETE FROM {table} WHERE node_id=ANY($1) OR attempt_id=ANY($2)"),
                params![nodes, attempts],
            )
            .await?;
        }
        for table in [
            "host_samples",
            "telemetry_batches",
            "inventory_observations",
            "latest_inventories",
            "latest_hosts",
            "startup_resources",
            "startup_hosts",
            "inventory_deliveries",
            "startup_inventories",
            "inventory_versions",
            "inventory_ingress",
        ] {
            sql::execute(
                &mut tx,
                &format!("DELETE FROM {table} WHERE node_id=ANY($1)"),
                params![nodes],
            )
            .await?;
        }
        let aggregates:Vec<(String,String,String)>=sql::tuples(&mut tx,"SELECT view_name::text,materialization_hypertable_schema::text,materialization_hypertable_name::text FROM timescaledb_information.continuous_aggregates WHERE view_schema=current_schema() AND view_name IN ('host_minutes','host_hours','build_minutes','build_hours')",params![]).await?;
        ensure!(aggregates.len() == 4, "telemetry materializations missing");
        for (view, schema, table) in aggregates {
            // Names come from the Timescale catalog, quoted as SQL identifiers.
            let relation = format!(
                "\"{}\".\"{}\"",
                schema.replace('"', "\"\""),
                table.replace('"', "\"\"")
            );
            let predicate = if view.starts_with("build_") {
                "node_id=ANY($1) OR attempt_id=ANY($2)"
            } else {
                "node_id=ANY($1)"
            };
            let values = if view.starts_with("build_") {
                params![nodes, attempts]
            } else {
                params![nodes]
            };
            sql::execute(
                &mut tx,
                &format!("DELETE FROM {relation} WHERE {predicate}"),
                values,
            )
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}

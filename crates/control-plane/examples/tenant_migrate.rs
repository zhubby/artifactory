//! Offline coordinated migration. Supply connection secrets only through environment variables.
use anyhow::{Context, Result, ensure};
use artifactory_control_plane::store::maintenance::TenantMigration;
use artifactory_planner_runtime::store::maintenance::TenantMigration as RuntimeMigration;
use std::path::Path;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let apply = args.iter().any(|a| a == "--apply");
    ensure!(
        !apply || args.iter().any(|a| a == "--maintenance-confirmed"),
        "stop all old business instances and confirm maintenance before applying"
    );
    let control =
        TenantMigration::connect(&std::env::var("DATABASE_URL").context("DATABASE_URL required")?)
            .await?;
    control.verify_drained().await?;
    let runtime = if let Ok(url) = std::env::var("ARTIFACTORY_RUNTIME_DATABASE_URL") {
        let migration = RuntimeMigration::connect(&url).await?;
        migration.verify_drained().await?;
        Some(migration)
    } else {
        None
    };
    if apply {
        control.apply().await?;
        if let Some(runtime) = &runtime {
            runtime.backfill(&control.project_tenants().await?).await?;
        }
        println!("Control migration and configured Runtime backfill verified");
    } else {
        println!("No in-flight work. Dry run only; no migration applied");
    }
    if args.iter().any(|a| a == "--write-snapshots") {
        control.write_snapshot(Path::new(
            "crates/control-plane/migrations/0004_snapshot.toml",
        ))?;
        if let Some(runtime) = runtime {
            runtime.write_snapshot(Path::new(
                "crates/planner-runtime/migrations/0001_snapshot.toml",
            ))?;
        }
        println!(
            "Toasty projection snapshots written; SQL owns server defaults, constraints and triggers"
        );
    }
    Ok(())
}

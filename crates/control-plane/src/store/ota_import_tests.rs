//! [INPUT]: 依赖隔离 PostgreSQL、发行导入事务与租户锁。
//! [OUTPUT]: 验证已获准导入仍与手动升级接受串行化。
//! [POS]: OTA 发行目录并发回归，不执行网络下载。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use crate::auth::Auth;
use artifactory_database::{PoolSettings, connect, with_options};
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn ota_import_serializes_with_manual_acceptance_even_for_admitted_tasks() -> anyhow::Result<()>
{
    let url = std::env::var("ARTIFACTORY_TEST_DATABASE_URL")?;
    let admin = connect(&url, Default::default(), PoolSettings::new(1)).await?;
    let schema = format!("ota_import_{}", Uuid::new_v4().simple());
    sql::execute(
        &mut admin.clone(),
        &format!("CREATE SCHEMA {schema}"),
        params![],
    )
    .await?;
    let pool = connect(
        &with_options(&url, &[("search_path", &schema)])?,
        models(),
        PoolSettings::new(5),
    )
    .await?;
    artifactory_database::migrate(&pool, &MIGRATIONS).await?;
    let unscoped = ControlStore::new(pool.clone());
    unscoped
        .bootstrap_admin("ota-owner".into(), "unused".into())
        .await?;
    let tenant = Uuid::from_u128(1);
    let actor: Uuid = sql::scalar(
        &mut pool.clone(),
        "SELECT id FROM users WHERE username='ota-owner'",
        params![],
    )
    .await?;
    let mut auth = Auth::scoped(actor, "admin", vec![], vec![]);
    auth.tenant_id = tenant;
    let store = unscoped.for_auth(&auth);
    let hash = "a".repeat(64);
    sql::execute(&mut pool.clone(), "INSERT INTO workspace_tasks(id,tenant_id,kind,object_hash,expires_at) VALUES($1,$2,'release_import',$3,clock_timestamp()+interval '1 minute')", params![Uuid::new_v4(),tenant,&hash]).await?;
    let mut connection = pool.clone();
    let mut acceptance = connection.transaction().await?;
    sql::execute(
        &mut acceptance,
        "SELECT pg_advisory_xact_lock(hashtextextended($1,105))",
        params![tenant.to_string()],
    )
    .await?;
    let blocker: i32 = sql::scalar(&mut acceptance, "SELECT pg_backend_pid()", params![]).await?;
    let attempt = store.clone();
    let import_hash = hash.clone();
    let task = tokio::spawn(async move {
        attempt
            .import_emissary_release("0.2.0", "{}".into(), "test", &import_hash)
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let waiting: bool = sql::scalar(
                &mut pool.clone(),
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)))",
                params![blocker],
            )
            .await
            .unwrap();
            if waiting {
                break;
            }
            assert!(
                !task.is_finished(),
                "admitted import bypassed the acceptance lock"
            );
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await?;
    let count: i64 = sql::scalar(
        &mut pool.clone(),
        "SELECT count(*) FROM emissary_releases",
        params![],
    )
    .await?;
    assert_eq!(
        count, 0,
        "directory stays stable while manual acceptance owns tenant lock"
    );
    acceptance.commit().await?;
    let id = task.await??;
    assert_eq!(
        store
            .import_emissary_release("0.2.0", "{}".into(), "test", &hash)
            .await?,
        id
    );
    assert!(
        store
            .import_emissary_release("0.2.0", "{different}".into(), "test", &"b".repeat(64))
            .await
            .is_err()
    );
    let count: i64 = sql::scalar(
        &mut pool.clone(),
        "SELECT count(*) FROM emissary_releases",
        params![],
    )
    .await?;
    assert_eq!(count, 1, "release identity remains immutable");
    sql::execute(
        &mut admin.clone(),
        &format!("DROP SCHEMA {schema} CASCADE"),
        params![],
    )
    .await?;
    Ok(())
}

//! [INPUT]: 依赖隔离 PostgreSQL、旧版本基线身份和当前初始化基线。
//! [OUTPUT]: 验证基线、追加迁移与误连未知历史时的无损拒绝。
//! [POS]: 业务数据库安装与兼容升级边界回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_control_plane::store::MIGRATIONS;

#[test]
fn new_baseline_has_a_distinct_identity() {
    let versions: Vec<_> = MIGRATIONS.migrations().iter().map(|m| m.id()).collect();
    assert_eq!(
        versions,
        vec![
            2026112101, 2026112201, 2026112301, 2026112302, 2026112303, 2026112401, 2026112402,
            2026112403, 2026112501, 2026112502, 2026112503
        ]
    );
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn image_failure_migration_preserves_existing_rows_and_unknown_failures() -> anyhow::Result<()>
{
    use artifactory_database::{
        PoolSettings, connect, migrate, params, sql, validate, with_options,
    };
    use toasty::migration::{MigrationFile, MigrationSet};
    use uuid::Uuid;
    static BASELINE: MigrationSet = MigrationSet::new(&[MigrationFile::new(
        2026112101,
        "0000_baseline.sql",
        include_str!("../migrations/migrations/0000_baseline.sql"),
    )]);
    let url = std::env::var("ARTIFACTORY_TEST_DATABASE_URL")?;
    let mut admin = connect(&url, Default::default(), PoolSettings::new(1)).await?;
    let schema = format!("migration_{}", Uuid::new_v4().simple());
    sql::execute(&mut admin, &format!("CREATE SCHEMA {schema}"), params![]).await?;
    let result=async {
        let scoped=with_options(&url,&[("search_path",&schema)])?;
        let mut db=connect(&scoped,artifactory_control_plane::store::models(),PoolSettings::new(2)).await?;
        migrate(&db,&BASELINE).await?;
        let store=artifactory_control_plane::store::ControlStore::new(db.clone());
        store.bootstrap_admin("migration-owner".into(),"unused".into()).await?;
        let project=Uuid::new_v4();
        let source=Uuid::new_v4();
        let build=Uuid::new_v4();
        let job=Uuid::new_v4();
        let execution=Uuid::new_v4();
        sql::execute(&mut db,"INSERT INTO projects(id,tenant_id,name,repository_url) VALUES($1,'00000000-0000-0000-0000-000000000001','migration','https://example.com/repo.git')",params![project]).await?;
        sql::execute(&mut db,"INSERT INTO source_manifests(id,project_id,commit_sha,manifest,digest) VALUES($1,$2,$3,'{}',$4)",params![source,project,"0".repeat(40),"a".repeat(64)]).await?;
        sql::execute(&mut db,"WITH r AS (INSERT INTO releases(id,project_id,version) VALUES(gen_random_uuid(),$2,'1.2.3') RETURNING id,version) INSERT INTO builds(id,project_id,requested_ref,status,source_repository_url,release_id,release_version,source_manifest_id) SELECT $1,$2,'main','queued','https://example.com/repo.git',id,version,$3 FROM r",params![build,project,source]).await?;
        sql::execute(&mut db,"INSERT INTO jobs(id,build_id,target,target_config) VALUES($1,$2,'image','{\"executor\":\"buildkit\",\"os\":\"linux\",\"arch\":\"amd64\",\"image\":{\"name\":\"app\",\"platforms\":[\"linux/amd64\"]}}')",params![job,build]).await?;
        anyhow::ensure!(store.claim_image(job,execution,"build","publish").await?);
        sql::execute(&mut db,"UPDATE image_executions SET status='failed',error='legacy unclassified diagnostic' WHERE id=$1",params![execution]).await?;
        let before:serde_json::Value=sql::scalar(&mut db,"SELECT to_jsonb(e) FROM image_executions e WHERE id=$1",params![execution]).await?;
        migrate(&db,&MIGRATIONS).await?;
        validate(&db,&MIGRATIONS).await?;
        migrate(&db,&MIGRATIONS).await?;
        let indexes:i64=sql::scalar(&mut db,"SELECT count(*) FROM pg_indexes WHERE schemaname=current_schema() AND indexname IN ('builds_project_created_at','builds_project_finished_at')",params![]).await?;
        assert_eq!(indexes,2);
        let preserved_builds:i64=sql::scalar(&mut db,"SELECT count(*) FROM builds WHERE id=$1",params![build]).await?;
        assert_eq!(preserved_builds,1);
        let after:serde_json::Value=sql::scalar(&mut db,"SELECT to_jsonb(e) FROM image_executions e WHERE id=$1",params![execution]).await?;
        assert!(after["failure_kind"].is_null());
        let mut original=after.as_object().unwrap().clone();
        original.remove("failure_kind");
        assert_eq!(serde_json::Value::Object(original),before);
        assert!(sql::execute(&mut db,"UPDATE image_executions SET failure_kind='unrecognized' WHERE id=$1",params![execution]).await.is_err());
        Ok::<_,anyhow::Error>(())
    }.await;
    sql::execute(
        &mut admin,
        &format!("DROP SCHEMA {schema} CASCADE"),
        params![],
    )
    .await?;
    result
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn legacy_baseline_is_rejected_without_changing_its_data() -> anyhow::Result<()> {
    use artifactory_database::{
        PoolSettings, connect, migrate, params, sql, validate, with_options,
    };
    use toasty::migration::{MigrationFile, MigrationSet};

    static LEGACY: MigrationSet = MigrationSet::new(&[MigrationFile::new(
        2026112001,
        "0000_baseline.sql",
        "CREATE TABLE legacy_planning_data(id bigint PRIMARY KEY,value text NOT NULL);
         INSERT INTO legacy_planning_data VALUES(1,'preserve-existing-instance');",
    )]);
    let url = std::env::var("ARTIFACTORY_TEST_DATABASE_URL")?;
    let mut admin = connect(&url, Default::default(), PoolSettings::new(1)).await?;
    let schema = format!("baseline_{}", uuid::Uuid::new_v4().simple());
    sql::execute(&mut admin, &format!("CREATE SCHEMA {schema}"), params![]).await?;
    let result = async {
        let scoped = with_options(&url, &[("search_path", &schema)])?;
        let mut db = connect(&scoped, Default::default(), PoolSettings::new(2)).await?;
        migrate(&db, &LEGACY).await?;
        // 启动校验只比较已应用身份；旧数据不能被新 SQL 重新解释。
        for error in [
            validate(&db, &MIGRATIONS).await.unwrap_err(),
            migrate(&db, &MIGRATIONS).await.unwrap_err(),
        ] {
            assert!(error.to_string().contains("unrecognized database migration"));
        }
        let record: String = sql::scalar(
            &mut db,
            "SELECT value FROM legacy_planning_data WHERE id=1",
            params![],
        ).await?;
        assert_eq!(record, "preserve-existing-instance");
        let version: i64 = sql::scalar(
            &mut db,
            "SELECT id FROM __toasty_migrations",
            params![],
        ).await?;
        assert_eq!(version, 2026112001);
        let created: bool = sql::scalar(
            &mut db,
            "SELECT EXISTS(SELECT 1 FROM pg_class WHERE relnamespace=current_schema()::regnamespace AND relname='planning_intents')",
            params![],
        ).await?;
        assert!(!created, "rejected startup must not create new planning tables");
        Ok::<_, anyhow::Error>(())
    }.await;
    sql::execute(
        &mut admin,
        &format!("DROP SCHEMA {schema} CASCADE"),
        params![],
    )
    .await?;
    result
}

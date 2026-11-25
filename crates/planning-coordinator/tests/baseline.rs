//! [INPUT]: 依赖隔离 PostgreSQL、旧基线与当前追加迁移。
//! [OUTPUT]: 验证迁移身份与误连旧数据库时的无损拒绝。
//! [POS]: 全新部署边界回归；不实现旧数据库迁移。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_planning_coordinator::store::MIGRATIONS;

#[test]
fn new_baseline_has_a_distinct_identity() {
    let versions: Vec<_> = MIGRATIONS.migrations().iter().map(|m| m.id()).collect();
    assert_eq!(versions, vec![2026111901, 2026112501]);
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn legacy_baseline_is_rejected_without_changing_its_data() -> anyhow::Result<()> {
    use artifactory_database::{
        PoolSettings, connect, migrate, params, sql, validate, with_options,
    };
    use toasty::migration::{MigrationFile, MigrationSet};

    static LEGACY: MigrationSet = MigrationSet::new(&[MigrationFile::new(
        2026111802,
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
        assert_eq!(version, 2026111802);
        let created: bool = sql::scalar(
            &mut db,
            "SELECT EXISTS(SELECT 1 FROM pg_class WHERE relnamespace=current_schema()::regnamespace AND relname='planning_runs')",
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

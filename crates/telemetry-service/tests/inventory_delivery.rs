//! [INPUT]: 隔离TimescaleDB、独立清单观察、正文外键与生命周期锁。
//! [OUTPUT]: 验证清单持久确认、迁移回滚、正文引用保护与删除围栏。
//! [POS]: 独立清单写入及读取的数据库集成回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_database::{PoolSettings, connect, params, sql, with_database};
use artifactory_telemetry_service::store;
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires isolated TimescaleDB administrator database"]
async fn migration_creates_durable_inventory_receipts_and_startup_projection() {
    let url = std::env::var("ARTIFACTORY_TEST_DATABASE_URL").unwrap();
    let mut admin = connect(&url, Default::default(), PoolSettings::new(2))
        .await
        .unwrap();
    let name = format!("inventory_{}", Uuid::new_v4().simple());
    sql::execute(&mut admin, &format!("CREATE DATABASE {name}"), params![])
        .await
        .unwrap();
    let pool = connect(
        &with_database(&url, &name).unwrap(),
        store::models(),
        PoolSettings::new(4),
    )
    .await
    .unwrap();
    artifactory_database::require_timescale(&pool)
        .await
        .unwrap();
    sql::scalar::<bool>(
        &mut pool.clone(),
        "SELECT _timescaledb_functions.stop_background_workers()",
        params![],
    )
    .await
    .unwrap();
    artifactory_database::migrate(&pool, &store::MIGRATIONS)
        .await
        .unwrap();
    let tables: i64 = sql::scalar(&mut pool.clone(), "SELECT count(*) FROM pg_class WHERE relnamespace=current_schema()::regnamespace AND relname IN ('inventory_deliveries','startup_inventories')", params![]).await.unwrap();
    assert_eq!(
        tables, 2,
        "清单回执与startup摘要必须耐久存在，不复用采样时间唯一键"
    );
    let node = Uuid::new_v4();
    let startup = Uuid::new_v4();
    let old_startup = Uuid::new_v4();
    let now = chrono::Utc::now();
    let make = |startup, revision, time, os: &str| {
        artifactory_emissary_protocol::InventoryObservation::new(
            startup,
            revision,
            time,
            5,
            &artifactory_emissary_protocol::telemetry::Inventory {
                os: os.into(),
                arch: "arm64".into(),
                ..Default::default()
            },
        )
        .unwrap()
    };
    let observation = make(startup, 0, now - chrono::Duration::days(3), "linux");
    assert!(
        !store::ingest_inventory(&pool, node, &observation, now)
            .await
            .unwrap(),
        "超过48小时的真实清单仍可补交"
    );
    // 两类引用分别保护正文；失败事务不得移除另一类引用。
    for removed in ["inventory_deliveries", "startup_inventories"] {
        let mut db = pool.clone();
        let mut tx = db.transaction().await.unwrap();
        sql::execute(
            &mut tx,
            &format!("DELETE FROM {removed} WHERE node_id=$1"),
            params![node],
        )
        .await
        .unwrap();
        let error = sql::execute(
            &mut tx,
            "DELETE FROM inventory_versions WHERE node_id=$1",
            params![node],
        )
        .await
        .unwrap_err();
        assert_eq!(
            artifactory_database::constraint(&error),
            Some(artifactory_database::Constraint::ForeignKey)
        );
        tx.rollback().await.unwrap();
        for table in [
            "inventory_deliveries",
            "startup_inventories",
            "inventory_versions",
        ] {
            let count: i64 = sql::scalar(
                &mut pool.clone(),
                &format!("SELECT count(*) FROM {table} WHERE node_id=$1"),
                params![node],
            )
            .await
            .unwrap();
            assert_eq!(count, 1, "{removed}回滚后{table}必须保留");
        }
    }
    assert!(
        store::ingest_inventory(&pool, node, &observation, now)
            .await
            .unwrap(),
        "ACK丢失重放回执"
    );
    let mut conflict = observation.clone();
    conflict.monotonic_ms += 1;
    conflict.delivery_digest = conflict.computed_digest();
    assert_eq!(
        store::ingest_inventory(&pool, node, &conflict, now)
            .await
            .unwrap_err()
            .code(),
        tonic::Code::AlreadyExists
    );
    let collision = make(startup, 0, now, "linux");
    assert_eq!(
        store::ingest_inventory(&pool, node, &collision, now)
            .await
            .unwrap_err()
            .code(),
        tonic::Code::AlreadyExists,
        "相同revision不能被新ID覆盖"
    );
    let newer = make(startup, 1, now, "darwin");
    store::ingest_inventory(&pool, node, &newer, now)
        .await
        .unwrap();
    let older = make(
        old_startup,
        0,
        now + chrono::Duration::seconds(1),
        "windows",
    );
    store::ingest_inventory(&pool, node, &older, now)
        .await
        .unwrap();
    let telemetry = store::TelemetryStore::new(pool.clone(), pool.clone());
    let latest = telemetry
        .latest_for_startup(node, startup)
        .await
        .unwrap_or_else(|_| panic!("当前startup查询失败"));
    assert_eq!(
        latest.inventory.as_ref().unwrap()["inventory"]["os"],
        "darwin",
        "旧startup较大时间不能覆盖当前观察"
    );
    assert_eq!(latest.inventory.as_ref().unwrap()["revision"], 1);
    use tower::ServiceExt;
    let response = artifactory_telemetry_service::query::router(
        artifactory_telemetry_service::Service {
            store: telemetry.clone(),
            authorization: store::BusinessAuthorizationRepository::new(pool.clone()),
        },
        "x".repeat(32),
    )
    .oneshot(
        axum::http::Request::builder()
            .uri(format!("/v1/nodes/{node}/latest?startup_id={startup}"))
            .header("authorization", format!("Bearer {}", "x".repeat(32)))
            .body(axum::body::Body::empty())
            .unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(response.status(), 200);
    let queried: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(queried["inventory"]["startup_id"], startup.to_string());
    assert_eq!(queried["inventory"]["inventory"]["os"], "darwin");
    let future = make(
        Uuid::new_v4(),
        0,
        now + chrono::Duration::minutes(6),
        "linux",
    );
    assert_eq!(
        artifactory_emissary_protocol::telemetry_negotiation::RejectionReason::from_status(
            &store::ingest_inventory(&pool, node, &future, now)
                .await
                .unwrap_err()
        ),
        Some(artifactory_emissary_protocol::telemetry_negotiation::RejectionReason::ClockFuture)
    );
    let mut held_db = pool.clone();
    let mut held = held_db.transaction().await.unwrap();
    sql::execute(
        &mut held,
        "SELECT pg_advisory_xact_lock(hashtextextended($1,47))",
        params![format!("inventory:{node}")],
    )
    .await
    .unwrap();
    assert_eq!(
        store::ingest_inventory(&pool, node, &observation, now)
            .await
            .unwrap_err()
            .code(),
        tonic::Code::ResourceExhausted,
        "同节点跨副本写入不得排队放大"
    );
    held.commit().await.unwrap();
    assert!(
        store::ingest_inventory(&pool, node, &observation, now)
            .await
            .unwrap()
    );
    let rollback_node = Uuid::new_v4();
    let rollback = make(Uuid::new_v4(), 0, now, "rollback");
    sql::execute(&mut pool.clone(),"CREATE FUNCTION fail_inventory_projection() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected projection failure'; END $$",params![]).await.unwrap();
    sql::execute(&mut pool.clone(),"CREATE TRIGGER fail_inventory_projection BEFORE INSERT ON startup_inventories FOR EACH ROW EXECUTE FUNCTION fail_inventory_projection()",params![]).await.unwrap();
    assert_eq!(
        store::ingest_inventory(&pool, rollback_node, &rollback, now)
            .await
            .unwrap_err()
            .code(),
        tonic::Code::Unavailable
    );
    for table in [
        "inventory_deliveries",
        "inventory_versions",
        "inventory_ingress",
    ] {
        let partial: i64 = sql::scalar(
            &mut pool.clone(),
            &format!("SELECT count(*) FROM {table} WHERE node_id=$1"),
            params![rollback_node],
        )
        .await
        .unwrap();
        assert_eq!(partial, 0, "事务失败不得留下{table}或消耗token");
    }
    sql::execute(
        &mut pool.clone(),
        "DROP TRIGGER fail_inventory_projection ON startup_inventories",
        params![],
    )
    .await
    .unwrap();
    assert!(
        !store::ingest_inventory(&pool, rollback_node, &rollback, now)
            .await
            .unwrap()
    );
    let last = make(startup, 2, now, "linux");
    store::ingest_inventory(&pool, node, &last, now)
        .await
        .unwrap();
    let limited = make(startup, 3, now, "linux");
    assert_eq!(
        store::ingest_inventory(&pool, node, &limited, now)
            .await
            .unwrap_err()
            .code(),
        tonic::Code::ResourceExhausted
    );
    assert!(
        !store::ingest_inventory(&pool, node, &limited, now + chrono::Duration::seconds(30))
            .await
            .unwrap(),
        "跨副本配额随时间恢复"
    );
    telemetry.retain().await.unwrap();
    let history = telemetry
        .inventory(node)
        .await
        .unwrap_or_else(|_| panic!("历史清单查询失败"));
    assert!(
        history
            .samples
            .iter()
            .any(|entry| entry["inventory"]["os"] == "darwin"),
        "保留周期内被新revision替代的正文不能清扫"
    );
    sql::execute(
        &mut pool.clone(),
        "UPDATE inventory_deliveries SET received_at=now()-interval '181 days' WHERE node_id=$1",
        params![node],
    )
    .await
    .unwrap();
    sql::execute(
        &mut pool.clone(),
        "UPDATE startup_inventories SET received_at=now()-interval '181 days' WHERE node_id=$1",
        params![node],
    )
    .await
    .unwrap();
    telemetry
        .retain_startups(&[(node, Some(startup))])
        .await
        .unwrap();
    telemetry.retain().await.unwrap();
    let retained: i64 = sql::scalar(
        &mut pool.clone(),
        "SELECT count(*) FROM inventory_deliveries WHERE node_id=$1",
        params![node],
    )
    .await
    .unwrap();
    assert_eq!(
        retained, 1,
        "历史按接收时间清扫，当前startup最后回执单独保护"
    );
    assert!(
        telemetry
            .latest_for_startup(node, startup)
            .await
            .unwrap_or_else(|_| panic!("当前startup查询失败"))
            .inventory
            .is_some()
    );
    assert!(
        telemetry
            .latest_for_startup(node, old_startup)
            .await
            .unwrap_or_else(|_| panic!("历史startup查询失败"))
            .inventory
            .is_none()
    );
    telemetry.purge(&[node], &[]).await.unwrap();
    assert_eq!(
        store::ingest_inventory(&pool, node, &observation, now)
            .await
            .unwrap_err()
            .code(),
        tonic::Code::PermissionDenied,
        "删除围栏优先于回执重放"
    );
    let remaining: i64 = sql::scalar(
        &mut pool.clone(),
        "SELECT count(*) FROM inventory_deliveries WHERE node_id=$1",
        params![node],
    )
    .await
    .unwrap();
    assert_eq!(remaining, 0);
    let other: i64 = sql::scalar(
        &mut pool.clone(),
        "SELECT count(*) FROM inventory_deliveries d JOIN startup_inventories s USING(node_id) JOIN inventory_versions v ON v.node_id=d.node_id AND v.digest=d.inventory_digest WHERE d.node_id=$1",
        params![rollback_node],
    )
    .await
    .unwrap();
    assert_eq!(other, 1, "删除目标节点不能清除另一节点的正文与引用");
    let cleanup = sql::execute(
        &mut admin,
        &format!("DROP DATABASE {name} WITH (FORCE)"),
        params![],
    )
    .await;
    cleanup.unwrap();
}

#[tokio::test]
#[ignore = "requires isolated TimescaleDB administrator database"]
async fn inventory_integrity_upgrade_rejects_orphans_without_partial_migration()
-> anyhow::Result<()> {
    use anyhow::ensure;
    use toasty::migration::{MigrationFile, MigrationSet};

    let url = std::env::var("ARTIFACTORY_TEST_DATABASE_URL")?;
    let mut admin = connect(&url, Default::default(), PoolSettings::new(2)).await?;
    for orphan in ["inventory_deliveries", "startup_inventories"] {
        let name = format!("inventory_upgrade_{}", Uuid::new_v4().simple());
        sql::execute(&mut admin, &format!("CREATE DATABASE {name}"), params![]).await?;
        let result = async {
            let pool = connect(
                &with_database(&url, &name)?,
                store::models(),
                PoolSettings::new(4),
            )
            .await?;
            artifactory_database::require_timescale(&pool).await?;
            sql::scalar::<bool>(&mut pool.clone(), "SELECT _timescaledb_functions.stop_background_workers()", params![]).await?;
            // 仅应用当前集合中0004之前的原始SQL和身份。
            let files: &'static [MigrationFile] = store::MIGRATIONS.migrations();
            let integrity = files.iter().position(|file| file.name() == "0004_inventory_integrity.sql").unwrap();
            ensure!(integrity == 4 && files[integrity].id() == 2026112601);
            let before = MigrationSet::new(&files[..integrity]);
            let target = MigrationSet::new(&files[..=integrity]);
            artifactory_database::migrate(&pool, &before).await?;
            let observation = artifactory_emissary_protocol::InventoryObservation::new(
                Uuid::new_v4(), 0, chrono::Utc::now(), 5,
                &artifactory_emissary_protocol::telemetry::Inventory {
                    os: "linux".into(), arch: "arm64".into(), ..Default::default()
                },
            )?;
            let node = Uuid::new_v4();
            store::ingest_inventory(&pool, node, &observation, chrono::Utc::now()).await?;
            let removed = if orphan == "inventory_deliveries" { "startup_inventories" } else { "inventory_deliveries" };
            sql::execute(&mut pool.clone(), &format!("DELETE FROM {removed} WHERE node_id=$1"), params![node]).await?;
            let ledger: String = sql::scalar(&mut pool.clone(), "SELECT jsonb_agg(to_jsonb(m) ORDER BY id)::text FROM __toasty_migrations m", params![]).await?;
            let original: String = sql::scalar(&mut pool.clone(), &format!("SELECT to_jsonb(r)::text FROM {orphan} r WHERE node_id=$1"), params![node]).await?;
            let body: serde_json::Value = sql::scalar(&mut pool.clone(), "SELECT to_jsonb(v) FROM inventory_versions v WHERE node_id=$1", params![node]).await?;
            let ingress: serde_json::Value = sql::scalar(&mut pool.clone(), "SELECT to_jsonb(i) FROM inventory_ingress i WHERE node_id=$1", params![node]).await?;
            sql::execute(&mut pool.clone(), "DELETE FROM inventory_versions WHERE node_id=$1", params![node]).await?;
            let error = artifactory_database::migrate(&pool, &target).await.err()
                .ok_or_else(||anyhow::anyhow!("孤立引用未阻止迁移"))?;
            ensure!(artifactory_database::constraint(error.as_ref()) == Some(artifactory_database::Constraint::ForeignKey), "{orphan}必须拒绝升级");
            let after: String = sql::scalar(&mut pool.clone(), "SELECT jsonb_agg(to_jsonb(m) ORDER BY id)::text FROM __toasty_migrations m", params![]).await?;
            ensure!(after == ledger, "失败升级不得推进迁移台账");
            let retained: String = sql::scalar(&mut pool.clone(), &format!("SELECT to_jsonb(r)::text FROM {orphan} r WHERE node_id=$1"), params![node]).await?;
            ensure!(retained == original, "失败升级不得修改原引用");
            let ingress_after: serde_json::Value = sql::scalar(&mut pool.clone(), "SELECT to_jsonb(i) FROM inventory_ingress i WHERE node_id=$1", params![node]).await?;
            ensure!(ingress_after == ingress, "失败升级不得修改准入预算");
            artifactory_database::validate(&pool, &before).await?;
            ensure!(artifactory_database::validate(&pool, &target).await.is_err());
            let partial: i64 = sql::scalar(&mut pool.clone(), "SELECT (SELECT count(*) FROM pg_class WHERE relnamespace=current_schema()::regnamespace AND relname IN ('inventory_delivery_content','startup_inventory_content'))+(SELECT count(*) FROM pg_constraint WHERE connamespace=current_schema()::regnamespace AND conname IN ('inventory_delivery_content','startup_inventory_content'))", params![]).await?;
            ensure!(partial == 0, "失败升级必须回滚全部索引与约束");
            let absent: i64 = sql::scalar(&mut pool.clone(), "SELECT count(*) FROM inventory_versions WHERE node_id=$1", params![node]).await?;
            ensure!(absent == 0, "失败升级不得伪造正文");
            // 仅验收夹具恢复先前保存的原正文；产品迁移不自动修复数据。
            sql::execute(&mut pool.clone(), "INSERT INTO inventory_versions(node_id,digest,first_seen,inventory) SELECT node_id,digest,first_seen,inventory FROM jsonb_populate_record(NULL::inventory_versions,$1::jsonb)", params![body.clone()]).await?;
            artifactory_database::migrate(&pool, &target).await?;
            artifactory_database::validate(&pool, &target).await?;
            let restored: serde_json::Value = sql::scalar(&mut pool.clone(), "SELECT to_jsonb(v) FROM inventory_versions v WHERE node_id=$1", params![node]).await?;
            ensure!(restored == body, "重试升级不得修改恢复的原正文");
            let retained: String = sql::scalar(&mut pool.clone(), &format!("SELECT to_jsonb(r)::text FROM {orphan} r WHERE node_id=$1"), params![node]).await?;
            ensure!(retained == original, "重试升级不得修改原引用");
            let validated: i64 = sql::scalar(&mut pool.clone(), "SELECT count(*) FROM pg_constraint WHERE connamespace=current_schema()::regnamespace AND conname IN ('inventory_delivery_content','startup_inventory_content') AND convalidated AND contype='f' AND NOT condeferrable AND confdeltype='r'", params![]).await?;
            ensure!(validated == 2, "恢复原正文后两类完整外键必须生效");
            let applied: String = sql::scalar(&mut pool.clone(), "SELECT jsonb_agg(to_jsonb(m) ORDER BY id)::text FROM __toasty_migrations m", params![]).await?;
            artifactory_database::migrate(&pool, &target).await?;
            let reapplied: String = sql::scalar(&mut pool.clone(), "SELECT jsonb_agg(to_jsonb(m) ORDER BY id)::text FROM __toasty_migrations m", params![]).await?;
            ensure!(applied == reapplied, "已提交迁移重放不得重写台账");
            Ok::<(), anyhow::Error>(())
        }.await;
        sql::execute(
            &mut admin,
            &format!("DROP DATABASE {name} WITH (FORCE)"),
            params![],
        )
        .await?;
        let absent: i64 = sql::scalar(
            &mut admin,
            "SELECT count(*) FROM pg_database WHERE datname=$1",
            params![name],
        )
        .await?;
        ensure!(absent == 0, "临时数据库删除必须读回");
        result?;
    }
    Ok(())
}

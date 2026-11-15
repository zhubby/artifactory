use artifactory_database::{PoolSettings, connect, params, sql, with_database};
use artifactory_telemetry_model::{Batch, HostSample, ProcessSample};
use artifactory_telemetry_service::store::{self, TelemetryStore};
use chrono::{Duration, Utc};
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires isolated TimescaleDB 2.32 administrator database"]
async fn purging_columnstore_and_expired_raw_preserves_other_identities_and_fences_replay() {
    let url = std::env::var("ARTIFACTORY_TEST_DATABASE_URL").unwrap();
    let mut admin = connect(&url, Default::default(), PoolSettings::new(2))
        .await
        .unwrap();
    let database = format!("purge_{}", Uuid::new_v4().simple());
    sql::execute(
        &mut admin,
        &format!("CREATE DATABASE {database}"),
        params![],
    )
    .await
    .unwrap();
    let pool = connect(
        &with_database(&url, &database).unwrap(),
        store::models(),
        PoolSettings::new(8),
    )
    .await
    .unwrap();
    artifactory_database::require_timescale(&pool)
        .await
        .unwrap();
    artifactory_database::migrate(&pool, &store::MIGRATIONS)
        .await
        .unwrap();
    let target = Uuid::new_v4();
    let survivor = Uuid::new_v4();
    let attempt = Uuid::new_v4();
    let other_attempt = Uuid::new_v4();
    let at = Utc::now() - Duration::minutes(15);
    let sample = |id, sampled_at| Batch {
        id: Uuid::new_v4(),
        session_id: id,
        sampled_at,
        monotonic_ms: 1,
        dropped_batches: 0,
        omitted_processes: 0,
        inventory: None,
        host: Some(HostSample {
            cpu_percent: Some(25.0),
            ..HostSample::default()
        }),
        processes: vec![ProcessSample {
            attempt_id: if id == target { attempt } else { other_attempt },
            pid: 17,
            started_at: 1,
            parent_pid: None,
            step: None,
            cpu_percent: Some(12.0),
            rss_bytes: Some(30),
            read_bytes: None,
            written_bytes: None,
            status: "running".into(),
        }],
    };
    for node in [target, survivor] {
        for timestamp in [at, at - Duration::days(45)] {
            let batch = sample(node, timestamp);
            store::ingest(
                &pool,
                node,
                &batch,
                &serde_json::to_vec(&batch).unwrap(),
                Utc::now(),
            )
            .await
            .unwrap();
        }
    }
    for view in ["host_minutes", "build_minutes", "host_hours", "build_hours"] {
        sql::execute(
            &mut pool.clone(),
            &format!("CALL refresh_continuous_aggregate('{view}',$1::timestamptz,$2::timestamptz)"),
            params![at - Duration::days(46), at + Duration::hours(2)],
        )
        .await
        .unwrap();
    }
    // Retention drops raw chunks without invalidating the long-lived aggregates.
    for table in ["host_samples", "build_samples", "process_samples"] {
        sql::execute(
            &mut pool.clone(),
            &format!("SELECT drop_chunks('{table}',older_than=>now()-interval '7 days')"),
            params![],
        )
        .await
        .unwrap();
    }
    let before:serde_json::Value=sql::scalar(&mut pool.clone(),"SELECT jsonb_build_object('host',(SELECT jsonb_agg(to_jsonb(h) ORDER BY bucket) FROM host_hours h WHERE node_id=$1),'build',(SELECT jsonb_agg(to_jsonb(h) ORDER BY bucket) FROM build_hours h WHERE node_id=$1))",params![survivor]).await.unwrap();
    let chunks:Vec<String>=sql::scalars(&mut pool.clone(),"SELECT format('%I.%I',chunk_schema,chunk_name) FROM timescaledb_information.chunks WHERE hypertable_name IN ('host_samples','build_samples','process_samples')",params![]).await.unwrap();
    for chunk in chunks {
        sql::execute(
            &mut pool.clone(),
            &format!("CALL convert_to_columnstore('{chunk}'::regclass)"),
            params![],
        )
        .await
        .unwrap();
    }
    let store = TelemetryStore::new(pool.clone(), pool.clone());
    store.purge(&[target], &[attempt]).await.unwrap();
    store.purge(&[target], &[attempt]).await.unwrap();
    for table in [
        "host_samples",
        "process_samples",
        "build_samples",
        "host_minutes",
        "host_hours",
        "build_minutes",
        "build_hours",
        "telemetry_batches",
        "latest_hosts",
        "startup_resources",
    ] {
        let count: i64 = sql::scalar(
            &mut pool.clone(),
            &format!("SELECT count(*) FROM {table} WHERE node_id=$1"),
            params![target],
        )
        .await
        .unwrap();
        assert_eq!(count, 0, "{table}");
    }
    let after:serde_json::Value=sql::scalar(&mut pool.clone(),"SELECT jsonb_build_object('host',(SELECT jsonb_agg(to_jsonb(h) ORDER BY bucket) FROM host_hours h WHERE node_id=$1),'build',(SELECT jsonb_agg(to_jsonb(h) ORDER BY bucket) FROM build_hours h WHERE node_id=$1))",params![survivor]).await.unwrap();
    assert_eq!(
        before, after,
        "unrelated aggregates whose raw data expired must remain identical"
    );
    let late = sample(target, at);
    assert_eq!(
        store
            .ingest(
                target,
                &late,
                &serde_json::to_vec(&late).unwrap(),
                Utc::now()
            )
            .await
            .unwrap_err()
            .code(),
        tonic::Code::PermissionDenied
    );
    let mut late = sample(survivor, at);
    late.processes[0].attempt_id = attempt;
    assert_eq!(
        store
            .ingest(
                survivor,
                &late,
                &serde_json::to_vec(&late).unwrap(),
                Utc::now()
            )
            .await
            .unwrap_err()
            .code(),
        tonic::Code::PermissionDenied
    );
    let survivor_batch = sample(survivor, at + Duration::seconds(1));
    store
        .ingest(
            survivor,
            &survivor_batch,
            &serde_json::to_vec(&survivor_batch).unwrap(),
            Utc::now(),
        )
        .await
        .unwrap();
    drop(store);
    drop(pool);
    sql::execute(
        &mut admin,
        &format!("DROP DATABASE {database} WITH (FORCE)"),
        params![],
    )
    .await
    .unwrap();
}

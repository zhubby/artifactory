//! [INPUT]: 隔离TimescaleDB、真实mTLS服务和受保护的Prometheus端点。
//! [OUTPUT]: 验证接收与重放计数、有限拒收标签、并发准入和许可回收。
//! [POS]: 单一集成进程隔离全局计数器，不修改共享测试容器。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_database::{PoolSettings, connect, params, sql, with_database};
use artifactory_emissary_protocol::{
    InventoryObservation, TelemetryBatch,
    telemetry::{Batch, HostSample, Inventory, MAX_BATCH_BYTES, ProcessSample},
    telemetry_client::TelemetryClient,
    telemetry_negotiation::{self, RejectionReason},
    telemetry_server::TelemetryServer,
};
use artifactory_telemetry_service::{Service, query, store};
use chrono::{Duration, Utc};
use rcgen::{
    BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use std::{collections::BTreeMap, time::Duration as Wait};
use tonic::transport::{
    Certificate, Channel, ClientTlsConfig, Endpoint, Identity, Server, ServerTlsConfig,
};
use tower::ServiceExt;
use uuid::Uuid;

const TOKEN: &str = "metrics-integration-service-token";
const PREFIX: &str = "artifactory_telemetry_";
const REASONS: &[&str] = &[
    "schema_unsupported",
    "encoding_unsupported",
    "overloaded",
    "database_unavailable",
    "authentication_failed",
    "authorization_denied",
    "startup_unregistered",
    "clock_future",
    "expired_sample",
    "malformed",
    "bounds_exceeded",
    "id_content_conflict",
    "identity_retired",
    "unclassified",
];

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL on isolated TimescaleDB; owns process counters"]
async fn real_rpc_metrics_count_outcomes_and_bound_concurrent_ingest() {
    let url = std::env::var("ARTIFACTORY_TEST_DATABASE_URL").unwrap();
    let mut admin = connect(&url, Default::default(), PoolSettings::new(2))
        .await
        .unwrap();
    let name = format!("ingest_metrics_{}", Uuid::new_v4().simple());
    sql::execute(&mut admin, &format!("CREATE DATABASE {name}"), params![])
        .await
        .unwrap();
    let pool = connect(
        &with_database(&url, &name).unwrap(),
        store::models(),
        PoolSettings::new(12),
    )
    .await
    .unwrap();
    artifactory_database::require_timescale(&pool)
        .await
        .unwrap();
    assert!(
        sql::scalar::<bool>(
            &mut pool.clone(),
            "SELECT _timescaledb_functions.stop_background_workers()",
            params![]
        )
        .await
        .unwrap()
    );
    artifactory_database::migrate(&pool, &store::MIGRATIONS)
        .await
        .unwrap();
    sql::execute(&mut pool.clone(), "DO $fixture$ BEGIN CREATE TABLE nodes(id uuid PRIMARY KEY,certificate_hash text,revoked boolean NOT NULL DEFAULT false,certificate_expires_at timestamptz NOT NULL DEFAULT now()+interval '1 day'); CREATE TABLE attempts(id uuid PRIMARY KEY,node_id uuid NOT NULL); CREATE TABLE node_startups(node_id uuid NOT NULL,startup_id uuid NOT NULL,PRIMARY KEY(node_id,startup_id)); END $fixture$", params![]).await.unwrap();
    let node = Uuid::new_v4();
    let startup = Uuid::new_v4();
    let service = Service {
        store: store::TelemetryStore::new(pool.clone(), pool.clone()),
        authorization: store::BusinessAuthorizationRepository::new(pool.clone()),
    };
    let router = query::router(service.clone(), TOKEN.into());
    let (mut rpc, stop, server) = serve(service, &pool, node).await;
    let initial = scrape(&router).await;
    assert_eq!(initial["accepted_total"], 0);
    assert_eq!(initial["replayed_total"], 0);
    assert_eq!(initial["rejected_total"], 0);
    assert_eq!(initial["ingest_inflight"], 0);
    rpc.negotiate(telemetry_negotiation::offer()).await.unwrap();
    assert_eq!(scrape(&router).await, initial, "成功协商不计持久接收");

    let batch = batch(startup);
    let first = rpc.upload(request(&batch)).await.unwrap().into_inner();
    assert!(!first.duplicate);
    assert!(
        rpc.upload(request(&batch))
            .await
            .unwrap()
            .into_inner()
            .duplicate
    );
    sql::execute(
        &mut pool.clone(),
        "INSERT INTO node_startups(node_id,startup_id) VALUES($1,$2)",
        params![node, startup],
    )
    .await
    .unwrap();
    let inventory = InventoryObservation::new(
        startup,
        0,
        Utc::now(),
        1,
        &Inventory {
            os: "linux".into(),
            arch: "arm64".into(),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        !rpc.upload_inventory(inventory.clone())
            .await
            .unwrap()
            .into_inner()
            .duplicate
    );
    assert!(
        rpc.upload_inventory(inventory.clone())
            .await
            .unwrap()
            .into_inner()
            .duplicate
    );
    let stored = scrape(&router).await;
    assert_eq!(stored["accepted_total"], 2, "采样和清单共用接收计数");
    assert_eq!(stored["replayed_total"], 2, "重放不重复计入接收");

    let mut unsupported = request(&batch);
    unsupported.schema_version = 99;
    rejected(
        rpc.upload(unsupported).await.unwrap_err(),
        RejectionReason::SchemaUnsupported,
    );
    rejected(
        rpc.upload(TelemetryBatch {
            json: b"/private/metrics-secret operation-123".to_vec(),
            schema_version: 1,
        })
        .await
        .unwrap_err(),
        RejectionReason::Malformed,
    );
    rejected(
        rpc.upload(TelemetryBatch {
            json: vec![b'x'; MAX_BATCH_BYTES + 1],
            schema_version: 1,
        })
        .await
        .unwrap_err(),
        RejectionReason::BoundsExceeded,
    );
    let mut invalid = batch.clone();
    invalid.sampled_at = Utc::now() + Duration::minutes(6);
    rejected(
        rpc.upload(request(&invalid)).await.unwrap_err(),
        RejectionReason::ClockFuture,
    );
    invalid.sampled_at = Utc::now() - Duration::hours(49);
    rejected(
        rpc.upload(request(&invalid)).await.unwrap_err(),
        RejectionReason::ExpiredSample,
    );
    invalid = batch.clone();
    invalid.processes.push(ProcessSample {
        attempt_id: Uuid::new_v4(),
        pid: 10,
        started_at: 1,
        parent_pid: None,
        step: None,
        cpu_percent: None,
        rss_bytes: None,
        read_bytes: None,
        written_bytes: None,
        status: "running".into(),
    });
    rejected(
        rpc.upload(request(&invalid)).await.unwrap_err(),
        RejectionReason::AuthorizationDenied,
    );
    invalid = batch.clone();
    invalid.monotonic_ms += 1;
    rejected(
        rpc.upload(request(&invalid)).await.unwrap_err(),
        RejectionReason::IdContentConflict,
    );
    let unregistered = InventoryObservation::new(
        Uuid::new_v4(),
        0,
        Utc::now(),
        1,
        &Inventory {
            os: "linux".into(),
            arch: "arm64".into(),
            ..Default::default()
        },
    )
    .unwrap();
    rejected(
        rpc.upload_inventory(unregistered).await.unwrap_err(),
        RejectionReason::StartupUnregistered,
    );
    sql::execute(
        &mut pool.clone(),
        "INSERT INTO telemetry_deleted_identities(kind,id) VALUES('node',$1)",
        params![node],
    )
    .await
    .unwrap();
    rejected(
        rpc.upload(request(&batch)).await.unwrap_err(),
        RejectionReason::IdentityRetired,
    );
    sql::execute(
        &mut pool.clone(),
        "DELETE FROM telemetry_deleted_identities WHERE id=$1",
        params![node],
    )
    .await
    .unwrap();
    sql::execute(
        &mut pool.clone(),
        "UPDATE nodes SET revoked=true WHERE id=$1",
        params![node],
    )
    .await
    .unwrap();
    rejected(
        rpc.negotiate(telemetry_negotiation::offer())
            .await
            .unwrap_err(),
        RejectionReason::AuthenticationFailed,
    );
    sql::execute(
        &mut pool.clone(),
        "UPDATE nodes SET revoked=false WHERE id=$1",
        params![node],
    )
    .await
    .unwrap();
    let failures = scrape(&router).await;
    assert_eq!(failures["rejected_total"], 10);
    for reason in [
        "schema_unsupported",
        "malformed",
        "bounds_exceeded",
        "clock_future",
        "expired_sample",
        "authorization_denied",
        "id_content_conflict",
        "startup_unregistered",
        "identity_retired",
        "authentication_failed",
    ] {
        assert_eq!(
            failures[&reason_key(reason)],
            1,
            "拒收必须只归属一个有限原因"
        );
    }
    assert_eq!(failures["ingest_inflight"], 0, "全部错误路径释放许可");

    // 只移动隔离库的授权表，验证真实数据库故障也进入有限分类。
    sql::execute(
        &mut pool.clone(),
        "ALTER TABLE nodes RENAME TO unavailable_nodes",
        params![],
    )
    .await
    .unwrap();
    rejected(
        rpc.negotiate(telemetry_negotiation::offer())
            .await
            .unwrap_err(),
        RejectionReason::DatabaseUnavailable,
    );
    sql::execute(
        &mut pool.clone(),
        "ALTER TABLE unavailable_nodes RENAME TO nodes",
        params![],
    )
    .await
    .unwrap();
    let restored = scrape(&router).await;
    assert_eq!(restored[&reason_key("database_unavailable")], 1);
    assert_eq!(restored["ingest_inflight"], 0);

    // 数据库锁保持真实授权查询在途；64个请求共用全部RPC的准入预算。
    let mut held_db = pool.clone();
    let mut held = held_db.transaction().await.unwrap();
    sql::execute(
        &mut held,
        "LOCK TABLE nodes IN ACCESS EXCLUSIVE MODE",
        params![],
    )
    .await
    .unwrap();
    let mut pending = tokio::task::JoinSet::new();
    for _ in 0..64 {
        let mut client = rpc.clone();
        pending.spawn(async move { client.negotiate(telemetry_negotiation::offer()).await });
    }
    tokio::time::timeout(Wait::from_secs(5), async {
        loop {
            let inflight = scrape(&router).await["ingest_inflight"];
            assert!(inflight <= 64);
            if inflight == 64 {
                break;
            }
            tokio::time::sleep(Wait::from_millis(10)).await;
        }
    })
    .await
    .expect("真实请求必须占满64个许可");
    rejected(
        rpc.upload(request(&batch)).await.unwrap_err(),
        RejectionReason::Overloaded,
    );
    rejected(
        rpc.upload_inventory(inventory).await.unwrap_err(),
        RejectionReason::Overloaded,
    );
    rejected(
        rpc.negotiate(telemetry_negotiation::offer())
            .await
            .unwrap_err(),
        RejectionReason::Overloaded,
    );
    assert_eq!(
        scrape(&router).await["ingest_inflight"],
        64,
        "超额拒收不能占用许可"
    );
    held.rollback().await.unwrap();
    drop(held_db);
    tokio::time::timeout(Wait::from_secs(10), async {
        while let Some(result) = pending.join_next().await {
            result.unwrap().unwrap();
        }
    })
    .await
    .unwrap();
    assert_eq!(scrape(&router).await["ingest_inflight"], 0);
    rpc.negotiate(telemetry_negotiation::offer()).await.unwrap();
    let finished = scrape(&router).await;
    assert_eq!(finished["rejected_total"], 14);
    assert_eq!(finished[&reason_key("overloaded")], 3);
    assert_eq!(finished["accepted_total"], 2);
    assert_eq!(finished["replayed_total"], 2);
    assert_eq!(finished["ingest_inflight"], 0);
    assert_eq!(
        REASONS
            .iter()
            .map(|reason| finished[&reason_key(reason)])
            .sum::<u64>(),
        14
    );

    drop(rpc);
    stop.send(()).unwrap();
    server.await.unwrap();
    drop(router);
    drop(pool);
    sql::execute(
        &mut admin,
        &format!("DROP DATABASE {name} WITH (FORCE)"),
        params![],
    )
    .await
    .unwrap();
}

fn batch(startup: Uuid) -> Batch {
    Batch {
        id: Uuid::new_v4(),
        session_id: startup,
        sampled_at: Utc::now(),
        monotonic_ms: 1,
        dropped_batches: 0,
        omitted_processes: 0,
        host: Some(HostSample::default()),
        processes: vec![],
        inventory: None,
    }
}
fn request(batch: &Batch) -> TelemetryBatch {
    TelemetryBatch {
        json: serde_json::to_vec(batch).unwrap(),
        schema_version: 1,
    }
}
fn rejected(status: tonic::Status, expected: RejectionReason) {
    assert_eq!(RejectionReason::from_status(&status), Some(expected));
}
fn reason_key(reason: &str) -> String {
    format!("rejection_reason_total{{reason=\"{reason}\"}}")
}

async fn scrape(router: &axum::Router) -> BTreeMap<String, u64> {
    let response = router
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/metrics")
                .header("authorization", format!("Bearer {TOKEN}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    let text = std::str::from_utf8(&body).unwrap();
    let mut values = BTreeMap::new();
    for line in text.lines().filter(|line| line.starts_with(PREFIX)) {
        let (key, value) = line.split_once(' ').expect("Prometheus样本格式");
        assert!(
            values
                .insert(
                    key.trim_start_matches(PREFIX).into(),
                    value.parse::<u64>().unwrap()
                )
                .is_none(),
            "指标样本不得重复"
        );
    }
    let mut allowed = vec![
        "accepted_total".to_owned(),
        "replayed_total".into(),
        "rejected_total".into(),
        "ingest_inflight".into(),
    ];
    allowed.extend(REASONS.iter().map(|reason| reason_key(reason)));
    assert_eq!(
        values
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>(),
        allowed.into_iter().collect(),
        "指标与标签只来自固定词汇，不能出现节点、路径、操作ID或错误正文"
    );
    assert!(!text.contains("metrics-secret"));
    assert!(!text.contains("operation-123"));
    values
}

async fn serve(
    service: Service,
    pool: &toasty::Db,
    node: Uuid,
) -> (
    TelemetryClient<Channel>,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
) {
    let ca_key = KeyPair::generate().unwrap();
    let mut ca_params = CertificateParams::default();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    let ca = ca_params.self_signed(&ca_key).unwrap();
    let issuer = Issuer::new(ca_params, ca_key);
    let server_key = KeyPair::generate().unwrap();
    let mut params = CertificateParams::new(vec!["localhost".into()]).unwrap();
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let server = params.signed_by(&server_key, &issuer).unwrap();
    let client_key = KeyPair::generate().unwrap();
    let mut params = CertificateParams::default();
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
    let client = params.signed_by(&client_key, &issuer).unwrap();
    sql::execute(
        &mut pool.clone(),
        "INSERT INTO nodes(id,certificate_hash) VALUES($1,$2)",
        params![
            node,
            artifactory_service_transport::crypto::hash(client.der())
        ],
    )
    .await
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let tls = ServerTlsConfig::new()
        .identity(Identity::from_pem(server.pem(), server_key.serialize_pem()))
        .client_ca_root(Certificate::from_pem(ca.pem()));
    let task = tokio::spawn(async move {
        Server::builder()
            .tls_config(tls)
            .unwrap()
            .add_service(TelemetryServer::new(service))
            .serve_with_incoming_shutdown(
                tokio_stream::wrappers::TcpListenerStream::new(listener),
                async {
                    let _ = stopped.await;
                },
            )
            .await
            .unwrap();
    });
    let tls = ClientTlsConfig::new()
        .domain_name("localhost")
        .ca_certificate(Certificate::from_pem(ca.pem()))
        .identity(Identity::from_pem(client.pem(), client_key.serialize_pem()));
    let channel = Endpoint::from_shared(format!("https://{address}"))
        .unwrap()
        .timeout(Wait::from_secs(20))
        .tls_config(tls)
        .unwrap()
        .connect()
        .await
        .unwrap();
    (TelemetryClient::new(channel), stop, task)
}

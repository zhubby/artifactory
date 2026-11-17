//! [INPUT]: 隔离数据库、节点元数据和 mTLS 规划夹具。
//! [OUTPUT]: 验证节点快照与租户查询，提供候选和工具夹具。
//! [POS]: placement 和 tools 子模块复用本模块的隔离服务。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
#[path = "node_placement.rs"]
mod placement;
#[path = "node_tools.rs"]
mod tools;

use super::*;
use artifactory_telemetry_model::nodes::MetadataSnapshot;
use std::collections::BTreeMap;

async fn connect_snapshot(
    device: &mut Device,
    snapshot: &MetadataSnapshot,
) -> Result<
    (
        mpsc::Sender<pb::EmissaryEvent>,
        tonic::Streaming<pb::Command>,
    ),
    tonic::Status,
> {
    let (events, receiver) = mpsc::channel(16);
    events
        .send(pb::EmissaryEvent {
            emissary_id: device.id.to_string(),
            kind: "hello".into(),
            protocol_major: 1,
            payload_json: json!({"startup":snapshot}).to_string(),
            ..Default::default()
        })
        .await
        .unwrap();
    let commands = device
        .client
        .connect(ReceiverStream::new(receiver))
        .await?
        .into_inner();
    Ok((events, commands))
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn startup_ack_reconnect_replace_and_retired_process_fencing() {
    let database = Database::with_tenant().await;
    let certificates = Certificates::new();
    let replica = Replica::start(database.state.clone(), &certificates).await;
    let mut device = Device::new(&database.pool, &replica, &certificates).await;
    device
        .events
        .send(pb::EmissaryEvent {
            emissary_id: device.id.to_string(),
            kind: "heartbeat".into(),
            command_id: Uuid::new_v4().to_string(),
            payload_json: json!({"capabilities":["git","bash","legacy-probe"],"ota":{}})
                .to_string(),
            ..Default::default()
        })
        .await
        .unwrap();
    device.next("emissary_health").await;
    assert_eq!(
        sql::scalar::<Value>(
            &mut database.pool.clone(),
            "SELECT capabilities FROM nodes WHERE id=$1",
            params![device.id]
        )
        .await
        .unwrap(),
        json!(["git", "bash", "legacy-probe"]),
        "legacy nodes retain their heartbeat capability updates until upgraded"
    );
    let snapshot = MetadataSnapshot::new(
        Uuid::new_v4(),
        BTreeMap::from([
            ("system.os".into(), json!("linux")),
            ("custom.site".into(), json!("lab")),
            (
                "tools.rust".into(),
                json!({"available":true,"version":"1.91"}),
            ),
        ]),
        vec!["git".into(), "bash".into()],
    )
    .unwrap();
    let (events, mut original_commands) = connect_snapshot(&mut device, &snapshot).await.unwrap();
    assert_eq!(
        original_commands.message().await.unwrap().unwrap().kind,
        "metadata_ack"
    );
    events
        .send(pb::EmissaryEvent {
            emissary_id: device.id.to_string(),
            kind: "heartbeat".into(),
            command_id: Uuid::new_v4().to_string(),
            payload_json: json!({"capabilities":["injected-capability"],"ota":{}}).to_string(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(
        original_commands.message().await.unwrap().unwrap().kind,
        "emissary_health"
    );
    assert_eq!(
        sql::scalar::<Value>(
            &mut database.pool.clone(),
            "SELECT capabilities FROM nodes WHERE id=$1",
            params![device.id]
        )
        .await
        .unwrap(),
        json!(snapshot.capabilities),
        "new nodes change static capabilities only with a new startup snapshot"
    );
    let first = database
        .state
        .store
        .node_metadata(device.id)
        .await
        .unwrap()
        .unwrap();
    // Ignore the ACK and resend exactly the cached data.
    events
        .send(pb::EmissaryEvent {
            emissary_id: device.id.to_string(),
            kind: "metadata".into(),
            payload_json: serde_json::to_string(&snapshot).unwrap(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(
        original_commands
            .message()
            .await
            .unwrap()
            .unwrap()
            .request_id,
        snapshot.startup_id.to_string()
    );
    let (_reconnect, mut commands) = connect_snapshot(&mut device, &snapshot).await.unwrap();
    assert_eq!(
        commands.message().await.unwrap().unwrap().kind,
        "metadata_ack"
    );
    assert_eq!(
        database
            .state
            .store
            .node_metadata(device.id)
            .await
            .unwrap()
            .unwrap(),
        first,
        "reconnect and ACK retries must not rewrite the snapshot"
    );
    let replacement = MetadataSnapshot::new(
        Uuid::new_v4(),
        BTreeMap::from([("system.os".into(), json!("linux"))]),
        vec!["git".into()],
    )
    .unwrap();
    let (_current, mut commands) = connect_snapshot(&mut device, &replacement).await.unwrap();
    assert_eq!(
        commands.message().await.unwrap().unwrap().kind,
        "metadata_ack"
    );
    let metadata = database
        .state
        .store
        .node_metadata(device.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        metadata["metadata"],
        json!({"system.os":"linux"}),
        "a new startup fully replaces old tools and custom fields"
    );
    let _ = events
        .send(pb::EmissaryEvent {
            emissary_id: device.id.to_string(),
            kind: "metadata".into(),
            payload_json: serde_json::to_string(&snapshot).unwrap(),
            ..Default::default()
        })
        .await;
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            original_commands.message()
        )
        .await
        .unwrap()
        .is_err(),
        "the superseded stream must not receive an ACK for old metadata"
    );
    assert!(
        connect_snapshot(&mut device, &snapshot).await.is_err(),
        "a retired process cannot recover ownership"
    );
    let admin:Value=sql::scalar(&mut database.pool.clone(),"SELECT jsonb_build_object('labels',labels,'signing_profiles',signing_profiles,'slots',slots) FROM nodes WHERE id=$1",params![device.id]).await.unwrap();
    assert_eq!(admin, json!({"labels":{},"signing_profiles":[],"slots":1}));
    replica.stop().await;
    database.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn metadata_detail_requires_a_viewer_session_and_stays_out_of_lists() {
    let database = Database::with_tenant().await;
    database.fixture().await;
    let node = database.node().await;
    let user = Uuid::new_v4();
    let startup = Uuid::new_v4();
    sql::execute(&mut database.pool.clone(), "UPDATE nodes SET startup_id=$2,metadata=jsonb_build_object('custom.site','lab'),metadata_digest=repeat('a',64),metadata_received_at=clock_timestamp() WHERE id=$1",params![node,startup]).await.unwrap();
    sql::execute(&mut database.pool.clone(), "INSERT INTO users(id,username,password_hash,role) VALUES($1,'metadata-viewer','unused','viewer')",params![user]).await.unwrap();
    sql::execute(
        &mut database.pool.clone(),
        "INSERT INTO sessions VALUES($1,$2,now()+interval '1 hour')",
        params![crypto::hash("metadata-session"), user],
    )
    .await
    .unwrap();
    sql::execute(&mut database.pool.clone(), "INSERT INTO api_tokens(tenant_id,id,user_id,name,token_hash,project_ids,scopes,expires_at) VALUES('00000000-0000-0000-0000-000000000001',$1,$2,'metadata-test',$3,'{}','{read}',now()+interval '1 hour')",params![Uuid::new_v4(),user,crypto::hash("metadata-token")]).await.unwrap();
    let app = api::router(database.state.clone());
    let uri = format!("/api/v1/nodes/{node}/metadata");
    for (header, status) in [
        (None, StatusCode::UNAUTHORIZED),
        (
            Some(("authorization", "Bearer metadata-token")),
            StatusCode::FORBIDDEN,
        ),
        (
            Some(("cookie", "artifactory_session=metadata-session")),
            StatusCode::OK,
        ),
    ] {
        let mut request = Request::builder().uri(&uri).header(
            "X-Artifactory-Tenant",
            "00000000-0000-0000-0000-000000000001",
        );
        if let Some((name, value)) = header {
            request = request.header(name, value);
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), status, "metadata access with {header:?}");
        if status == StatusCode::OK {
            let bytes = axum::body::to_bytes(response.into_body(), 128 * 1024)
                .await
                .unwrap();
            let detail: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(detail["startup_id"], json!(startup));
            assert_eq!(detail["metadata"], json!({"custom.site":"lab"}));
            assert_eq!(detail["digest"], "a".repeat(64));
            assert!(detail["received_at"].is_string());
        }
    }
    for (path, expected) in [
        ("/api/v1/nodes".into(), StatusCode::OK),
        (
            format!("/api/v1/nodes/{}/metadata", Uuid::new_v4()),
            StatusCode::NOT_FOUND,
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header(
                        "X-Artifactory-Tenant",
                        "00000000-0000-0000-0000-000000000001",
                    )
                    .header("cookie", "artifactory_session=metadata-session")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        if expected == StatusCode::OK {
            let bytes = axum::body::to_bytes(response.into_body(), 128 * 1024)
                .await
                .unwrap();
            let nodes: Value = serde_json::from_slice(&bytes).unwrap();
            assert!(nodes[0].get("metadata").is_none());
        }
    }
    database.close().await;
}

async fn planner(
    database: &Database,
    replica: &Replica,
    certificates: &Certificates,
    build: Uuid,
) -> (pb::planning_client::PlanningClient<Channel>, Uuid, Value) {
    let (certificate, key) = certificates.device();
    let worker = Uuid::new_v4();
    let run = Uuid::new_v4();
    sql::execute(&mut database.pool.clone(),"INSERT INTO ai_workers(tenant_id,id,name,certificate_hash,certificate_expires_at) VALUES('00000000-0000-0000-0000-000000000001',$1,'test',$2,now()+interval '1 day')",params![worker,pki::certificate_hash(&certificate).unwrap()]).await.unwrap();
    sql::execute(&mut database.pool.clone(),"UPDATE builds SET status='planning',goal='test',planning_context=jsonb_build_object('pool_ids',jsonb_build_array('00000000-0000-0000-0000-000000000001'),'signing_profiles','[]'::jsonb) WHERE id=$1",params![build]).await.unwrap();
    sql::execute(&mut database.pool.clone(),"INSERT INTO planning_runs(id,build_id,worker_id,purpose,state,model_config,lease_hash,lease_expires_at,deadline_at) VALUES($1,$2,$3,'initial','leased','{}',$4,now()+interval '1 minute',now()+interval '5 minutes')",params![run,build,worker,crypto::hash("lease")]).await.unwrap();
    let identity = json!({"endpoint":replica.url,"ca":certificates.ca,"certificate":certificate,"key":key,"run_id":run,"lease_token":"lease"});
    let channel = Channel::from_shared(replica.url.clone())
        .unwrap()
        .tls_config(
            ClientTlsConfig::new()
                .domain_name("localhost")
                .ca_certificate(Certificate::from_pem(&certificates.ca))
                .identity(Identity::from_pem(certificate, key)),
        )
        .unwrap()
        .connect()
        .await
        .unwrap();
    (
        pb::planning_client::PlanningClient::new(channel),
        run,
        identity,
    )
}
async fn node_tool(
    client: &mut pb::planning_client::PlanningClient<Channel>,
    run: Uuid,
    name: &str,
    args: Value,
) -> Value {
    let reply = client
        .tool(pb::PlannerToolRequest {
            run_id: run.to_string(),
            lease_token: "lease".into(),
            name: name.into(),
            arguments_json: args.to_string(),
        })
        .await
        .unwrap()
        .into_inner();
    serde_json::from_str(&reply.result_json).unwrap()
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn mtls_workers_cannot_claim_other_tenant_tasks_or_use_their_leases() {
    let database = Database::with_tenant().await;
    let (project, build, _) = database.fixture().await;
    let certificates = Certificates::new();
    let replica = Replica::start(database.state.clone(), &certificates).await;
    let (mut first, run, _) = planner(&database, &replica, &certificates, build).await;
    let owner: Uuid = sql::scalar(
        &mut database.pool.clone(),
        "SELECT owner_id FROM tenants WHERE id='00000000-0000-0000-0000-000000000001'",
        params![],
    )
    .await
    .unwrap();
    let other_tenant = database
        .state
        .store
        .create_tenant(owner, "Other", Uuid::new_v4())
        .await
        .unwrap();
    let (certificate, key) = certificates.device();
    let worker = Uuid::new_v4();
    sql::execute(&mut database.pool.clone(), "INSERT INTO ai_workers(id,name,tenant_id,certificate_hash,certificate_expires_at) VALUES($1,'other',$2,$3,now()+interval '1 day')", params![worker,other_tenant,pki::certificate_hash(&certificate).unwrap()]).await.unwrap();
    let channel = Channel::from_shared(replica.url.clone())
        .unwrap()
        .tls_config(
            ClientTlsConfig::new()
                .domain_name("localhost")
                .ca_certificate(Certificate::from_pem(&certificates.ca))
                .identity(Identity::from_pem(certificate, key)),
        )
        .unwrap()
        .connect()
        .await
        .unwrap();
    let mut other = pb::planning_client::PlanningClient::new(channel);
    let repository = Uuid::new_v4();
    let snapshot = Uuid::new_v4();
    sql::execute(&mut database.pool.clone(), "INSERT INTO source_repositories(id,project_id,repository_url,cache_key) VALUES($1,$2,'https://example.com/repo.git',$3)", params![repository,project,repository.to_string()]).await.unwrap();
    sql::execute(&mut database.pool.clone(), "INSERT INTO source_snapshots(id,project_id,root_repository_id,commit_sha,manifest,digest) VALUES($1,$2,$3,$4,'{}',$5)", params![snapshot,project,repository,"0".repeat(40),"a".repeat(64)]).await.unwrap();
    sql::execute(
        &mut database.pool.clone(),
        "UPDATE builds SET source_snapshot_id=$2 WHERE id=$1",
        params![build, snapshot],
    )
    .await
    .unwrap();
    let secret = "tenant-a-model-secret";
    let config = json!({"key_ciphertext":crypto::seal(&database.state.settings.master_key,secret.as_bytes()).unwrap(),"max_calls":2,"timeout_seconds":60});
    sql::execute(&mut database.pool.clone(), "UPDATE planning_runs SET state='queued',worker_id=NULL,lease_hash=NULL,model_config=$2 WHERE id=$1", params![run,config]).await.unwrap();
    assert!(
        other
            .poll(pb::PlannerPollRequest::default())
            .await
            .unwrap()
            .into_inner()
            .run_id
            .is_empty()
    );
    let task = first
        .poll(pb::PlannerPollRequest::default())
        .await
        .unwrap()
        .into_inner();
    assert_eq!(task.run_id, run.to_string());
    assert_eq!(
        serde_json::from_str::<Value>(&task.model_config_json).unwrap()["api_key"],
        secret
    );
    let denied = other
        .tool(pb::PlannerToolRequest {
            run_id: task.run_id.clone(),
            lease_token: task.lease_token.clone(),
            name: "summarize_nodes".into(),
            arguments_json: "{}".into(),
        })
        .await
        .unwrap_err();
    assert_eq!(denied.code(), tonic::Code::FailedPrecondition);
    first
        .tool(pb::PlannerToolRequest {
            run_id: task.run_id,
            lease_token: task.lease_token,
            name: "summarize_nodes".into(),
            arguments_json: "{}".into(),
        })
        .await
        .unwrap();
    replica.stop().await;
    database.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn scoped_node_queries_page_and_keep_missing_telemetry_unknown() {
    let database = Database::with_tenant().await;
    let (_, build, _) = database.fixture().await;
    let certificates = Certificates::new();
    let replica = Replica::start(database.state.clone(), &certificates).await;
    let (mut client, run, _) = planner(&database, &replica, &certificates, build).await;
    sql::execute(&mut database.pool.clone(),"INSERT INTO nodes(id,name,os,arch,pool_id,certificate_hash,certificate_expires_at,metadata) SELECT gen_random_uuid(),'node-'||s,'linux','amd64','00000000-0000-0000-0000-000000000001',s::text,now()+interval '1 day','{\"custom.site\":\"lab\"}' FROM generate_series(1,120) s",params![]).await.unwrap();
    let page = node_tool(
        &mut client,
        run,
        "query_nodes",
        json!({"metadata":{"custom.site":"lab"}}),
    )
    .await;
    assert_eq!(page["nodes"].as_array().unwrap().len(), 50, "{page}");
    assert!(page["nodes"][0]["resources"].is_null());
    assert_eq!(page["nodes"][0]["resources_fresh"], false);
    assert!(page["nodes"][0].get("metadata").is_none());
    let next = node_tool(
        &mut client,
        run,
        "query_nodes",
        json!({"cursor":page["next_cursor"],"limit":100}),
    )
    .await;
    assert_eq!(next["nodes"].as_array().unwrap().len(), 70, "{next}");
    let id = page["nodes"][0]["node_id"].clone();
    let detail = node_tool(&mut client, run, "get_node", json!({"node_id":id})).await;
    assert_eq!(detail["nodes"][0]["metadata"]["custom.site"], "lab");
    assert!(
        node_tool(&mut client, run, "query_nodes", json!({"limit":101}))
            .await
            .get("error")
            .is_some()
    );
    sql::execute(&mut database.pool.clone(), "UPDATE nodes SET startup_id=gen_random_uuid(),metadata=metadata || '{\"tools.go\":{\"available\":true,\"version\":\"1.25\"}}',diagnostics='{\"tools\":{\"go\":{\"version\":\"obsolete\"}}}',signing_profiles='{private-profile}' WHERE id=$1",params![serde_json::from_value::<Uuid>(id.clone()).unwrap()]).await.unwrap();
    let tools = node_tool(&mut client, run, "query_nodes", json!({"tools":["go"]})).await;
    assert_eq!(tools["nodes"].as_array().unwrap().len(), 1);
    let summary = node_tool(&mut client, run, "summarize_nodes", json!({})).await;
    let group = summary["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["tools"]["go"]["version"] == "1.25")
        .unwrap();
    assert_eq!(group["signing_profiles"], json!([]));
    assert_eq!(group["task_slots"], 1);
    // Oversized pages end at a stable cursor and keep every node reachable.
    sql::execute(
        &mut database.pool.clone(),
        "UPDATE nodes SET labels=jsonb_build_object('large',repeat('x',4096))",
        params![],
    )
    .await
    .unwrap();
    let mut cursor = Value::Null;
    let mut seen = std::collections::BTreeSet::new();
    loop {
        let page = node_tool(
            &mut client,
            run,
            "query_nodes",
            json!({"limit":100,"cursor":cursor}),
        )
        .await;
        assert!(serde_json::to_vec(&page).unwrap().len() <= 128 * 1024);
        for node in page["nodes"].as_array().unwrap() {
            assert!(seen.insert(node["node_id"].as_str().unwrap().to_owned()));
        }
        cursor = page["next_cursor"].clone();
        if cursor.is_null() {
            break;
        }
        assert!(seen.len() < 120);
    }
    assert_eq!(seen.len(), 120);
    sql::execute(
        &mut database.pool.clone(),
        "UPDATE projects SET pool_ids='{}' WHERE id=(SELECT project_id FROM builds WHERE id=$1)",
        params![build],
    )
    .await
    .unwrap();
    assert_eq!(
        node_tool(&mut client, run, "get_node", json!({"node_id":id})).await["nodes"],
        json!([]),
        "current project permission overrides frozen access"
    );
    assert_eq!(
        node_tool(&mut client, run, "summarize_nodes", json!({})).await["nodes"],
        json!([])
    );
    replica.stop().await;
    database.close().await;
}

async fn resource_server(
    database: &mut Database,
    certificates: &Certificates,
    values: Arc<RwLock<Vec<Value>>>,
) -> (tempfile::TempDir, tokio::task::JoinHandle<()>) {
    resource_server_notifying(database, certificates, values, None).await
}

async fn resource_server_notifying(
    database: &mut Database,
    certificates: &Certificates,
    values: Arc<RwLock<Vec<Value>>>,
    requested: Option<Arc<tokio::sync::Notify>>,
) -> (tempfile::TempDir, tokio::task::JoinHandle<()>) {
    let directory = tempfile::tempdir().unwrap();
    tokio::fs::write(directory.path().join("ca.pem"), &certificates.ca)
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = axum::Router::new().route(
        "/v1/resources",
        axum::routing::post(move |axum::Json(identities): axum::Json<Vec<Value>>| {
            let values = values.clone();
            let requested = requested.clone();
            async move {
                if let Some(requested) = requested {
                    requested.notify_one();
                }
                axum::Json(
                    values
                        .read()
                        .await
                        .iter()
                        .filter(|value| {
                            identities.iter().any(|id| {
                                id["node_id"] == value["node_id"]
                                    && id["startup_id"] == value["startup_id"]
                            })
                        })
                        .cloned()
                        .collect::<Vec<_>>(),
                )
            }
        }),
    );
    let settings = Arc::make_mut(&mut database.state.settings);
    settings.pki_dir = directory.path().into();
    settings.telemetry = Some(artifactory_control_plane::settings::TelemetrySettings {
        endpoint: String::new(),
        query_url: format!("http://{address}"),
        token: "test".repeat(8),
    });
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (directory, task)
}
async fn placement_node(database: &Database) -> (Uuid, Uuid, Value) {
    let node = database.node().await;
    let startup = Uuid::new_v4();
    sql::execute(&mut database.pool.clone(),"UPDATE nodes SET startup_id=$2,metadata_digest=$3,capabilities='[\"git\",\"bash\",\"source-cache-v1\",\"external-build-plan-v1\"]' WHERE id=$1",params![node,startup,"a".repeat(64)]).await.unwrap();
    (
        node,
        startup,
        json!({"node_id":node,"startup_id":startup,"sampled_at":chrono::Utc::now(),"received_at":chrono::Utc::now(),"available_memory_bytes":8192,"build_disk_available_bytes":16384,"cpu_percent":20}),
    )
}
async fn rank(database: &Database, build: Uuid, job: Uuid, ids: &[Uuid]) {
    let placement = json!({"candidates":ids.iter().map(|id|json!({"node_id":id,"metadata_digest":"a".repeat(64),"reason":"native platform and available memory"})).collect::<Vec<_>>(),"resources":{"min_available_memory_bytes":4096,"min_build_disk_available_bytes":8192,"max_cpu_percent":80}});
    sql::execute(&mut database.pool.clone(),"UPDATE builds SET goal='test',planning_context=jsonb_build_object('placement_version',1,'pool_ids',jsonb_build_array('00000000-0000-0000-0000-000000000001'),'signing_profiles','[]'::jsonb),model_config_snapshot='{\"max_calls\":20,\"timeout_seconds\":600}' WHERE id=$1",params![build]).await.unwrap();
    sql::execute(
        &mut database.pool.clone(),
        "INSERT INTO job_placements(job_id,placement) VALUES($1,$2)",
        params![job, placement],
    )
    .await
    .unwrap();
}

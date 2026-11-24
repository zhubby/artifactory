//! [INPUT]: 依赖真实 PostgreSQL、三个副本、设备证书与浏览器 WebSocket。
//! [OUTPUT]: 验证慢快照授权截止、丢 ACK 与内部断流恢复和退出尾部。
//! [POS]: 同一设备终端在独立入口会合的端到端验收，不用共享内存代替内部流。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use futures::{SinkExt, StreamExt};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};

#[path = "terminal_recovery.rs"]
mod recovery;
use recovery::{close_code, reconnect};

async fn read_frame(
    stream: &mut tonic::Streaming<pb::TerminalFrame>,
    kind: &str,
) -> pb::TerminalFrame {
    tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            let frame = stream
                .message()
                .await
                .unwrap()
                .expect("terminal stream closed before expected message");
            if frame.kind == kind {
                return frame;
            }
        }
    })
    .await
    .expect("terminal message deadline")
}

fn fresh_replica(state: &AppState) -> AppState {
    AppState {
        coordination: Default::default(),
        connections: Arc::new(RwLock::new(HashMap::new())),
        ..state.clone()
    }
}

#[derive(Clone, Copy)]
enum End {
    SessionRevoked,
    OwnerStopped,
    DatabaseBlocked,
    ReplayExit,
    SlowReaderRevoked,
    ReconnectInput,
    LostAck,
    RelayInterrupted,
    SnapshotRevoked,
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn browser_and_device_streams_join_owner_across_three_tls_replicas() {
    exercise(End::SessionRevoked, false).await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn terminal_owner_stop_closes_the_remote_device_stream() {
    exercise(End::OwnerStopped, false).await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn terminal_authorization_query_hang_cannot_extend_five_second_grant() {
    exercise(End::DatabaseBlocked, false).await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn terminal_reconnect_replays_a_thousand_frames_before_real_exit() {
    exercise(End::ReplayExit, false).await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn terminal_direct_slow_reader_cannot_extend_revoked_authority() {
    exercise(End::SlowReaderRevoked, true).await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn terminal_three_replica_slow_reader_cannot_extend_revoked_authority() {
    exercise(End::SlowReaderRevoked, false).await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn terminal_reconnect_discards_lost_input_without_rejecting_later_sequence() {
    exercise(End::ReconnectInput, false).await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn terminal_reconnect_recovers_consumed_output_when_final_ack_was_lost() {
    exercise(End::LostAck, false).await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn terminal_browser_relay_interruption_requests_same_renderer_recovery() {
    exercise(End::RelayInterrupted, false).await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn terminal_delayed_old_authorization_snapshot_cannot_renew_revoked_grant() {
    exercise(End::SnapshotRevoked, false).await;
}

async fn exercise(ending: End, direct: bool) {
    let database = Database::with_tenant().await;
    let owner_state = database.state.clone();
    let browser_state = if direct {
        owner_state.clone()
    } else {
        fresh_replica(&owner_state)
    };
    let device_state = if direct {
        owner_state.clone()
    } else {
        fresh_replica(&owner_state)
    };
    for state in [&owner_state, &browser_state, &device_state] {
        artifactory_control_plane::terminals::start(state.clone())
            .await
            .unwrap();
    }
    let instance = owner_state
        .store
        .terminal_instance(owner_state.coordination.id())
        .await
        .unwrap();
    let relay_channel = Channel::from_shared(instance.endpoint)
        .unwrap()
        .tls_config(
            ClientTlsConfig::new()
                .ca_certificate(Certificate::from_pem(instance.certificate))
                .domain_name("terminal.internal"),
        )
        .unwrap()
        .connect()
        .await
        .unwrap();
    let mut untrusted = pb::terminal_relay_client::TerminalRelayClient::new(relay_channel);
    let rejected = untrusted
        .connect(tokio_stream::iter(Vec::<pb::TerminalFrame>::new()))
        .await;
    assert!(
        rejected.is_err_and(|error| error.code() == tonic::Code::Unauthenticated),
        "internal TLS alone must not authorize relay access"
    );
    let certificates = Certificates::new();
    let owner = Replica::start(owner_state.clone(), &certificates).await;
    let entry = Replica::start(device_state.clone(), &certificates).await;
    let mut device = Device::new(&database.pool, &owner, &certificates).await;
    let startup = Uuid::new_v4();
    sql::execute(&mut database.pool.clone(),"UPDATE nodes SET startup_id=$2,capabilities='[\"terminal-v1\"]',metadata=$3 WHERE id=$1",params![device.id,startup,json!({"emissary.terminal":{"enabled":true,"profiles":[{"id":"default","label":"Shell"}],"account":"fixture"}})]).await.unwrap();
    let user: Uuid = sql::scalar(
        &mut database.pool.clone(),
        "SELECT id FROM users WHERE username='fixture-owner'",
        params![],
    )
    .await
    .unwrap();
    let cookie = "artifactory_session=terminal-three-replica-login";
    let credential = crypto::hash("terminal-three-replica-login");
    sql::execute(
        &mut database.pool.clone(),
        "INSERT INTO sessions(token_hash,user_id,expires_at) VALUES($1,$2,now()+interval '1 hour')",
        params![&credential, user],
    )
    .await
    .unwrap();
    let mut auth = artifactory_control_plane::auth::Auth::scoped(user, "admin", vec![], vec![]);
    auth.token_projects = None;
    auth.role = "owner".into();
    auth.tenant_id = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
    // 通过真实 HTTP 提取器取得原 Session 身份，测试不能伪造 credential_hash。
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let http_address = listener.local_addr().unwrap();
    let http_state = browser_state.clone();
    let http = tokio::spawn(async move {
        axum::serve(listener, api::router(http_state))
            .await
            .unwrap()
    });
    let client = reqwest::Client::new();
    let renderer = Uuid::new_v4();
    let created:Value=client.post(format!("http://{http_address}/api/v1/nodes/{}/terminal-sessions",device.id))
        .header("cookie",cookie).header("x-artifactory-tenant",auth.tenant_id.to_string()).header("x-artifactory-csrf","1")
        .json(&json!({"idempotency_key":Uuid::new_v4(),"renderer_id":renderer,"profile":"default","columns":80,"rows":24,"term":"xterm-256color"}))
        .send().await.unwrap().error_for_status().unwrap().json().await.unwrap();
    let id = Uuid::parse_str(created["id"].as_str().unwrap()).unwrap();
    assert_eq!(created["owner"], owner_state.coordination.id().to_string());
    let attach: Value = client
        .post(format!(
            "http://{http_address}/api/v1/terminal-sessions/{id}/attach"
        ))
        .header("cookie", cookie)
        .header("x-artifactory-tenant", auth.tenant_id.to_string())
        .header("x-artifactory-csrf", "1")
        .json(&json!({"renderer_id":renderer,"offset":0}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let mut ws_request = format!("ws://{http_address}/api/v1/terminal-sessions/{id}/stream")
        .into_client_request()
        .unwrap();
    ws_request
        .headers_mut()
        .insert("cookie", cookie.parse().unwrap());
    ws_request
        .headers_mut()
        .insert("origin", "http://localhost:3000".parse().unwrap());
    let (mut browser, _) = connect_async(ws_request).await.unwrap();
    browser.send(Message::Text(json!({"type":"authenticate","ticket":attach["ticket"],"renderer_id":renderer,"offset":0}).to_string().into())).await.unwrap();
    let command = device.next("terminal_open").await;
    let replay = device.next("terminal_open").await;
    assert_eq!(command.command_id, replay.command_id);
    assert_eq!(
        command.payload_json, replay.payload_json,
        "control replay must not extend the Open deadline"
    );
    let open: pb::terminal::OpenTerminal = serde_json::from_str(&command.payload_json).unwrap();
    assert_eq!(open.terminal_id, id);
    let channel = Channel::from_shared(entry.url.clone())
        .unwrap()
        .tls_config(
            ClientTlsConfig::new()
                .ca_certificate(Certificate::from_pem(&certificates.ca))
                .identity(Identity::from_pem(&device.certificate, &device.key)),
        )
        .unwrap()
        .connect()
        .await
        .unwrap();
    let mut terminal = pb::terminal_client::TerminalClient::new(channel);
    let (tx, rx) = mpsc::channel(16);
    tx.send(pb::TerminalFrame {
        terminal_id: id.to_string(),
        kind: "hello".into(),
        payload_json: serde_json::to_string(&pb::terminal::TerminalHello {
            node_id: device.id,
            node_session: open.node_session,
            startup_id: startup,
        })
        .unwrap(),
        ..Default::default()
    })
    .await
    .unwrap();
    let mut stream = terminal
        .connect(ReceiverStream::new(rx))
        .await
        .unwrap()
        .into_inner();
    let start = read_frame(&mut stream, "start").await;
    assert_eq!((start.columns, start.rows), (80, 24));
    tx.send(pb::TerminalFrame {
        terminal_id: id.to_string(),
        kind: "ready".into(),
        payload_json: json!({"account":"fixture","cwd":"/tmp","shell":"/bin/sh"}).to_string(),
        ..Default::default()
    })
    .await
    .unwrap();
    let output = vec![0, 255, 195, 169, 10];
    tx.send(pb::TerminalFrame {
        terminal_id: id.to_string(),
        kind: "output".into(),
        offset: 0,
        data: output.clone(),
        ..Default::default()
    })
    .await
    .unwrap();
    let received = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let item = browser.next().await.unwrap().unwrap();
            let Message::Text(text) = item else { continue };
            let value: Value = serde_json::from_str(&text).unwrap();
            if value["type"] == "output" {
                break value;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(
        STANDARD.decode(received["data"].as_str().unwrap()).unwrap(),
        output
    );
    browser
        .send(Message::Text(
            json!({"type":"ack","offset":output.len()})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    assert_eq!(
        read_frame(&mut stream, "ack").await.offset,
        output.len() as u64
    );
    browser
        .send(Message::Text(
            json!({"type":"input","sequence":1,"data":STANDARD.encode(b"pwd\n")})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    assert_eq!(read_frame(&mut stream, "input").await.data, b"pwd\n");
    if matches!(ending, End::LostAck) {
        let end = output.len() + 1_048_576;
        for index in 0..64 {
            tx.send(pb::TerminalFrame {
                terminal_id: id.to_string(),
                kind: "output".into(),
                offset: (output.len() + index * 16_384) as u64,
                data: vec![41; 16_384],
                ..Default::default()
            })
            .await
            .unwrap();
        }
        let mut rendered = output.len();
        tokio::time::timeout(Duration::from_secs(6), async {
            while rendered < end {
                if let Message::Text(text) = browser.next().await.unwrap().unwrap() {
                    let value: Value = serde_json::from_str(&text).unwrap();
                    if value["type"] == "output" {
                        assert_eq!(value["offset"], rendered);
                        rendered += STANDARD
                            .decode(value["data"].as_str().unwrap())
                            .unwrap()
                            .len();
                    }
                }
            }
        })
        .await
        .unwrap();
        // 输出已渲染，但断线丢失全部消费确认。附着偏移就是原渲染器的确认。
        browser.close(None).await.unwrap();
        drop(browser);
        browser = reconnect(
            &client,
            http_address,
            id,
            cookie,
            auth.tenant_id,
            renderer,
            end,
        )
        .await;
        assert_eq!(read_frame(&mut stream, "ack").await.offset, end as u64);
        tx.send(pb::TerminalFrame {
            terminal_id: id.to_string(),
            kind: "output".into(),
            offset: end as u64,
            data: b"credit resumed".to_vec(),
            ..Default::default()
        })
        .await
        .unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let Message::Text(text) = browser.next().await.unwrap().unwrap() else {
                    continue;
                };
                let value: Value = serde_json::from_str(&text).unwrap();
                if value["type"] == "output" {
                    assert_eq!(
                        STANDARD.decode(value["data"].as_str().unwrap()).unwrap(),
                        b"credit resumed"
                    );
                    break;
                }
            }
        })
        .await
        .unwrap();
    }
    if matches!(ending, End::RelayInterrupted) {
        browser.close(None).await.unwrap();
        drop(browser);
        let instance = owner_state
            .store
            .terminal_instance(owner_state.coordination.id())
            .await
            .unwrap();
        let target = url::Url::parse(&instance.endpoint).unwrap();
        let address = format!("{}:{}", target.host_str().unwrap(), target.port().unwrap());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy = format!("https://{}", listener.local_addr().unwrap());
        sql::execute(
            &mut database.pool.clone(),
            "UPDATE terminal_instances SET endpoint=$2 WHERE id=$1",
            params![owner_state.coordination.id(), proxy],
        )
        .await
        .unwrap();
        let proxy = tokio::spawn(async move {
            let (mut incoming, _) = listener.accept().await.unwrap();
            let mut outgoing = tokio::net::TcpStream::connect(address).await.unwrap();
            let _ = tokio::io::copy_bidirectional(&mut incoming, &mut outgoing).await;
        });
        browser = reconnect(
            &client,
            http_address,
            id,
            cookie,
            auth.tenant_id,
            renderer,
            output.len(),
        )
        .await;
        // 先证明新流经过代理，再只关闭这条浏览器内部连接。
        browser.send(Message::Text(json!({"type":"input","sequence":2,"data":STANDARD.encode(b"before interruption\n")}).to_string().into())).await.unwrap();
        assert_eq!(read_frame(&mut stream, "input").await.offset, 2);
        proxy.abort();
        let _ = proxy.await;
        assert_eq!(close_code(&mut browser).await, 1012);
        drop(browser);
        // 内部入口尚未恢复时，首帧建流也必须允许后续重试。
        browser = reconnect(
            &client,
            http_address,
            id,
            cookie,
            auth.tenant_id,
            renderer,
            output.len(),
        )
        .await;
        assert_eq!(close_code(&mut browser).await, 1012);
        drop(browser);
        sql::execute(
            &mut database.pool.clone(),
            "UPDATE terminal_instances SET endpoint=$2 WHERE id=$1",
            params![owner_state.coordination.id(), instance.endpoint],
        )
        .await
        .unwrap();
        browser = reconnect(
            &client,
            http_address,
            id,
            cookie,
            auth.tenant_id,
            renderer,
            output.len(),
        )
        .await;
        browser.send(Message::Text(json!({"type":"input","sequence":3,"data":STANDARD.encode(b"after interruption\n")}).to_string().into())).await.unwrap();
        let input = read_frame(&mut stream, "input").await;
        assert_eq!(input.offset, 3);
        assert_eq!(input.data, b"after interruption\n");
    }
    if matches!(ending, End::ReconnectInput) {
        browser.close(None).await.unwrap();
        drop(browser);
        tokio::time::sleep(Duration::from_millis(100)).await;
        browser = reconnect(
            &client,
            http_address,
            id,
            cookie,
            auth.tenant_id,
            renderer,
            output.len(),
        )
        .await;
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let Message::Text(text) = browser.next().await.unwrap().unwrap() else {
                    continue;
                };
                let status: Value = serde_json::from_str(&text).unwrap();
                if status["state"] == "active" {
                    break;
                }
            }
        })
        .await
        .unwrap();
        // 第二条输入在短断时丢失。页面不重放它，第三条仍保持可用。
        browser
            .send(Message::Text(
                json!({"type":"input","sequence":3,"data":STANDARD.encode(b"whoami\n")})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        let input = read_frame(&mut stream, "input").await;
        assert_eq!(input.offset, 3);
        assert_eq!(input.data, b"whoami\n");
    }
    if matches!(ending, End::ReplayExit) {
        browser.close(None).await.unwrap();
        drop(browser);
        tokio::time::sleep(Duration::from_millis(200)).await;
        let mut expected = Vec::new();
        for index in 0..1200 {
            let data = vec![(index % 251) as u8; 512];
            tx.send(pb::TerminalFrame {
                terminal_id: id.to_string(),
                kind: "output".into(),
                offset: (output.len() + expected.len()) as u64,
                data: data.clone(),
                ..Default::default()
            })
            .await
            .unwrap();
            expected.extend_from_slice(&data);
        }
        tx.send(pb::TerminalFrame {
            terminal_id: id.to_string(),
            kind: "exit".into(),
            payload_json: json!({"code":7,"signal":null,"reason":"shell_exited"}).to_string(),
            ..Default::default()
        })
        .await
        .unwrap();
        let mut resumed = reconnect(
            &client,
            http_address,
            id,
            cookie,
            auth.tenant_id,
            renderer,
            output.len(),
        )
        .await;
        let mut actual = Vec::new();
        tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                let Message::Text(text) = resumed.next().await.unwrap().unwrap() else {
                    continue;
                };
                let value: Value = serde_json::from_str(&text).unwrap();
                if value["type"] == "output" {
                    assert_eq!(
                        value["offset"].as_u64().unwrap(),
                        (output.len() + actual.len()) as u64
                    );
                    actual.extend_from_slice(
                        &STANDARD.decode(value["data"].as_str().unwrap()).unwrap(),
                    );
                    let _ = resumed
                        .send(Message::Text(
                            json!({"type":"ack","offset":output.len()+actual.len()})
                                .to_string()
                                .into(),
                        ))
                        .await;
                    tokio::time::sleep(Duration::from_millis(15)).await;
                } else if value["type"] == "exit" {
                    assert_eq!(value["code"], 7);
                    assert_eq!(value["reason"], "shell_exited");
                    break;
                }
            }
        })
        .await
        .expect("all buffered output and real exit must drain");
        assert_eq!(actual, expected);
        tokio::time::timeout(Duration::from_secs(2),async{
            loop{
                let details:Option<Value>=sql::optional_scalar(&mut database.pool.clone(),"SELECT details FROM audit_events WHERE action='terminal_closed' AND resource_id=$1",params![id.to_string()]).await.unwrap();
                if let Some(details)=details && details["output_bytes"]==(output.len()+expected.len()) {
                    assert_eq!(details["account"],"fixture");assert_eq!(details["input_bytes"],4);break;
                }
                tokio::task::yield_now().await;
            }
        }).await.unwrap();
        browser_state.coordination.stop();
        device_state.coordination.stop();
        http.abort();
        let _ = http.await;
        owner.stop().await;
        entry.stop().await;
        database.close().await;
        return;
    }
    if matches!(ending, End::SlowReaderRevoked) {
        for index in 0..63 {
            tx.send(pb::TerminalFrame {
                terminal_id: id.to_string(),
                kind: "output".into(),
                offset: output.len() as u64 + index * 16_384,
                data: vec![42; 16_384],
                ..Default::default()
            })
            .await
            .unwrap();
        }
        // 浏览器保持不读，积压覆盖 broker、内部流和 WebSocket 出口。
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    if matches!(ending, End::SnapshotRevoked) {
        sql::execute(
            &mut database.pool.clone(),
            "ALTER TABLE sessions RENAME TO sessions_snapshot_source",
            params![],
        )
        .await
        .unwrap();
        sql::execute(&mut database.pool.clone(),"CREATE VIEW sessions AS WITH delayed AS MATERIALIZED (SELECT s.* FROM sessions_snapshot_source s CROSS JOIN LATERAL (SELECT pg_sleep(2.8+length(s.token_hash)*0.0)) delay) SELECT * FROM delayed",params![]).await.unwrap();
        // 五处复核先取得旧快照，再使登录过期。
        tokio::time::timeout(Duration::from_secs(4),async {
            loop {
                let sleeping:i64=sql::scalar(&mut database.admin.clone(),"SELECT count(*) FROM pg_stat_activity a WHERE wait_event='PgSleep' AND query LIKE '%terminal_sessions%' AND EXISTS(SELECT 1 FROM pg_locks l WHERE l.pid=a.pid AND l.relation=to_regclass($1))",params![format!("{}.sessions_snapshot_source",database.schema)]).await.unwrap();
                if sleeping>=5 {break}
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.expect("all five authorization queries must hold the old snapshot");
        sql::execute(&mut database.pool.clone(),"UPDATE sessions_snapshot_source SET expires_at=clock_timestamp()-interval '1 second' WHERE token_hash=$1",params![&credential]).await.unwrap();
    }
    let mut blocking_pool = database.pool.clone();
    let mut blocked = match ending {
        End::SessionRevoked
        | End::SlowReaderRevoked
        | End::ReconnectInput
        | End::LostAck
        | End::RelayInterrupted => {
            sql::execute(
                &mut database.pool.clone(),
                "DELETE FROM sessions WHERE token_hash=$1",
                params![credential],
            )
            .await
            .unwrap();
            None
        }
        End::OwnerStopped => {
            owner_state.coordination.stop();
            None
        }
        End::DatabaseBlocked => {
            let mut tx = blocking_pool.transaction().await.unwrap();
            sql::execute(
                &mut tx,
                "LOCK TABLE sessions IN ACCESS EXCLUSIVE MODE",
                params![],
            )
            .await
            .unwrap();
            Some(tx)
        }
        End::ReplayExit => unreachable!(),
        End::SnapshotRevoked => None,
    };
    let deadline_started = Instant::now();
    tokio::time::timeout(Duration::from_millis(5250), async {
        loop {
            match stream.message().await {
                Ok(Some(frame)) if frame.kind == "close" => break,
                Ok(Some(_)) => {}
                Err(error) => {
                    assert!(matches!(
                        error.code(),
                        tonic::Code::PermissionDenied | tonic::Code::Unavailable
                    ));
                    break;
                }
                Ok(None) => break,
            }
        }
    })
    .await
    .expect("device authority must expire outside the output queue");
    assert!(
        deadline_started.elapsed() < Duration::from_millis(5250),
        "forwarding authority must not wait for the blocked database query"
    );
    if matches!(ending, End::SlowReaderRevoked) {
        tokio::time::sleep_until(
            tokio::time::Instant::from_std(deadline_started) + Duration::from_millis(5200),
        )
        .await;
        tokio::time::timeout(Duration::from_secs(1), async {
            while let Some(message) = browser.next().await {
                match message {
                    Ok(Message::Close(Some(close))) => {
                        assert_eq!(u16::from(close.code), 1008);
                        break;
                    }
                    Ok(Message::Close(None)) | Err(_) => break,
                    Ok(_) => {}
                }
            }
        })
        .await
        .expect("slow browser must already be closed after the hard deadline");
    }
    if let Some(tx) = blocked.take() {
        tx.rollback().await.unwrap();
    }
    let stored = database.state.store.terminal_session(id).await.unwrap();
    let persisted_deadline = if matches!(ending, End::SnapshotRevoked) {
        tokio::time::Instant::from_std(deadline_started) + Duration::from_secs(20)
    } else {
        tokio::time::Instant::now() + Duration::from_secs(2)
    };
    tokio::time::timeout_at(persisted_deadline, async {
        loop {
            if database
                .state
                .store
                .terminal_session(id)
                .await
                .unwrap()
                .state
                == "closed"
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("terminal closure metadata must persist after forwarding stops");
    assert_eq!(stored.node_id, device.id);
    browser_state.coordination.stop();
    device_state.coordination.stop();
    http.abort();
    let _ = http.await;
    owner.stop().await;
    entry.stop().await;
    database.close().await;
}

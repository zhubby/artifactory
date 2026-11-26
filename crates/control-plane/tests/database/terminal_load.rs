//! [INPUT]: 依赖真实 PostgreSQL、三份 Control 状态、TLS 设备流和浏览器 WebSocket。
//! [OUTPUT]: 量化十会话输出、credit、RTT与资源；心跳超限打印实际采样。
//! [POS]: 虚拟设备协议负载；不启动 PTY，不代表实机或独立进程副本负载。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use futures::{SinkExt, StreamExt};
use std::{collections::VecDeque, sync::Mutex};
use tokio::sync::{Barrier, oneshot};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};
use tokio_util::sync::CancellationToken;

const BULK: usize = 2 * 1024 * 1024;
const WINDOW: u64 = pb::terminal::MAX_BUFFER_BYTES as u64;
type Browser = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

struct Session {
    id: Uuid,
    cookie: String,
    browser: Browser,
    send: mpsc::Sender<pb::TerminalFrame>,
    receive: tonic::Streaming<pb::TerminalFrame>,
    device: Device,
    slow: bool,
}

fn replica(state: &AppState) -> AppState {
    AppState {
        coordination: Default::default(),
        connections: Arc::new(RwLock::new(HashMap::new())),
        ..state.clone()
    }
}

async fn prepare(
    db: &Database,
    owner: &Replica,
    entry: &Replica,
    certificates: &Certificates,
    address: std::net::SocketAddr,
    index: usize,
) -> Session {
    let user = Uuid::new_v4();
    let raw = Uuid::new_v4().to_string();
    let cookie = format!("artifactory_session={raw}");
    sql::execute(
        &mut db.pool.clone(),
        "INSERT INTO users(id,username,password_hash,role) VALUES($1,$2,'unused','admin')",
        params![user, format!("terminal-load-{index}")],
    )
    .await
    .unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "INSERT INTO sessions(token_hash,user_id,expires_at) VALUES($1,$2,now()+interval '1 hour')",
        params![crypto::hash(&raw), user],
    )
    .await
    .unwrap();
    let mut device = Device::new(&db.pool, owner, certificates).await;
    let startup = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(),"UPDATE nodes SET startup_id=$2,capabilities='[\"terminal-v1\"]',metadata=$3 WHERE id=$1",params![device.id,startup,json!({"emissary.terminal":{"enabled":true,"profiles":[{"id":"default","label":"Shell"}],"account":"load-fixture"}})]).await.unwrap();
    let client = reqwest::Client::new();
    let renderer = Uuid::new_v4();
    let created:Value=client.post(format!("http://{address}/api/v1/nodes/{}/terminal-sessions",device.id))
        .header("cookie",&cookie).header("x-artifactory-tenant","00000000-0000-0000-0000-000000000001").header("x-artifactory-csrf","1")
        .json(&json!({"idempotency_key":Uuid::new_v4(),"renderer_id":renderer,"profile":"default","columns":80,"rows":24,"term":"xterm-256color"}))
        .send().await.unwrap().error_for_status().unwrap().json().await.unwrap();
    let id = Uuid::parse_str(created["id"].as_str().unwrap()).unwrap();
    let ticket: Value = client
        .post(format!(
            "http://{address}/api/v1/terminal-sessions/{id}/attach"
        ))
        .header("cookie", &cookie)
        .header(
            "x-artifactory-tenant",
            "00000000-0000-0000-0000-000000000001",
        )
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
    let mut request = format!("ws://{address}/api/v1/terminal-sessions/{id}/stream")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("cookie", cookie.parse().unwrap());
    request
        .headers_mut()
        .insert("origin", "http://localhost:3000".parse().unwrap());
    let (mut browser, _) = connect_async(request).await.unwrap();
    browser.send(Message::Text(json!({"type":"authenticate","ticket":ticket["ticket"],"renderer_id":renderer,"offset":0}).to_string().into())).await.unwrap();
    let open: pb::terminal::OpenTerminal =
        serde_json::from_str(&device.next("terminal_open").await.payload_json).unwrap();
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
    let (send, rx) = mpsc::channel(32);
    send.send(pb::TerminalFrame {
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
    let mut receive = pb::terminal_client::TerminalClient::new(channel)
        .connect(ReceiverStream::new(rx))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(receive.message().await.unwrap().unwrap().kind, "start");
    Session {
        id,
        cookie,
        browser,
        send,
        receive,
        device,
        slow: index >= 5,
    }
}

async fn control(
    mut device: Device,
    stop: CancellationToken,
    closed: oneshot::Sender<Instant>,
) -> Vec<f64> {
    let mut samples = Vec::new();
    let mut pending = HashMap::new();
    let mut closed = Some(closed);
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    loop {
        tokio::select! {
            _=stop.cancelled()=>break,
            _=tick.tick()=>{
                let request=Uuid::new_v4().to_string();pending.insert(request.clone(),Instant::now());
                assert!(pending.len()<64,"terminal load stalled the Control stream");
                device.events.send(pb::EmissaryEvent{kind:"heartbeat".into(),emissary_id:device.id.to_string(),command_id:request,payload_json:json!({"capabilities":["git","bash","terminal-v1"],"diagnostics":{"emissary_version":"terminal-load"},"ota":{},"active_attempts":[]}).to_string(),..Default::default()}).await.unwrap();
            },
            message=device.commands.message()=>{
                let command=message.unwrap().expect("Control stream must remain healthy under terminal load");
                if command.kind=="emissary_health" && let Some(start)=pending.remove(&command.request_id){samples.push(start.elapsed().as_secs_f64()*1000.0);}
                if command.kind=="terminal_close" && let Some(done)=closed.take(){let _=done.send(Instant::now());}
            }
        }
    }
    samples
}

async fn virtual_device(
    id: Uuid,
    send: mpsc::Sender<pb::TerminalFrame>,
    mut receive: tonic::Streaming<pb::TerminalFrame>,
) -> (u64, bool, Instant) {
    send.send(pb::TerminalFrame {
        terminal_id: id.to_string(),
        kind: "ready".into(),
        payload_json: json!({"account":"load-fixture","cwd":"/tmp","shell":"virtual"}).to_string(),
        ..Default::default()
    })
    .await
    .unwrap();
    let mut produced = 0u64;
    let mut consumed = 0u64;
    let mut bulk = 0usize;
    let mut echoes = VecDeque::<Vec<u8>>::new();
    let mut started = false;
    let mut peak = 0;
    let mut stalled = false;
    loop {
        let data = echoes
            .front()
            .cloned()
            .or_else(|| (started && bulk < BULK).then(|| vec![42; 16 * 1024]));
        let writable = data
            .as_ref()
            .is_some_and(|data| produced - consumed + data.len() as u64 <= WINDOW);
        if data.is_some() && !writable {
            stalled = true;
        }
        tokio::select! {biased;
            value=receive.message()=>{
                let incoming=match value{
                    Ok(Some(frame))=>frame,
                    Ok(None)=>return(peak,stalled,Instant::now()),
                    Err(error)=>{assert!(matches!(error.code(),tonic::Code::PermissionDenied|tonic::Code::Unavailable));return(peak,stalled,Instant::now())},
                };
                match incoming.kind.as_str(){
                    "input"=>{started=true;echoes.push_back(incoming.data);},
                    "ack"=>{assert!(incoming.offset>=consumed&&incoming.offset<=produced);consumed=incoming.offset;},
                    "close"=>return(peak,stalled,Instant::now()),
                    "lease"=>{},
                    other=>panic!("unexpected virtual device frame: {other}"),
                }
            },
            permit=send.reserve(),if writable=>{
                let data=data.unwrap();
                if echoes.pop_front().is_none(){bulk+=data.len();}
                let offset=produced;produced+=data.len()as u64;peak=peak.max(produced-consumed);
                assert!(peak<=WINDOW,"device must obey cumulative consumption credit");
                permit.unwrap().send(pb::TerminalFrame{terminal_id:id.to_string(),kind:"output".into(),offset,data,..Default::default()});
            }
        }
    }
}

async fn ping(browser: &mut Browser, sequence: u64, sent: &mut HashMap<u64, Instant>) {
    sent.insert(sequence, Instant::now());
    browser.send(Message::Text(json!({"type":"input","sequence":sequence,"data":STANDARD.encode(format!("RTT:{sequence}\n"))}).to_string().into())).await.unwrap();
}

async fn run(mut session: Session, address: std::net::SocketAddr, barrier: Arc<Barrier>) -> Value {
    let stop = CancellationToken::new();
    let (control_closed, closed) = oneshot::channel();
    let heartbeat = tokio::spawn(control(session.device, stop.clone(), control_closed));
    let device = tokio::spawn(virtual_device(session.id, session.send, session.receive));
    barrier.wait().await;
    let started = Instant::now();
    let mut produced = 0u64;
    let mut bulk = 0usize;
    let mut echo = Vec::new();
    let mut sent = HashMap::new();
    let mut rtt = Vec::new();
    let mut sequence = 0;
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let text = match session
                .browser
                .next()
                .await
                .expect("load socket ended")
                .expect("load socket error")
            {
                Message::Text(text) => text,
                Message::Close(close) => {
                    panic!("load socket closed at {bulk} bulk bytes: {close:?}")
                }
                _ => continue,
            };
            let value: Value = serde_json::from_str(&text).unwrap();
            if value["state"] == "active" && sequence == 0 {
                sequence = 1;
                ping(&mut session.browser, sequence, &mut sent).await;
            }
            if value["type"] != "output" {
                assert_ne!(value["type"], "exit", "{value}");
                continue;
            }
            assert_eq!(value["offset"].as_u64().unwrap(), produced);
            let data = STANDARD.decode(value["data"].as_str().unwrap()).unwrap();
            produced += data.len() as u64;
            for byte in data {
                if byte == 42 {
                    assert!(echo.is_empty());
                    bulk += 1;
                } else if byte == b'\n' {
                    let marker = String::from_utf8(std::mem::take(&mut echo)).unwrap();
                    let number = marker.strip_prefix("RTT:").unwrap().parse::<u64>().unwrap();
                    rtt.push(sent.remove(&number).unwrap().elapsed().as_secs_f64() * 1000.0);
                } else {
                    echo.push(byte);
                }
            }
            if sequence == 1 && bulk >= 64 * 1024 {
                sequence = 2;
                ping(&mut session.browser, sequence, &mut sent).await;
            }
            if sequence == 2 && bulk >= 512 * 1024 {
                sequence = 3;
                ping(&mut session.browser, sequence, &mut sent).await;
            }
            if session.slow {
                tokio::time::sleep(Duration::from_millis(15)).await;
            }
            session
                .browser
                .send(Message::Text(
                    json!({"type":"ack","offset":produced}).to_string().into(),
                ))
                .await
                .unwrap();
            if bulk == BULK && rtt.len() == 3 {
                break;
            }
        }
    })
    .await
    .expect("ten simultaneous fast/slow sessions must make progress");
    let transfer_ms = started.elapsed().as_secs_f64() * 1000.0;
    let closing = Instant::now();
    reqwest::Client::new()
        .post(format!(
            "http://{address}/api/v1/terminal-sessions/{}/close",
            session.id
        ))
        .header("cookie", &session.cookie)
        .header("x-artifactory-csrf", "1")
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let (peak, stalled, device_closed) = tokio::time::timeout(Duration::from_millis(5250), device)
        .await
        .unwrap()
        .unwrap();
    let control_closed = tokio::time::timeout(Duration::from_millis(5250), closed)
        .await
        .unwrap()
        .unwrap();
    stop.cancel();
    let heartbeats = heartbeat.await.unwrap();
    assert!(!heartbeats.is_empty());
    assert!(
        heartbeats.iter().all(|ms| *ms < 2000.0),
        "terminal data must not starve Control heartbeats: {heartbeats:?}"
    );
    assert!(closing.elapsed() < Duration::from_millis(5250));
    if session.slow {
        assert!(stalled, "slow consumer must exercise the byte credit limit");
    }
    json!({"slow":session.slow,"bulk_bytes":bulk,"wire_data_bytes":produced,"max_inflight_bytes":peak,"credit_stalled":stalled,"transfer_ms":transfer_ms,"echo_rtt_ms":rtt,"control_heartbeat_ms":heartbeats,"device_close_ms":device_closed.duration_since(closing).as_secs_f64()*1000.0,"control_close_ms":control_closed.duration_since(closing).as_secs_f64()*1000.0})
}

fn rss() -> Option<u64> {
    #[cfg(unix)]
    {
        let output = std::process::Command::new("ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output()
            .ok()?;
        String::from_utf8(output.stdout)
            .ok()?
            .trim()
            .parse::<u64>()
            .ok()
            .map(|kb| kb * 1024)
    }
    #[cfg(not(unix))]
    {
        None
    }
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn terminal_ten_sessions_bound_credit_and_keep_control_responsive() {
    let db = Database::with_tenant().await;
    let owner_state = db.state.clone();
    let browser_state = replica(&owner_state);
    let device_state = replica(&owner_state);
    for state in [&owner_state, &browser_state, &device_state] {
        artifactory_control_plane::terminals::start(state.clone())
            .await
            .unwrap();
    }
    let certificates = Certificates::new();
    let owner = Replica::start(owner_state, &certificates).await;
    let entry = Replica::start(device_state.clone(), &certificates).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let http_state = browser_state.clone();
    let http = tokio::spawn(async move {
        axum::serve(listener, api::router(http_state))
            .await
            .unwrap();
    });
    let mut sessions = Vec::new();
    for index in 0..10 {
        sessions.push(prepare(&db, &owner, &entry, &certificates, address, index).await);
    }
    let baseline = rss();
    let resources = Arc::new(Mutex::new(
        json!({"rss_peak_bytes":baseline,"database_connections_peak":0,"database_active_peak":0,"samples":0}),
    ));
    let probe_resources = resources.clone();
    let probe_pool = db.pool.clone();
    let probe_stop = CancellationToken::new();
    let probing = probe_stop.clone();
    let probe = tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_millis(100));
        loop {
            tokio::select! {_=probing.cancelled()=>break,_=tick.tick()=>{}}
            let(connections,active):(i64,i64)=sql::tuple(&mut probe_pool.clone(),"SELECT count(*),count(*) FILTER(WHERE state='active') FROM pg_stat_activity WHERE datname=current_database()",params![]).await.unwrap();
            let rss = rss();
            let mut snapshot = probe_resources.lock().unwrap();
            snapshot["database_connections_peak"] = json!(
                snapshot["database_connections_peak"]
                    .as_i64()
                    .unwrap()
                    .max(connections)
            );
            snapshot["database_active_peak"] = json!(
                snapshot["database_active_peak"]
                    .as_i64()
                    .unwrap()
                    .max(active)
            );
            if let Some(rss) = rss {
                snapshot["rss_peak_bytes"] =
                    json!(snapshot["rss_peak_bytes"].as_u64().unwrap_or(0).max(rss));
            }
            snapshot["samples"] = json!(snapshot["samples"].as_u64().unwrap() + 1);
        }
    });
    let barrier = Arc::new(Barrier::new(11));
    let mut tasks = Vec::new();
    for session in sessions {
        tasks.push(tokio::spawn(run(session, address, barrier.clone())));
    }
    barrier.wait().await;
    let mut reports = Vec::new();
    for task in tasks {
        reports.push(task.await.unwrap());
    }
    probe_stop.cancel();
    probe.await.unwrap();
    let snapshot = resources.lock().unwrap().clone();
    if let (Some(initial), Some(peak)) = (baseline, snapshot["rss_peak_bytes"].as_u64()) {
        assert!(
            peak.saturating_sub(initial) < 256 * 1024 * 1024,
            "bounded terminal queues must not grow unbounded memory"
        );
    }
    let report = json!({"scope":"10 virtual mTLS devices; 3 independent Control states/TLS listeners in one test process; real PostgreSQL and WS; no PTY, gateway, physical devices or Windows service account","sessions":reports,"rss_baseline_bytes":baseline,"resources":snapshot,"database_scope":"pg_stat_activity for entire shared test database; fixture pool maximum 10 plus admin pool maximum 2","credit_limit_bytes":WINDOW});
    let evidence = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.local/terminal/load-report.json");
    std::fs::create_dir_all(evidence.parent().unwrap()).unwrap();
    std::fs::write(evidence, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    browser_state.coordination.stop();
    device_state.coordination.stop();
    http.abort();
    let _ = http.await;
    owner.stop().await;
    entry.stop().await;
    db.close().await;
}

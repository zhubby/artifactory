//! [INPUT]: 依赖隔离 PostgreSQL、HTTP 鉴权与对象响应夹具。
//! [OUTPUT]: 验证日志分页、过滤、授权及终态流的结束与恢复。
//! [POS]: 构建日志的真实数据库与 HTTP 边界验收。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

async fn events(db: &Database, cookie: &str, build: Uuid, cursor: i64) -> axum::response::Response {
    let response = api::router(db.state.clone())
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/builds/{build}/events"))
                .header("cookie", cookie)
                .header("X-Artifactory-Tenant", Uuid::from_u128(1).to_string())
                .header("Last-Event-ID", cursor.to_string())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    response
}

async fn closed_stream(response: axum::response::Response) -> String {
    let body = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        axum::body::to_bytes(response.into_body(), 1024 * 1024),
    )
    .await
    .expect("completed stream must drain and close promptly")
    .unwrap();
    String::from_utf8(body.to_vec()).unwrap()
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn history_pages_are_ordered_filtered_authorized_and_terminal_streams_drain() {
    let mut db = Database::with_tenant().await;
    let (_, build, job) = db.fixture().await;
    let node = db.node().await;
    let attempt = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(), "INSERT INTO attempts(id,job_id,node_id,lease_token,lease_expires_at,status,command_id) VALUES($1,$2,$3,'test',now(),'succeeded',$4)", params![attempt, job, node, Uuid::new_v4()]).await.unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE builds SET status='succeeded',finished_at=now() WHERE id=$1",
        params![build],
    )
    .await
    .unwrap();
    sql::execute(&mut db.pool.clone(), "INSERT INTO log_chunks(attempt_id,sequence,step,object_key,sha256,size) SELECT $1,n,CASE WHEN n%2=0 THEN 'compile' ELSE 'verify' END,'history/'||n,'test',8 FROM generate_series(1,205) n", params![attempt]).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let unavailable = Arc::new(AtomicBool::new(false));
    let failures = Arc::new(AtomicUsize::new(0));
    let object_unavailable = unavailable.clone();
    let object_failures = failures.clone();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            axum::Router::new().fallback(move || {
                let unavailable = object_unavailable.clone();
                let failures = object_failures.clone();
                async move {
                    if unavailable.load(Ordering::SeqCst) {
                        failures.fetch_add(1, Ordering::SeqCst);
                        (StatusCode::NOT_FOUND, "object unavailable")
                    } else {
                        (StatusCode::OK, "fixture log\n")
                    }
                }
            }),
        )
        .await
        .unwrap();
    });
    let mut settings = (*db.state.settings).clone();
    settings.s3_endpoint = endpoint.clone();
    settings.s3_public_endpoint = endpoint;
    db.state.storage = Arc::new(Storage::new(&settings).unwrap());
    let (_, cookie) = account(&db, "log-reader", "admin").await;
    let tenant_scope = Some(Uuid::from_u128(1));
    let path = format!("/builds/{build}/logs?attempt_id={attempt}");
    let (status, page) = request(&db, &cookie, tenant_scope, "GET", &path, Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["logs"].as_array().unwrap().len(), 100);
    assert_eq!(page["logs"][0]["id"], 106);
    assert_eq!(page["logs"][99]["id"], 205);
    assert_eq!(page["logs"][0]["text"], "fixture log\n");
    assert_eq!(page["next_before"], 106);
    let (_, older) = request(
        &db,
        &cookie,
        tenant_scope,
        "GET",
        &format!("{path}&before=106"),
        Value::Null,
    )
    .await;
    assert_eq!(older["logs"][0]["id"], 6);
    assert_eq!(older["logs"][99]["id"], 105);
    let (_, first) = request(
        &db,
        &cookie,
        tenant_scope,
        "GET",
        &format!("{path}&before=6"),
        Value::Null,
    )
    .await;
    assert_eq!(first["logs"].as_array().unwrap().len(), 5);
    assert_eq!(first["next_before"], Value::Null);
    let (_, filtered) = request(
        &db,
        &cookie,
        tenant_scope,
        "GET",
        &format!("{path}&step=compile"),
        Value::Null,
    )
    .await;
    assert!(
        filtered["logs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|log| log["step"] == "compile")
    );
    assert_eq!(filtered["logs"][0]["id"], 6);
    let (_, empty) = request(
        &db,
        &cookie,
        tenant_scope,
        "GET",
        &format!("/builds/{build}/logs?attempt_id={}", Uuid::new_v4()),
        Value::Null,
    )
    .await;
    assert_eq!(empty["logs"], json!([]));
    let (status, _) = request(
        &db,
        &cookie,
        tenant_scope,
        "GET",
        &format!("{path}&before=-1"),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let other = tenant(&db, &cookie, "Other log tenant").await;
    let (status, _) = request(&db, &cookie, Some(other), "GET", &path, Value::Null).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let text = closed_stream(events(&db, &cookie, build, 0).await).await;
    assert_eq!(text.matches("event: log\n").count(), 205);
    assert_eq!(text.matches("event: complete\n").count(), 1);
    assert!(text.rfind("event: log\n").unwrap() < text.find("event: complete\n").unwrap());

    for (status, cursor, count) in [("failed", 105, 100), ("canceled", 205, 0)] {
        sql::execute(
            &mut db.pool.clone(),
            "UPDATE builds SET status=$2 WHERE id=$1",
            params![build, status],
        )
        .await
        .unwrap();
        let text = closed_stream(events(&db, &cookie, build, cursor).await).await;
        assert_eq!(text.matches("event: log\n").count(), count);
        assert_eq!(text.matches("event: complete\n").count(), 1);
    }

    unavailable.store(true, Ordering::SeqCst);
    let (status, _) = request(&db, &cookie, tenant_scope, "GET", &path, Value::Null).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    failures.store(0, Ordering::SeqCst);
    let response = events(&db, &cookie, build, 204).await;
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024);
    tokio::pin!(body);
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(1250), &mut body)
            .await
            .is_err(),
        "missing log must prevent complete"
    );
    assert!(
        (1..=3).contains(&failures.load(Ordering::SeqCst)),
        "storage failure must not cause a busy retry loop"
    );
    unavailable.store(false, Ordering::SeqCst);
    let body = tokio::time::timeout(std::time::Duration::from_secs(5), body)
        .await
        .unwrap()
        .unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert_eq!(text.matches("event: log\n").count(), 1);
    assert!(text.contains("id: 205\n"));
    assert_eq!(text.matches("event: complete\n").count(), 1);
    server.abort();
    db.close().await;
}

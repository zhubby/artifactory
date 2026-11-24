//! [INPUT]: 依赖真实租户会话、源码阶段和构建历史。
//! [OUTPUT]: 验证过程查询的权限、精确投影与旧记录边界。
//! [POS]: 构建观察接口的 HTTP 数据库回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn build_activity_preserves_source_history_without_exposing_credentials() {
    let db = Database::new().await;
    let (_, owner) = account(&db, "activity-owner", "viewer").await;
    let (_, outsider) = account(&db, "activity-outsider", "viewer").await;
    let tenant_id = tenant(&db, &owner, "activity").await;
    let project_id = project(&db, &owner, tenant_id, "observed").await;
    let build = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(),
        "WITH r AS (INSERT INTO releases(id,project_id,version) VALUES(gen_random_uuid(),$1,'1.0.0') RETURNING id,version) INSERT INTO builds(id,project_id,requested_ref,status,source_repository_url,release_id,release_version,error,finished_at) SELECT $2,$1,'main','failed','https://example.com/repo.git',id,version,'source preparation failed',now() FROM r",
        params![project_id,build]).await.unwrap();
    sql::execute(&mut db.pool.clone(),
        "INSERT INTO source_tasks(id,tenant_id,project_id,build_id,purpose,repository_url,reference,status,deadline_at,bindings,source_execution,progress,result) VALUES($1,$2,$3,$1,'resolve','https://example.com/repo.git','main','failed',now(),$4,$5,$6,$7)",
        params![build,tenant_id,project_id,json!([]),
            json!({"attempt":1,"name":"source-pod","uid":"pod-uid","cr_uid":"cr-uid","private":"private-credential"}),
            json!([{"sequence":1,"stage":"resolve_ref","state":"started","attempt":1,"pod":"source-pod","created_at":"2026-11-24T08:25:10Z"}]),
            json!({"diagnostic":{"stage":"resolve_ref","code":"timeout"},"private":"must-not-leave-store"})]).await.unwrap();
    let path = format!("/builds/{build}/activity");
    let (status, data) = request(&db, &owner, Some(tenant_id), "GET", &path, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{data}");
    assert_eq!(data["source"]["diagnostic"]["code"], "timeout");
    assert_eq!(data["source"]["history_available"], true);
    assert_eq!(data["source"]["attempt"], 1);
    assert!(
        data["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["action"] == "source_resolve_ref")
    );
    assert!(!data.to_string().contains("must-not-leave-store"));
    assert!(!data.to_string().contains("private-credential"));
    let (status, _) = request(&db, &outsider, Some(tenant_id), "GET", &path, json!({})).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE source_tasks SET progress='[]',result=NULL WHERE id=$1",
        params![build],
    )
    .await
    .unwrap();
    let (_, data) = request(&db, &owner, Some(tenant_id), "GET", &path, json!({})).await;
    assert_eq!(data["source"]["history_available"], false);
    assert!(data["source"]["diagnostic"].is_null());
    db.close().await;
}

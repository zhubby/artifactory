//! [INPUT]: 依赖真实计划解析、kube 客户端与隔离 HTTP API。
//! [OUTPUT]: 验证缺省与显式池、404 和查询故障分层及原生计划隔离。
//! [POS]: 镜像池准入回归；不连接真实集群。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use axum::{
    Json, Router,
    extract::State,
    http::{StatusCode, Uri},
};
use std::sync::{Arc, Mutex};

fn plan(pool: Option<&str>, context: &Value) -> ExecutionPlan {
    let mut value = json!({"summary":"build image","evidence":[{"path":"Dockerfile","reason":"image entry"}],"recipe":{"version":1,"targets":{"app":{"executor":"buildkit","os":"linux","arch":"amd64","application_platform":null,"image":{"name":"app","platforms":["linux/amd64"]}}}}});
    if let Some(pool) = pool {
        value["recipe"]["targets"]["app"]["image"]["pool"] = json!(pool);
    }
    normalize_plan_pools(&mut value, context).unwrap();
    ExecutionPlan::parse(value).unwrap()
}

#[test]
fn frozen_default_only_fills_an_omitted_pool() {
    let context = json!({"image_builder_pool":"ci-images","pool_ids":["04499bdc-4b50-44f1-b460-0afc0233de54"]});
    assert_eq!(
        plan(None, &context).recipe.targets["app"]
            .image
            .as_ref()
            .unwrap()
            .pool,
        "ci-images"
    );
    assert_eq!(
        plan(Some("custom-images"), &context).recipe.targets["app"]
            .image
            .as_ref()
            .unwrap()
            .pool,
        "custom-images"
    );
    assert_eq!(
        plan(None, &json!({})).recipe.targets["app"]
            .image
            .as_ref()
            .unwrap()
            .pool,
        DEFAULT_BUILDER_POOL
    );
    assert_eq!(pool_name(None).unwrap(), DEFAULT_BUILDER_POOL);
    assert!(pool_name(Some("Native/Pool")).is_err());
    assert!(pool_name(Some("")).is_err());
}

#[tokio::test]
async fn only_existing_builder_pools_are_accepted_and_lookup_failures_stay_errors() {
    async fn handle(
        State(requests): State<Arc<Mutex<Vec<String>>>>,
        uri: Uri,
    ) -> (StatusCode, Json<Value>) {
        requests.lock().unwrap().push(uri.path().to_owned());
        let name = uri.path().rsplit('/').next().unwrap();
        if name == "timeout" {
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
        let code = match name {
            "ci-images" | "custom-images" => 200,
            "denied" => 403,
            "unavailable" => 503,
            _ => 404,
        };
        (
            StatusCode::from_u16(code).unwrap(),
            Json(if code == 200 {
                json!({"apiVersion":"images.artifactory.dev/v1alpha1","kind":"BuilderPool","metadata":{"name":name}})
            } else {
                json!({"apiVersion":"v1","kind":"Status","status":"Failure","reason":if code == 404 {"NotFound"} else {"Injected"},"message":"private API detail","code":code})
            }),
        )
    }
    let requests = Arc::new(Mutex::new(vec![]));
    let app = Router::new().fallback(handle).with_state(requests.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut config = kube::Config::new(
        format!("http://{}", listener.local_addr().unwrap())
            .parse()
            .unwrap(),
    );
    config.default_retry = false;
    let client = Client::try_from(config).unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let context = json!({"image_builder_pool":"ci-images"});
    for pool in [None, Some("custom-images")] {
        let plan = plan(pool, &context);
        validate_with(
            &client,
            "images",
            &plan,
            Instant::now() + Duration::from_secs(1),
        )
        .await
        .unwrap();
    }
    let missing = validate_with(
        &client,
        "images",
        &plan(Some("04499bdc-4b50-44f1-b460-0afc0233de54"), &context),
        Instant::now() + Duration::from_secs(1),
    )
    .await
    .unwrap_err();
    assert!(missing.to_string().contains("does not exist"));
    assert!(matches!(missing, PoolValidationError::MissingPool(_)));
    for (name, code) in [("denied", 403), ("unavailable", 503)] {
        let error = validate_with(
            &client,
            "images",
            &plan(Some(name), &context),
            Instant::now() + Duration::from_secs(1),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, PoolValidationError::Lookup(actual) if actual == code));
        assert!(!error.to_string().contains("private API detail"));
    }
    let timeout = validate_with(
        &client,
        "images",
        &plan(Some("timeout"), &context),
        Instant::now() + Duration::from_millis(20),
    )
    .await
    .unwrap_err();
    assert!(matches!(timeout, PoolValidationError::TimedOut));
    assert!(requests.lock().unwrap().iter().all(|path| {
        path.starts_with("/apis/images.artifactory.dev/v1alpha1/namespaces/images/builderpools/")
    }));
    let native = ExecutionPlan::parse(json!({"summary":"build native","evidence":[{"path":"Makefile","reason":"native entry"}],"recipe":{"version":1,"targets":{"cli":{"os":"linux","arch":"amd64","application_platform":null,"steps":[{"name":"build","run":"make"}],"artifacts":["dist/cli"]}}}})).unwrap();
    let count = requests.lock().unwrap().len();
    validate_with(
        &client,
        "images",
        &native,
        Instant::now() + Duration::from_secs(1),
    )
    .await
    .unwrap();
    validate_plan_pools(&native).await.unwrap();
    assert_eq!(requests.lock().unwrap().len(), count);
    server.abort();
}

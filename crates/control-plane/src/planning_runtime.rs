//! [INPUT]: 依赖项目读取授权、业务规划记录与 Coordinator 运行查询。
//! [OUTPUT]: 为最新活动运行附加用量与本代源码准备摘要。
//! [POS]: 公开规划查询的安全投影；保留业务确认值。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{
    auth::Auth,
    error::Result,
    store::{
        ControlStore,
        dto::{BuildPlans, PlanningRuntime},
    },
};
use serde_json::{Value, json};
use std::{future::Future, time::Duration};
use uuid::Uuid;

pub(crate) async fn plans<F, Fut>(
    store: &ControlStore,
    build: Uuid,
    auth: &Auth,
    read: F,
) -> Result<BuildPlans>
where
    F: FnOnce(Uuid) -> Fut,
    Fut: Future<Output = anyhow::Result<Value>>,
{
    let mut plans = store.plans(build, auth).await?;
    let Some(run) = plans
        .runs
        .last_mut()
        .filter(|run| matches!(run.state.as_str(), "queued" | "leased"))
    else {
        return Ok(plans);
    };
    run.runtime = match tokio::time::timeout(Duration::from_secs(2), read(run.id)).await {
        Ok(Ok(value)) => summary(&value),
        _ => None,
    };
    if run.runtime.is_none() {
        run.runtime_unavailable = Some(true);
    }
    Ok(plans)
}

fn summary(value: &Value) -> Option<PlanningRuntime> {
    let status = value["status"].as_str()?;
    if ![
        "queued",
        "provisioning",
        "running",
        "succeeded",
        "needs_input",
        "failed",
        "canceled",
    ]
    .contains(&status)
    {
        return None;
    }
    let generation = value["lifecycle"]["generation"]
        .as_i64()
        .filter(|value| *value > 0)?;
    let calls = u32::try_from(value["lifecycle"]["calls"].as_u64()?).ok()?;
    let mut usage = serde_json::Map::new();
    for field in ["input", "output", "cacheRead", "cacheWrite", "totalTokens"] {
        if let Some(value) = value["usage"][field].as_u64() {
            usage.insert(field.into(), json!(value));
        }
    }
    if let Some(cost) = value["usage"]["cost"]
        .as_f64()
        .filter(|cost| cost.is_finite() && *cost >= 0.0)
    {
        usage.insert("cost".into(), json!(cost));
    }
    Some(PlanningRuntime {
        status: status.into(),
        generation,
        calls,
        usage: Value::Object(usage),
        preparation: preparation(value, generation),
    })
}

fn preparation(value: &Value, generation: i64) -> Option<Value> {
    let summary = value["preparation"].as_object()?;
    let execution = &value["execution"];
    if summary.get("execution")? != execution || execution["generation"].as_i64()? != generation {
        return None;
    }
    let mut result = serde_json::Map::new();
    for (field, source) in [
        ("run_id", "run_id"),
        ("pod_uid", "uid"),
        ("pod_name", "name"),
    ] {
        result.insert(field.into(), json!(execution[source].as_str()?));
    }
    result.insert("generation".into(), json!(generation));
    if let Some(stage) = summary
        .get("current_stage")
        .and_then(Value::as_str)
        .filter(|stage| {
            [
                "authorization",
                "resolve_ref",
                "pin_ref",
                "inspect",
                "fetch",
                "submodules",
                "lfs",
                "verify",
                "manifest",
                "ready",
            ]
            .contains(stage)
        })
    {
        result.insert("current_stage".into(), json!(stage));
    }
    if let Some(state) = summary
        .get("current_state")
        .or_else(|| summary.get("state"))
        .or_else(|| summary.get("progress").and_then(|v| v.get("state")))
        .and_then(Value::as_str)
        .filter(|state| ["started", "completed"].contains(state))
    {
        result.insert("current_state".into(), json!(state));
    }
    for field in ["ready_at", "started_at", "updated_at"] {
        if let Some(at) = summary
            .get(field)
            .and_then(Value::as_str)
            .filter(|at| chrono::DateTime::parse_from_rfc3339(at).is_ok())
        {
            result.insert(field.into(), json!(at));
        }
    }
    if let Some(diagnostic) = summary.get("diagnostic").and_then(|value| {
        serde_json::from_value::<artifactory_source_checkout::SourceDiagnostic>(value.clone()).ok()
    }) {
        result.insert("diagnostic".into(), json!(diagnostic));
    }
    Some(Value::Object(result))
}

#[cfg(test)]
#[path = "store/planning_runtime_tests.rs"]
mod tests;

#[cfg(test)]
mod preparation_tests {
    use super::*;

    #[test]
    fn preparation_projects_only_current_execution_and_safe_fields() {
        let mut value = json!({"execution":{"run_id":"run","generation":2,"uid":"pod-2","name":"planning-2"},
            "preparation":{"execution":{"run_id":"run","generation":2,"uid":"pod-2","name":"planning-2"},
                "current_stage":"lfs","progress":{"state":"started"},"ready_at":"2026-11-25T12:00:00Z",
                "secret":"do-not-project","credential":"private"}});
        let projected = preparation(&value, 2).unwrap();
        assert_eq!(projected["pod_uid"], "pod-2");
        assert_eq!(projected["current_state"], "started");
        assert_eq!(projected["ready_at"], "2026-11-25T12:00:00Z");
        assert!(!projected.to_string().contains("private"));
        assert!(preparation(&value, 3).is_none());
        value["preparation"]["execution"]["uid"] = json!("pod-old");
        assert!(preparation(&value, 2).is_none());
    }
}

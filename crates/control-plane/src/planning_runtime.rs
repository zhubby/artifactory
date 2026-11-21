//! [INPUT]: 依赖项目读取授权、业务规划记录与 Coordinator 运行查询。
//! [OUTPUT]: 为最新活动运行附加只读运行时摘要。
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
    })
}

#[cfg(test)]
#[path = "store/planning_runtime_tests.rs"]
mod tests;

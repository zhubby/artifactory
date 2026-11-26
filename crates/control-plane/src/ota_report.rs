//! [INPUT]: 设备观察信封、上次序号和数据库冻结资产。
//! [OUTPUT]: 校验进度身份与序号，保留已接受观察并投影停滞。
//! [POS]: OTA状态旁路观测准入，不决定激活或健康成功。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use anyhow::{Result, ensure};
use artifactory_emissary_protocol::ota::Observability;
use serde_json::Value;

pub fn observation(status: &Value) -> Result<Option<Observability>> {
    let Some(value) = status.get("observability").filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let observation: Observability = serde_json::from_value(value.clone())?;
    observation.validate()?;
    Ok(Some(observation))
}

pub fn accept(
    status: &mut Value,
    previous: &Value,
    generation: Option<i64>,
    manifest: Option<&str>,
    os: &str,
    arch: &str,
) -> Result<()> {
    ensure!(
        previous["generation"]
            .as_i64()
            .zip(status["generation"].as_i64())
            .is_none_or(|(old, new)| new >= old),
        "older OTA status generation"
    );
    let Some(mut current) = observation(status)? else {
        if previous["update_id"] == status["update_id"]
            && previous["generation"] == status["generation"]
            && let Some(old) = observation(previous)?
        {
            status["observability"] = serde_json::to_value(project_stalled(old, status))?;
        }
        return Ok(());
    };
    if let Some(progress) = &current.progress {
        ensure!(
            Some(progress.generation) == generation
                && status["update_id"].as_str() == Some(progress.update_id.to_string().as_str()),
            "stale OTA observation"
        );
        let manifest: super::Manifest = serde_json::from_str(
            manifest.ok_or_else(|| anyhow::anyhow!("missing frozen manifest"))?,
        )?;
        let asset = manifest
            .assets
            .iter()
            .find(|a| a.os == os && a.arch == arch)
            .ok_or_else(|| anyhow::anyhow!("frozen platform missing"))?;
        ensure!(
            progress.asset_sha256 == asset.sha256 && progress.bytes_total == asset.size,
            "progress does not match frozen asset"
        );
    }
    if let Some(old) = observation(previous)? {
        let same_update = previous["update_id"] == status["update_id"]
            && previous["generation"] == status["generation"];
        if same_update && current.sequence <= old.sequence {
            status["observability"] = serde_json::to_value(project_stalled(old, status))?;
            return Ok(());
        }
    }
    current = project_stalled(current, status);
    status["observability"] = serde_json::to_value(current)?;
    Ok(())
}
fn project_stalled(mut current: Observability, status: &Value) -> Observability {
    current.stalled = status["phase"] == "draining"
        && current.drain.as_ref().is_some_and(|d| {
            d.active_workers > 0
                || d.pending_log_records > 0
                || d.pending_terminal_records > 0
                || d.terminals > 0
                || d.operations > 0
        })
        && chrono::Utc::now()
            .signed_duration_since(current.last_progress_at)
            .num_seconds()
            >= 90;
    current
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_an_observation_with_secret_fields() {
        assert!(
            observation(&serde_json::json!({"observability":{"schema_version":1,"url":"secret"}}))
                .is_err()
        );
    }
    fn status(sequence: u64, done: u64) -> Value {
        let id = uuid::Uuid::from_u128(3);
        serde_json::json!({"update_id":id,"generation":7,"observability":Observability {
            schema_version:1,sequence,launcher_version:"0.2.3".into(),launcher_capabilities:vec!["ota-progress-v1".into()],
            progress:Some(artifactory_emissary_protocol::ota::Progress {update_id:id,generation:7,asset_sha256:"a".repeat(64),sequence,subphase:"download".into(),bytes_done:done,bytes_total:10,download_attempt:1,last_progress_at:chrono::Utc::now(),updated_at:chrono::Utc::now(),error_code:None}),drain:None,last_progress_at:chrono::Utc::now(),stalled:false
        }})
    }
    fn manifest() -> String {
        serde_json::to_string(&super::super::Manifest {
            version: "0.2.3".into(),
            commit: "b".repeat(40),
            protocol_min: 1,
            protocol_max: 1,
            assets: vec![super::super::Asset {
                os: "linux".into(),
                arch: "amd64".into(),
                name: "artifactory-emissary-linux-amd64".into(),
                size: 10,
                sha256: "a".repeat(64),
            }],
        })
        .unwrap()
    }
    #[test]
    fn late_progress_is_ignored_and_missing_progress_keeps_last_observation() {
        let previous = status(4, 8);
        let mut late = status(3, 2);
        accept(
            &mut late,
            &previous,
            Some(7),
            Some(&manifest()),
            "linux",
            "amd64",
        )
        .unwrap();
        assert_eq!(late["observability"], previous["observability"]);
        let mut throttled =
            serde_json::json!({"update_id":uuid::Uuid::from_u128(3),"generation":7});
        accept(
            &mut throttled,
            &previous,
            Some(7),
            Some(&manifest()),
            "linux",
            "amd64",
        )
        .unwrap();
        assert_eq!(throttled["observability"], previous["observability"]);
    }
    #[test]
    fn progress_cannot_change_asset_or_generation() {
        let mut changed = status(5, 8);
        changed["observability"]["progress"]["asset_sha256"] = serde_json::json!("c".repeat(64));
        assert!(
            accept(
                &mut changed,
                &status(4, 7),
                Some(7),
                Some(&manifest()),
                "linux",
                "amd64"
            )
            .is_err()
        );
        let mut older = status(5, 8);
        older["generation"] = serde_json::json!(6);
        assert!(
            accept(
                &mut older,
                &status(4, 7),
                Some(7),
                Some(&manifest()),
                "linux",
                "amd64"
            )
            .is_err()
        );
    }
    #[test]
    fn omitted_or_late_observation_recomputes_stalled_only_during_drain() {
        let mut previous = status(4, 8);
        previous["phase"] = serde_json::json!("draining");
        previous["observability"]["last_progress_at"] =
            serde_json::json!(chrono::Utc::now() - chrono::Duration::seconds(91));
        previous["observability"]["drain"] =
            serde_json::to_value(artifactory_emissary_protocol::ota::Drain {
                active_workers: 1,
                ..Default::default()
            })
            .unwrap();
        let mut missing = serde_json::json!({"phase":"draining","update_id":previous["update_id"],"generation":7});
        accept(
            &mut missing,
            &previous,
            Some(7),
            Some(&manifest()),
            "linux",
            "amd64",
        )
        .unwrap();
        assert_eq!(missing["observability"]["stalled"], true);
        missing["phase"] = serde_json::json!("succeeded");
        let mut late = missing.clone();
        accept(
            &mut late,
            &missing,
            Some(7),
            Some(&manifest()),
            "linux",
            "amd64",
        )
        .unwrap();
        assert_eq!(late["observability"]["stalled"], false);
    }
}

//! [INPUT]: 真实Runner子进程、mTLS对端和可恢复的OTA文件路径故障。
//! [OUTPUT]: 验证健康会话围栏、健康确认及排空保存失败不终止runtime。
//! [POS]: OTA控制面与本地保存的组合回归；不安装服务。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
#![cfg(unix)]
mod support;
use artifactory_emissary::{config::atomic_json, ota};
use std::{fs, path::Path, time::Duration};
use tokio::process::Command;

async fn wait_for(mut condition: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(15), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("OTA health persistence did not recover");
}

fn read_health(directory: &Path) -> Option<ota::Health> {
    artifactory_emissary::config::read_json(&directory.join("health.json")).ok()
}

#[tokio::test]
async fn health_and_drain_storage_failures_keep_the_same_runtime_and_connection() {
    tokio::time::timeout(Duration::from_secs(120), exercise())
        .await
        .expect("runtime health fault acceptance timed out");
}

async fn exercise() {
    let directory = tempfile::tempdir().unwrap();
    let mut fixture = support::Fixture::start(directory.path(), &[&rustls::version::TLS13]).await;
    let config = directory.path().join("emissary.json");
    fixture.cfg.save(&config).unwrap();
    let ota_dir = fixture.cfg.root.join("ota");
    atomic_json(&ota_dir.join("state.json"), &ota::State::default()).unwrap();
    // 首次连接前无法保存会话围栏时，进程等待而不是退出。
    fs::create_dir(ota_dir.join("health.json")).unwrap();
    let mut runtime = Command::new(env!("CARGO_BIN_EXE_artifactory-emissary"))
        .args(["run", "--config"])
        .arg(&config)
        .env("ARTIFACTORY_OTA_DIRECTORY", &ota_dir)
        .stdin(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let pid = runtime.id().unwrap();
    wait_for(|| {
        assert!(
            runtime.try_wait().unwrap().is_none(),
            "runtime exited during startup"
        );
        fixture.cfg.root.join("operations/journal.json").is_file()
    })
    .await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(
        runtime.try_wait().unwrap().is_none(),
        "initial health reset exited runtime"
    );
    assert_eq!(
        fixture
            .metrics
            .tcp_accepted
            .load(std::sync::atomic::Ordering::SeqCst),
        0,
        "control connected before the health session fence was durable"
    );
    fs::remove_dir(ota_dir.join("health.json")).unwrap();
    let mut connection = fixture.connection().await;
    wait_for(|| read_health(&ota_dir).is_some_and(|health| !health.id.is_empty())).await;
    let original_session = read_health(&ota_dir).unwrap().session;

    // 连接存续期间的健康保存失败不能消耗进程或mTLS连接。
    fs::remove_file(ota_dir.join("health.json")).unwrap();
    fs::create_dir(ota_dir.join("health.json")).unwrap();
    connection.event("heartbeat").await;
    connection.event("heartbeat").await;
    assert!(
        runtime.try_wait().unwrap().is_none(),
        "health ACK exited runtime"
    );
    fs::remove_dir(ota_dir.join("health.json")).unwrap();
    wait_for(|| read_health(&ota_dir).is_some_and(|health| !health.id.is_empty())).await;
    assert_eq!(read_health(&ota_dir).unwrap().session, original_session);

    // 排空标记失败时保持draining；解除故障后同连接再次确认。
    fs::create_dir(ota_dir.join("drained.json")).unwrap();
    atomic_json(
        &ota_dir.join("state.json"),
        &ota::State {
            update_id: "drain-recovery".into(),
            phase: "draining".into(),
            ..Default::default()
        },
    )
    .unwrap();
    loop {
        let heartbeat = connection.event("heartbeat").await;
        let payload: serde_json::Value = serde_json::from_str(&heartbeat.payload_json).unwrap();
        if payload["ota"]["activation_pending"] == true {
            break;
        }
    }
    connection.event("heartbeat").await;
    assert!(
        runtime.try_wait().unwrap().is_none(),
        "drain ACK exited runtime"
    );
    assert!(ota_dir.join("drained.json").is_dir());
    fs::remove_dir(ota_dir.join("drained.json")).unwrap();
    wait_for(|| ota_dir.join("drained.json").is_file()).await;
    let marker: serde_json::Value =
        artifactory_emissary::config::read_json(&ota_dir.join("drained.json")).unwrap();
    assert_eq!(marker["id"], "drain-recovery");
    assert_eq!(read_health(&ota_dir).unwrap().session, original_session);
    assert_eq!(runtime.id(), Some(pid));
    assert!(runtime.try_wait().unwrap().is_none());
    assert!(
        std::process::Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        tokio::time::timeout(Duration::from_secs(15), runtime.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
}

//! [INPUT]: 依赖真实Spool文件和持久缺口计数。
//! [OUTPUT]: 验证相同拒收重试只累计一次。
//! [POS]: 普通采样删除事务的故障回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
fn sample() -> Batch {
    let mut batch: Batch = serde_json::from_slice(include_bytes!(
        "../../../emissary-protocol/tests/fixtures/telemetry-batch.json"
    ))
    .unwrap();
    batch.id = uuid::Uuid::new_v4();
    batch.inventory = None;
    batch.host = None;
    batch.processes.clear();
    batch.sampled_at = chrono::Utc::now();
    batch
}
#[test]
fn repeated_rejection_counts_one_durable_gap_for_identical_original() {
    let root = tempfile::tempdir().unwrap();
    let mut spool = Spool::open(root.path().into(), 4096).unwrap();
    spool.push(&mut sample()).unwrap();
    let (path, bytes) = spool.first().unwrap().unwrap();
    spool.fail_removal = true;
    assert!(spool.reject(&path, &bytes, "bounds_exceeded").is_err());
    for _ in 0..2 {
        spool.reject(&path, &bytes, "bounds_exceeded").unwrap();
    }
    assert_eq!(spool.diagnostics()["bounds_exceeded_batches"], 1);
    assert_eq!(spool.dropped(), 1);
    let repeated = path
        .parent()
        .unwrap()
        .join("00000000000000000000-replayed.batch");
    fs::write(&repeated, &bytes).unwrap();
    drop(spool);
    let mut reopened = Spool::open(root.path().into(), 4096).unwrap();
    reopened
        .reject(&repeated, &bytes, "bounds_exceeded")
        .unwrap();
    assert_eq!(reopened.dropped(), 1, "相同ID与摘要重放不重复累计");
}

#[test]
fn counted_rejection_finishes_after_restart_before_any_resend() {
    let root = tempfile::tempdir().unwrap();
    let mut spool = Spool::open(root.path().into(), 4096).unwrap();
    spool.push(&mut sample()).unwrap();
    let (path, bytes) = spool.first().unwrap().unwrap();
    spool
        .begin_removal(&path, &bytes, "expired_sample", true)
        .unwrap();
    assert!(path.exists());
    assert_eq!(spool.dropped(), 1);
    drop(spool);
    let mut reopened = Spool::open(root.path().into(), 4096).unwrap();
    assert!(!path.exists());
    assert!(reopened.first().unwrap().is_none());
    assert_eq!(reopened.diagnostics()["expired_batches"], 1);
    assert_eq!(reopened.dropped(), 1);
    let rejection: serde_json::Value = read_json(&root.path().join("rejected.json")).unwrap();
    assert_eq!(rejection.as_array().unwrap().len(), 1);
    assert!(reopened.diagnostics()["last_successful_ack_at"].is_null());
}
#[test]
fn quota_retry_and_restart_finish_same_eviction_once() {
    let root = tempfile::tempdir().unwrap();
    let mut spool = Spool::open(root.path().into(), 4096).unwrap();
    spool.push(&mut sample()).unwrap();
    let (path, _) = spool.first().unwrap().unwrap();
    spool.fail_removal = true;
    assert!(spool.discard().is_err());
    assert!(path.exists());
    assert_eq!(spool.diagnostics()["quota_eviction_batches"], 1);
    spool.fail_removal = true;
    assert!(spool.discard().is_err());
    assert_eq!(spool.diagnostics()["quota_eviction_batches"], 1);
    drop(spool);
    let reopened = Spool::open(root.path().into(), 4096).unwrap();
    assert!(!path.exists());
    assert_eq!(reopened.diagnostics()["quota_eviction_batches"], 1);
    assert_eq!(reopened.dropped(), 1);
}

#[test]
fn failed_reason_index_write_preserves_original_and_pending_count() {
    let root = tempfile::tempdir().unwrap();
    let mut spool = Spool::open(root.path().into(), 4096).unwrap();
    spool.push(&mut sample()).unwrap();
    let (path, bytes) = spool.first().unwrap().unwrap();
    fs::create_dir(root.path().join("rejected.json")).unwrap();
    assert!(spool.reject(&path, &bytes, "malformed").is_err());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(spool.dropped(), 1);
    fs::remove_dir(root.path().join("rejected.json")).unwrap();
    drop(spool);
    let reopened = Spool::open(root.path().into(), 4096).unwrap();
    assert!(!path.exists());
    assert_eq!(reopened.dropped(), 1);
}

#[test]
fn failed_stats_transaction_never_removes_or_counts_original() {
    let root = tempfile::tempdir().unwrap();
    let mut spool = Spool::open(root.path().into(), 4096).unwrap();
    spool.push(&mut sample()).unwrap();
    let (path, bytes) = spool.first().unwrap().unwrap();
    fs::create_dir(root.path().join("stats.json")).unwrap();
    assert!(spool.reject(&path, &bytes, "malformed").is_err());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(spool.dropped(), 0);
    fs::remove_dir(root.path().join("stats.json")).unwrap();
    spool.reject(&path, &bytes, "malformed").unwrap();
    assert!(!path.exists());
    assert_eq!(spool.dropped(), 1);
}

#[test]
fn fresh_pending_rejection_recovers_and_protected_sources_refuse_disposition() {
    let root = tempfile::tempdir().unwrap();
    let mut spool = Spool::open(root.path().into(), 4096).unwrap();
    let mut fresh: Batch = serde_json::from_slice(include_bytes!(
        "../../../emissary-protocol/tests/fixtures/telemetry-batch.json"
    ))
    .unwrap();
    fresh.inventory = None;
    fresh.processes.clear();
    spool.push(&mut fresh).unwrap();
    let (path, bytes) = spool.next().unwrap().unwrap();
    assert!(path.parent().unwrap().ends_with("fresh"));
    spool
        .begin_removal(&path, &bytes, "bounds_exceeded", true)
        .unwrap();
    drop(spool);
    let mut reopened = Spool::open(root.path().into(), 4096).unwrap();
    assert!(!path.exists());
    assert_eq!(reopened.dropped(), 1);
    reopened.push(&mut sample()).unwrap();
    let (protected, bytes) = reopened.first().unwrap().unwrap();
    reopened.inventory_blocked.insert(protected.clone());
    assert!(reopened.reject(&protected, &bytes, "malformed").is_err());
    assert!(protected.exists());
    assert_eq!(reopened.dropped(), 1);
    reopened.inventory_blocked.clear();
    reopened.opaque.insert(protected.clone());
    assert!(reopened.ack(&protected).is_err());
    assert!(protected.exists());
}
#[test]
fn directory_sync_failure_does_not_recount_or_restore_removed_record() {
    let root = tempfile::tempdir().unwrap();
    let mut spool = Spool::open(root.path().into(), 4096).unwrap();
    spool.push(&mut sample()).unwrap();
    let (path, bytes) = spool.first().unwrap().unwrap();
    spool.sync.fail_next = true;
    assert!(spool.reject(&path, &bytes, "malformed").is_err());
    assert!(!path.exists());
    assert_eq!(spool.dropped(), 1);
    spool.reject(&path, &bytes, "malformed").unwrap();
    assert_eq!(spool.diagnostics()["malformed_batches"], 1);
    drop(spool);
    let reopened = Spool::open(root.path().into(), 4096).unwrap();
    assert_eq!(reopened.dropped(), 1);
}
#[test]
fn pending_disposition_never_removes_changed_bytes_or_protected_source() {
    let root = tempfile::tempdir().unwrap();
    let mut spool = Spool::open(root.path().into(), 4096).unwrap();
    spool.push(&mut sample()).unwrap();
    let (path, bytes) = spool.first().unwrap().unwrap();
    spool
        .begin_removal(&path, &bytes, "id_content_conflict", true)
        .unwrap();
    let replacement = serde_json::to_vec(&sample()).unwrap();
    fs::write(&path, &replacement).unwrap();
    drop(spool);
    let mut reopened = Spool::open(root.path().into(), 4096).unwrap();
    assert!(reopened.first().is_err());
    assert_eq!(fs::read(&path).unwrap(), replacement);
    assert_eq!(reopened.dropped(), 1);
}
#[test]
fn rejection_index_stays_bounded_while_repeated_identity_is_not_recounted() {
    let root = tempfile::tempdir().unwrap();
    let mut spool = Spool::open(root.path().into(), 4096).unwrap();
    for _ in 0..70 {
        spool.push(&mut sample()).unwrap();
        let (path, bytes) = spool.first().unwrap().unwrap();
        spool.reject(&path, &bytes, "bounds_exceeded").unwrap();
    }
    let records: serde_json::Value = read_json(&root.path().join("rejected.json")).unwrap();
    assert_eq!(records.as_array().unwrap().len(), 64);
    assert!(
        fs::metadata(root.path().join("rejected.json"))
            .unwrap()
            .len()
            <= 64 << 10
    );
    assert_eq!(spool.dropped(), 70);
}

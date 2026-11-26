//! [INPUT]: 真实8192条记录上限、待发主机槽和固定发送槽。
//! [OUTPUT]: 验证等量合并保留历史，新增槽仅释放所需名额。
//! [POS]: Spool计数额度回归；不改变字节额度或发送顺序。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;

#[test]
fn replacing_pending_at_record_capacity_preserves_history_but_new_slot_evicts_one() {
    let directory = tempfile::tempdir().unwrap();
    let mut host: Batch = serde_json::from_slice(include_bytes!(
        "../../../emissary-protocol/tests/fixtures/telemetry-batch.json"
    ))
    .unwrap();
    host.inventory = None;
    host.processes.clear();
    let mut history = host.clone();
    history.host = None;
    let mut sources = Vec::new();
    for index in 0..MAX_RECORDS - 1 {
        history.id = uuid::Uuid::new_v4();
        let bytes = serde_json::to_vec(&history).unwrap();
        let path = directory
            .path()
            .join(format!("{index:020}-{}.batch", history.id));
        fs::write(&path, &bytes).unwrap();
        sources.push((path, bytes));
    }
    fs::create_dir(directory.path().join("fresh")).unwrap();
    fs::write(
        directory.path().join("fresh/pending.batch"),
        serde_json::to_vec(&host).unwrap(),
    )
    .unwrap();
    let mut spool = Spool::open(directory.path().into(), 27 << 20).unwrap();
    host.id = uuid::Uuid::new_v4();
    spool.push(&mut host).unwrap();
    assert_eq!(
        spool.records.len(),
        MAX_RECORDS - 1,
        "replacing pending does not add a record"
    );
    assert_eq!(spool.fresh.records().unwrap(), 1);
    assert_eq!(spool.stats.coalesced, 1);
    assert_eq!(spool.dropped(), 0);
    assert_eq!(spool.stats.reasons.get("quota_eviction"), None);
    for (path, bytes) in &sources {
        assert_eq!(fs::read(path).unwrap(), *bytes);
    }
    let (inflight, original) = spool.next().unwrap().unwrap();
    assert_eq!(
        serde_json::from_slice::<Batch>(&original).unwrap().id,
        host.id
    );
    host.id = uuid::Uuid::new_v4();
    spool.push(&mut host).unwrap();
    assert_eq!(spool.fresh.records().unwrap(), 2);
    assert_eq!(spool.records.len(), MAX_RECORDS - 2);
    assert_eq!(
        spool.dropped(),
        1,
        "new pending slot releases exactly one record"
    );
    assert_eq!(spool.stats.reasons.get("quota_eviction"), Some(&1));
    assert_eq!(fs::read(inflight).unwrap(), original);
    assert_eq!(sources.iter().filter(|(path, _)| !path.exists()).count(), 1);
    for (path, bytes) in sources.iter().filter(|(path, _)| path.exists()) {
        assert_eq!(fs::read(path).unwrap(), *bytes);
    }
}

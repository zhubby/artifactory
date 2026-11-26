//! [INPUT]: 真实清单文件、启动检查点和目录同步故障钩子。
//! [OUTPUT]: 验证精确冲突隔离、重开、写盘失败及有界索引。
//! [POS]: 清单队列持久冲突回归；不伪造业务ACK。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use artifactory_emissary_protocol::telemetry::Inventory;

fn seed(root: &std::path::Path) -> (InventoryQueue, Uuid, InventoryObservation) {
    let mut queue = InventoryQueue::open(root.into()).unwrap();
    let startup = Uuid::new_v4();
    queue
        .seed_startup(startup, chrono::Utc::now(), &Inventory::default())
        .unwrap();
    let observation = queue.first().unwrap().unwrap();
    (queue, startup, observation)
}

#[test]
fn conflict_preserves_source_and_checkpoint_across_reopen() {
    let root = tempfile::tempdir().unwrap();
    let (mut queue, startup, original) = seed(root.path());
    queue.refresh(startup, &Inventory::default(), 1).unwrap();
    let source = root
        .path()
        .join(format!("{}.inventory", original.observation_id));
    let bytes = fs::read(&source).unwrap();
    let checkpoint = fs::read(root.path().join("startup.json")).unwrap();
    queue.conflict(&original).unwrap();
    queue.conflict(&original).unwrap();
    assert_eq!(queue.stats.conflicts_total, 1);
    assert_eq!(queue.first().unwrap().unwrap().revision, 1);
    let mut other = original.clone();
    other.delivery_digest = "f".repeat(64);
    assert!(!queue.is_conflicted(&other));
    assert!(queue.conflict(&other).is_err());
    drop(queue);
    let mut queue = InventoryQueue::open(root.path().into()).unwrap();
    queue
        .seed_startup(startup, chrono::Utc::now(), &Inventory::default())
        .unwrap();
    assert_eq!(queue.first().unwrap().unwrap().revision, 1);
    assert_eq!(queue.conflicts(), 1);
    assert_eq!(queue.stats.conflicts_total, 1);
    assert_eq!(fs::read(source).unwrap(), bytes);
    assert_eq!(
        fs::read(root.path().join("startup.json")).unwrap(),
        checkpoint
    );
    // fallback也必须跳过冲突，且空选择不能删除原记录。
    queue.set_current(Uuid::new_v4());
    let refresh = queue.first().unwrap().unwrap();
    queue.conflict(&refresh).unwrap();
    assert!(queue.first().unwrap().is_none());
    assert!(queue.first_compatible(1).unwrap().is_none());
    assert_eq!(
        queue.waiting(1),
        0,
        "permanent conflicts do not await a larger schema budget"
    );
    assert_eq!(queue.len(), 2);
}

#[test]
fn conflict_write_failure_and_post_rename_sync_failure_never_double_count() {
    let root = tempfile::tempdir().unwrap();
    let (mut queue, _, observation) = seed(root.path());
    let stats = root.path().join("stats.json");
    fs::create_dir(&stats).unwrap();
    assert!(queue.conflict(&observation).is_err());
    assert_eq!(queue.conflicts(), 0);
    assert_eq!(queue.stats.conflicts_total, 0);
    assert_eq!(queue.first().unwrap(), Some(observation.clone()));
    fs::remove_dir(&stats).unwrap();
    queue.sync.fail_next = true;
    assert!(queue.conflict(&observation).is_err());
    assert_eq!(
        queue.conflicts(),
        1,
        "rename already installed the new index"
    );
    assert_eq!(queue.stats.conflicts_total, 1);
    let installed: Stats = serde_json::from_slice(&fs::read(&stats).unwrap()).unwrap();
    assert_eq!(installed.conflicts_total, 1);
    queue.conflict(&observation).unwrap();
    assert!(queue.first().unwrap().is_none());
    drop(queue);
    let mut queue = InventoryQueue::open(root.path().into()).unwrap();
    queue.conflict(&observation).unwrap();
    assert_eq!(queue.stats.conflicts_total, 1);
    assert_eq!(queue.conflicts(), 1);
}

#[test]
fn bounded_index_preserves_oldest_active_marker_and_rejects_corruption() {
    let root = tempfile::tempdir().unwrap();
    let (mut queue, startup, original) = seed(root.path());
    queue.conflict(&original).unwrap();
    for _ in 1..64 {
        queue.stats.conflicts.push(Conflict {
            observation_id: Uuid::new_v4().to_string(),
            delivery_digest: "a".repeat(64),
        });
    }
    queue.stats.conflicts_total = 64;
    queue.refresh(startup, &Inventory::default(), 1).unwrap();
    let refresh = queue.first().unwrap().unwrap();
    queue.conflict(&refresh).unwrap();
    assert!(
        queue.is_conflicted(&original),
        "active marker cannot be evicted"
    );
    assert_eq!(queue.stats.conflicts.len(), 64);
    assert_eq!(queue.stats.conflicts_total, 65);
    assert!(fs::metadata(root.path().join("stats.json")).unwrap().len() <= 64 << 10);
    drop(queue);
    let queue = InventoryQueue::open(root.path().into()).unwrap();
    assert_eq!(queue.conflicts(), 2);
    let mut bad = queue.stats.clone();
    bad.conflicts[1] = bad.conflicts[0].clone();
    drop(queue);
    atomic_json(&root.path().join("stats.json"), &bad).unwrap();
    assert!(InventoryQueue::open(root.path().into()).is_err());
    atomic_bytes(
        &root.path().join("stats.json"),
        &vec![b' '; (64 << 10) + 1],
        false,
    )
    .unwrap();
    assert!(InventoryQueue::open(root.path().into()).is_err());
}

//! [INPUT]: 真实私有目录、旧状态JSON和固定资产摘要。
//! [OUTPUT]: 验证空间边界、保护集合、未知文件与链接不被清扫。
//! [POS]: OTA存储和回退安全回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_emissary::{config::atomic_json, ota};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn asset(directory: &Path, value: u8) -> PathBuf {
    let bytes = [value; 32];
    let (os, arch) = artifactory_emissary::platform();
    let asset = ota::Asset::from_bytes(os, arch, &bytes);
    let path = directory
        .join("versions")
        .join(asset.sha256)
        .join(ota::binary_name());
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn space_budget_preserves_reserve_and_rejects_arithmetic_overflow() {
    assert_eq!(ota::required_space(100, 30).unwrap(), (64 << 20) + 70);
    assert!(ota::required_space(100, 101).is_err());
    assert!(ota::required_space(u64::MAX, 0).is_err());
}

#[test]
fn low_space_gc_never_deletes_current_previous_candidate_or_active_download() {
    let root = tempfile::tempdir().unwrap();
    let current = asset(root.path(), 1);
    let previous = asset(root.path(), 2);
    let candidate = asset(root.path(), 3);
    let active = asset(root.path(), 4);
    let removable = asset(root.path(), 5);
    let state = ota::State {
        current: ota::Image {
            path: current.to_string_lossy().into(),
            ..Default::default()
        },
        previous: ota::Image {
            path: previous.to_string_lossy().into(),
            ..Default::default()
        },
        candidate: ota::Image {
            path: candidate.to_string_lossy().into(),
            ..Default::default()
        },
        ..Default::default()
    };
    atomic_json(&root.path().join("state.json"), &state).unwrap();
    let report = ota::garbage_collect(
        root.path(),
        std::slice::from_ref(&active),
        ota::GcPressure::LowSpace,
    )
    .unwrap();
    assert_eq!(report.protected_assets, 4);
    assert_eq!(report.removed_assets, 1);
    for path in [current, previous, candidate, active] {
        assert!(path.exists());
    }
    assert!(!removable.exists());
}

#[test]
fn invalid_state_prevents_gc_instead_of_guessing_rollback_assets() {
    let root = tempfile::tempdir().unwrap();
    let target = asset(root.path(), 5);
    fs::write(root.path().join("state.json"), b"broken state").unwrap();
    assert!(ota::garbage_collect(root.path(), &[], ota::GcPressure::LowSpace).is_err());
    assert!(target.exists());
}

#[test]
fn current_asset_with_equivalent_path_is_still_protected() {
    let root = tempfile::tempdir().unwrap();
    let current = asset(root.path(), 9);
    let parent = current.parent().unwrap();
    let equivalent = parent
        .join("..")
        .join(parent.file_name().unwrap())
        .join(ota::binary_name());
    atomic_json(
        &root.path().join("state.json"),
        &ota::State {
            current: ota::Image {
                path: equivalent.to_string_lossy().into(),
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .unwrap();
    ota::garbage_collect(root.path(), &[], ota::GcPressure::LowSpace).unwrap();
    assert!(
        current.exists(),
        "canonical current identity must override path spelling"
    );
}

#[cfg(unix)]
#[test]
fn gc_never_follows_external_symlinks() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let sentinel = outside.path().join("precious");
    fs::write(&sentinel, b"keep").unwrap();
    fs::create_dir(root.path().join("versions")).unwrap();
    std::os::unix::fs::symlink(
        outside.path(),
        root.path().join("versions").join("a".repeat(64)),
    )
    .unwrap();
    let report = ota::garbage_collect(root.path(), &[], ota::GcPressure::LowSpace).unwrap();
    assert_eq!(report.skipped_unknown, 1);
    assert_eq!(fs::read(sentinel).unwrap(), b"keep");
}

#[cfg(unix)]
#[test]
fn dangling_storage_lock_symlink_cannot_create_a_file_outside_ota() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("must-not-be-created");
    std::os::unix::fs::symlink(&target, root.path().join("storage.lock")).unwrap();
    assert!(ota::garbage_collect(root.path(), &[], ota::GcPressure::Normal).is_err());
    assert!(!target.exists());
}

#[test]
fn legacy_state_is_still_readable_without_progress_fields() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("state.json"),
        br#"{"generation":7,"phase":"draining","current":{"version":"old","path":"old-binary"}}"#,
    )
    .unwrap();
    let mut state = ota::snapshot(root.path()).unwrap();
    assert!(state.recover());
    assert_eq!(state.phase, "failed");
    assert_eq!(state.current.version, "old");
    assert!(ota::progress_snapshot(root.path()).unwrap().is_none());
}

#[test]
fn unknown_trash_directory_is_not_deleted() {
    let root = tempfile::tempdir().unwrap();
    atomic_json(&root.path().join("state.json"), &ota::State::default()).unwrap();
    let unknown = root.path().join("trash").join("manual-backup");
    fs::create_dir_all(&unknown).unwrap();
    fs::write(unknown.join(ota::binary_name()), b"keep backup").unwrap();
    ota::garbage_collect(root.path(), &[], ota::GcPressure::Normal).unwrap();
    assert!(unknown.exists());
}

#[test]
fn trash_digest_name_does_not_authorize_deleting_unverified_content() {
    let root = tempfile::tempdir().unwrap();
    atomic_json(&root.path().join("state.json"), &ota::State::default()).unwrap();
    let unknown = root.path().join("trash").join("a".repeat(64));
    fs::create_dir_all(&unknown).unwrap();
    fs::write(unknown.join(ota::binary_name()), b"unverified backup").unwrap();
    let report = ota::garbage_collect(root.path(), &[], ota::GcPressure::LowSpace).unwrap();
    assert!(unknown.exists());
    assert_eq!(report.skipped_unknown, 1);
}

#[test]
fn durable_active_download_marker_protects_only_the_current_generation() {
    let root = tempfile::tempdir().unwrap();
    let active = asset(root.path(), 7);
    let directory = active.parent().unwrap();
    atomic_json(
        &root.path().join("state.json"),
        &ota::State {
            update_id: "active-update".into(),
            generation: 9,
            phase: "downloading".into(),
            ..Default::default()
        },
    )
    .unwrap();
    for generation in [9, 8] {
        atomic_json(
            &root.path().join("active-download.json"),
            &serde_json::json!({
                "update_id": "active-update",
                "generation": generation,
                "path": directory,
            }),
        )
        .unwrap();
        ota::garbage_collect(root.path(), &[], ota::GcPressure::LowSpace).unwrap();
        assert_eq!(active.exists(), generation == 9);
    }
}

#[test]
fn missing_state_prevents_cache_deletion() {
    let root = tempfile::tempdir().unwrap();
    let current = asset(root.path(), 8);
    ota::garbage_collect(root.path(), &[], ota::GcPressure::LowSpace).unwrap();
    assert!(current.exists());
}

#[test]
fn normal_gc_keeps_only_two_verified_unreferenced_assets() {
    let root = tempfile::tempdir().unwrap();
    atomic_json(&root.path().join("state.json"), &ota::State::default()).unwrap();
    let paths = (1..=4)
        .map(|value| asset(root.path(), value))
        .collect::<Vec<_>>();
    let report = ota::garbage_collect(root.path(), &[], ota::GcPressure::Normal).unwrap();
    assert_eq!(report.removed_assets, 2);
    assert_eq!(paths.iter().filter(|p| p.exists()).count(), 2);
}

#[test]
fn normal_gc_respects_last_use_instead_of_the_original_download_time() {
    use std::time::{Duration, SystemTime};
    let root = tempfile::tempdir().unwrap();
    atomic_json(&root.path().join("state.json"), &ota::State::default()).unwrap();
    let paths = (1..=4)
        .map(|value| {
            let path = asset(root.path(), value);
            fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_times(
                    fs::FileTimes::new()
                        .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(value as u64)),
                )
                .unwrap();
            path
        })
        .collect::<Vec<_>>();
    let first = paths[0].parent().unwrap();
    atomic_json(
        &first.join("last-use.json"),
        &serde_json::json!({
            "schema_version": 1,
            "asset_sha256": first.file_name().unwrap().to_str().unwrap(),
            "used_at": SystemTime::UNIX_EPOCH + Duration::from_secs(5),
        }),
    )
    .unwrap();
    ota::garbage_collect(root.path(), &[], ota::GcPressure::Normal).unwrap();
    assert!(paths[0].exists());
    assert!(paths[3].exists());
    assert!(!paths[1].exists());
    assert!(!paths[2].exists());
}

//! [INPUT]: 依赖遥测私有目录和队列已确认的损坏路径。
//! [OUTPUT]: 只读观测正文、临时文件、索引和物理分配块。
//! [POS]: 后台磁盘诊断边界；心跳只读取调用方缓存。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use cap_fs_ext::DirExt;
use cap_std::fs::{Dir, Metadata};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

// 8192条历史加独立清单、新鲜槽和保留文件的扫描余量。
const MAX_ENTRIES: usize = 16_384;
const MAX_DEPTH: usize = 8;

#[derive(Clone, Copy)]
enum ScanError {
    Failed,
    Unsafe,
    Limit,
}
impl ScanError {
    fn state(self) -> &'static str {
        match self {
            Self::Failed => "scan_failed",
            Self::Unsafe => "unsafe_entry",
            Self::Limit => "scan_budget_exceeded",
        }
    }
}
type ScanResult<T> = Result<T, ScanError>;

#[derive(Default)]
struct Totals {
    files: u64,
    file_bytes: u64,
    record_bytes: u64,
    temporary_bytes: u64,
    index_bytes: u64,
    corrupt_bytes: u64,
    unknown_bytes: u64,
    allocated_bytes: u64,
    block_overhead_bytes: u64,
    #[cfg(unix)]
    allocations: HashSet<(u64, u64)>,
    entries: usize,
}

fn add(total: &mut u64, value: u64) -> ScanResult<()> {
    *total = total.checked_add(value).ok_or(ScanError::Limit)?;
    Ok(())
}

fn unsafe_metadata(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use cap_std::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    metadata.file_type().is_symlink() || !(metadata.is_file() || metadata.is_dir())
}

fn allocation(metadata: &Metadata, totals: &mut Totals) -> ScanResult<()> {
    #[cfg(unix)]
    {
        use cap_std::fs::MetadataExt;
        if totals.allocations.insert((metadata.dev(), metadata.ino())) {
            let allocated = metadata.blocks().checked_mul(512).ok_or(ScanError::Limit)?;
            add(&mut totals.allocated_bytes, allocated)?;
            let payload = if metadata.is_file() {
                metadata.len()
            } else {
                0
            };
            add(
                &mut totals.block_overhead_bytes,
                allocated.saturating_sub(payload),
            )?;
        }
    }
    #[cfg(not(unix))]
    let _ = (metadata, totals);
    Ok(())
}

fn classify(
    relative: &Path,
    known_corrupt: bool,
    bytes: u64,
    totals: &mut Totals,
) -> ScanResult<()> {
    let name = relative.file_name().and_then(|name| name.to_str());
    if known_corrupt
        || relative
            .components()
            .any(|part| part.as_os_str() == "opaque")
    {
        add(&mut totals.corrupt_bytes, bytes)
    } else if name.is_some_and(|name| name.starts_with(".tmp")) {
        add(&mut totals.temporary_bytes, bytes)
    } else if relative == Path::new("stats.json")
        || relative == Path::new("rejected.json")
        || relative == Path::new("lock")
        || relative == Path::new("inventories/stats.json")
        || relative == Path::new("inventories/startup.json")
        || relative == Path::new("inventories/revision.json")
    {
        add(&mut totals.index_bytes, bytes)
    } else if relative
        .extension()
        .is_some_and(|extension| extension == "batch" || extension == "inventory")
    {
        add(&mut totals.record_bytes, bytes)
    } else {
        add(&mut totals.unknown_bytes, bytes)
    }
}

fn scan(
    dir: &Dir,
    root: &Path,
    relative: &Path,
    known_corrupt: &HashSet<PathBuf>,
    totals: &mut Totals,
    limit: usize,
) -> ScanResult<()> {
    if relative.components().count() > MAX_DEPTH {
        return Err(ScanError::Limit);
    }
    allocation(&dir.dir_metadata().map_err(|_| ScanError::Failed)?, totals)?;
    for entry in dir.entries().map_err(|_| ScanError::Failed)? {
        let entry = entry.map_err(|_| ScanError::Failed)?;
        totals.entries += 1;
        if totals.entries > limit {
            return Err(ScanError::Limit);
        }
        let name = entry.file_name();
        let child = relative.join(&name);
        let metadata = dir.symlink_metadata(&name).map_err(|_| ScanError::Failed)?;
        if unsafe_metadata(&metadata) {
            return Err(ScanError::Unsafe);
        }
        if metadata.is_dir() {
            let next = dir
                .open_dir_nofollow(&name)
                .map_err(|_| ScanError::Failed)?;
            scan(&next, root, &child, known_corrupt, totals, limit)?;
        } else {
            allocation(&metadata, totals)?;
            add(&mut totals.files, 1)?;
            add(&mut totals.file_bytes, metadata.len())?;
            classify(
                &child,
                known_corrupt.contains(&root.join(&child)),
                metadata.len(),
                totals,
            )?;
        }
    }
    Ok(())
}

fn observe(root: &Path, known_corrupt: &HashSet<PathBuf>, limit: usize) -> ScanResult<Totals> {
    let parent = root.parent().ok_or(ScanError::Unsafe)?;
    let name = root.file_name().ok_or(ScanError::Unsafe)?;
    let parent = Dir::open_ambient_dir(parent, cap_std::ambient_authority())
        .map_err(|_| ScanError::Failed)?;
    let metadata = parent
        .symlink_metadata(name)
        .map_err(|_| ScanError::Failed)?;
    if unsafe_metadata(&metadata) || !metadata.is_dir() {
        return Err(ScanError::Unsafe);
    }
    let dir = parent
        .open_dir_nofollow(name)
        .map_err(|_| ScanError::Failed)?;
    let mut totals = Totals::default();
    scan(&dir, root, Path::new(""), known_corrupt, &mut totals, limit)?;
    Ok(totals)
}

fn with_limit(root: &Path, known_corrupt: &HashSet<PathBuf>, limit: usize) -> Value {
    let result = observe(root, known_corrupt, limit);
    let (state, totals) = match result {
        Ok(totals) => ("ready", Some(totals)),
        Err(error) => (error.state(), None),
    };
    let allocated = if cfg!(unix) {
        totals.as_ref().map(|totals| totals.allocated_bytes)
    } else {
        None
    };
    json!({
        "disk_state": state,
        "disk_observed_at": chrono::Utc::now().to_rfc3339(),
        "disk_file_bytes": totals.as_ref().map(|totals| totals.file_bytes),
        "disk_record_bytes": totals.as_ref().map(|totals| totals.record_bytes),
        "disk_allocated_bytes": allocated,
        "disk_block_overhead_bytes": allocated.and(totals.as_ref().map(|totals| totals.block_overhead_bytes)),
        "disk_temporary_bytes": totals.as_ref().map(|totals| totals.temporary_bytes),
        "disk_index_bytes": totals.as_ref().map(|totals| totals.index_bytes),
        "disk_corrupt_bytes": totals.as_ref().map(|totals| totals.corrupt_bytes),
        "disk_unknown_bytes": totals.as_ref().map(|totals| totals.unknown_bytes),
        "disk_files": totals.as_ref().map(|totals| totals.files)
    })
}

/// 扫描完整目录后才发布总量，不把逻辑额度当作物理上限。
/// 已知损坏路径使用原始绝对路径；本函数不解码或修改正文。
pub(super) fn snapshot(root: &Path, known_corrupt: &HashSet<PathBuf>) -> Value {
    with_limit(root, known_corrupt, MAX_ENTRIES)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn counts_files_temporary_indexes_and_known_corrupt_without_reading_payloads() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fs::create_dir(root.join("inventories")).unwrap();
        fs::create_dir(root.join("fresh")).unwrap();
        fs::create_dir(root.join("fresh/opaque")).unwrap();
        for (path, bytes) in [
            ("old.batch", 3),
            ("stats.json", 5),
            ("inventories/startup.json", 7),
            ("inventories/observation.inventory", 2),
            ("fresh/pending.batch", 4),
            (".tmp-residue", 6),
            ("fresh/opaque/pending.batch", 8),
            ("damaged.batch", 2),
            ("unknown", 1),
        ] {
            fs::write(root.join(path), vec![b'x'; bytes]).unwrap();
        }
        let known = HashSet::from([root.join("damaged.batch")]);
        let observed = snapshot(root, &known);
        assert_eq!(observed["disk_state"], "ready");
        assert_eq!(observed["disk_file_bytes"], 38);
        assert_eq!(observed["disk_record_bytes"], 9);
        assert_eq!(observed["disk_temporary_bytes"], 6);
        assert_eq!(observed["disk_index_bytes"], 12);
        assert_eq!(observed["disk_corrupt_bytes"], 10);
        assert_eq!(observed["disk_unknown_bytes"], 1);
        assert_eq!(observed["disk_files"], 9);
        assert!(observed["disk_observed_at"].as_str().is_some());
        assert_eq!(
            fs::read(root.join("fresh/opaque/pending.batch")).unwrap(),
            b"xxxxxxxx"
        );
    }

    #[cfg(unix)]
    #[test]
    fn reports_actual_blocks_and_counts_a_hard_link_allocation_once() {
        use std::os::unix::fs::MetadataExt;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fs::write(root.join("one.batch"), b"x").unwrap();
        fs::hard_link(root.join("one.batch"), root.join("two.batch")).unwrap();
        let allocated = (fs::metadata(root).unwrap().blocks()
            + fs::metadata(root.join("one.batch")).unwrap().blocks())
            * 512;
        let observed = snapshot(root, &HashSet::new());
        assert_eq!(observed["disk_allocated_bytes"], allocated);
        assert_eq!(observed["disk_file_bytes"], 2);
        assert_eq!(
            observed["disk_block_overhead_bytes"],
            allocated.saturating_sub(1)
        );
    }

    #[cfg(unix)]
    #[test]
    fn sparse_file_savings_do_not_hide_small_file_block_overhead() {
        use std::os::unix::fs::MetadataExt;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fs::write(root.join("one.batch"), b"x").unwrap();
        fs::File::create(root.join("sparse.batch"))
            .unwrap()
            .set_len(1 << 20)
            .unwrap();
        let small = fs::metadata(root.join("one.batch")).unwrap().blocks() * 512;
        let sparse = fs::metadata(root.join("sparse.batch")).unwrap().blocks() * 512;
        let overhead = fs::metadata(root).unwrap().blocks() * 512
            + small.saturating_sub(1)
            + sparse.saturating_sub(1 << 20);
        let observed = snapshot(root, &HashSet::new());
        assert_eq!(observed["disk_block_overhead_bytes"], overhead);
    }

    #[test]
    fn scan_budget_never_publishes_partial_totals() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("one.batch"), b"one").unwrap();
        fs::write(directory.path().join("two.batch"), b"two").unwrap();
        let partial = with_limit(directory.path(), &HashSet::new(), 1);
        assert_eq!(partial["disk_state"], "scan_budget_exceeded");
        assert!(partial["disk_file_bytes"].is_null());
        assert!(partial["disk_files"].is_null());
        let complete = with_limit(directory.path(), &HashSet::new(), 2);
        assert_eq!(complete["disk_state"], "ready");
        assert_eq!(complete["disk_file_bytes"], 6);
    }

    #[test]
    fn deep_tree_stops_instead_of_scanning_without_bound() {
        let directory = tempfile::tempdir().unwrap();
        let mut path = directory.path().to_path_buf();
        for _ in 0..9 {
            path.push("nested");
            fs::create_dir(&path).unwrap();
        }
        fs::write(path.join("unseen.batch"), b"x").unwrap();
        let observed = snapshot(directory.path(), &HashSet::new());
        assert_eq!(observed["disk_state"], "scan_budget_exceeded");
        assert!(observed["disk_file_bytes"].is_null());
    }

    #[test]
    fn actual_atomic_temporary_file_is_separate_from_persisted_records() {
        use std::io::Write;
        let directory = tempfile::tempdir().unwrap();
        let mut pending = tempfile::NamedTempFile::new_in(directory.path()).unwrap();
        pending.write_all(b"pending").unwrap();
        let observed = snapshot(directory.path(), &HashSet::new());
        assert_eq!(observed["disk_file_bytes"], 7);
        assert_eq!(observed["disk_temporary_bytes"], 7);
        assert_eq!(observed["disk_record_bytes"], 0);
    }

    #[test]
    fn failed_scan_keeps_totals_unknown_instead_of_zero() {
        let directory = tempfile::tempdir().unwrap();
        let absent = snapshot(&directory.path().join("absent"), &HashSet::new());
        assert_eq!(absent["disk_state"], "scan_failed");
        for key in [
            "disk_file_bytes",
            "disk_allocated_bytes",
            "disk_temporary_bytes",
            "disk_index_bytes",
            "disk_corrupt_bytes",
            "disk_files",
        ] {
            assert!(absent[key].is_null(), "{key} must remain unknown");
        }
        fs::write(directory.path().join("file"), b"x").unwrap();
        assert_eq!(
            snapshot(&directory.path().join("file"), &HashSet::new())["disk_state"],
            "unsafe_entry"
        );
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_child_directory_invalidates_all_totals() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let child = directory.path().join("unreadable");
        fs::create_dir(&child).unwrap();
        fs::write(child.join("hidden.batch"), b"not zero").unwrap();
        let permissions = fs::metadata(&child).unwrap().permissions();
        fs::set_permissions(&child, fs::Permissions::from_mode(0o0)).unwrap();
        if fs::read_dir(&child).is_ok() {
            // root或额外ACL仍允许读取时，不能构造权限失败。
            fs::set_permissions(&child, permissions).unwrap();
            return;
        }
        let observed = snapshot(directory.path(), &HashSet::new());
        fs::set_permissions(&child, permissions).unwrap();
        assert_eq!(observed["disk_state"], "scan_failed");
        assert!(observed["disk_file_bytes"].is_null());
        assert!(observed["disk_index_bytes"].is_null());
        assert!(observed["disk_allocated_bytes"].is_null());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_directory_and_file_symlinks_without_traversing_outside() {
        use std::os::unix::fs::symlink;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("root");
        let outside = directory.path().join("outside");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("secret.batch"), b"do not read").unwrap();
        symlink(&outside, root.join("escape")).unwrap();
        let observed = snapshot(&root, &HashSet::new());
        assert_eq!(observed["disk_state"], "unsafe_entry");
        assert!(observed["disk_file_bytes"].is_null());
        fs::remove_file(root.join("escape")).unwrap();
        symlink(outside.join("missing"), root.join("dangling.batch")).unwrap();
        assert_eq!(
            snapshot(&root, &HashSet::new())["disk_state"],
            "unsafe_entry"
        );
        symlink(&root, directory.path().join("root-link")).unwrap();
        assert_eq!(
            snapshot(&directory.path().join("root-link"), &HashSet::new())["disk_state"],
            "unsafe_entry"
        );
    }
}

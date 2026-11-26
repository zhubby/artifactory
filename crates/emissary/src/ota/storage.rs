//! [INPUT]: OTA根目录、持久current/previous/candidate与文件系统余量。
//! [OUTPUT]: 提供存储/请求交接锁、空间预算和受保护资产清扫。
//! [POS]: OTA资产存储边界，不删除未知路径或回滚资产。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::{State, binary_name, snapshot};
use crate::config::{atomic_json, private_dir, read_json};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

pub fn required_space(asset_size: u64, written: u64) -> Result<u64> {
    let remaining = asset_size
        .checked_sub(written)
        .context("written size exceeds asset")?;
    let tenth = asset_size.checked_add(9).context("OTA space overflow")? / 10;
    remaining
        .checked_add(tenth.max(64 << 20))
        .context("OTA space overflow")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GcPressure {
    Normal,
    LowSpace,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GcReport {
    pub removed_assets: u64,
    pub removed_bytes: u64,
    pub protected_assets: u64,
    pub skipped_unknown: u64,
    pub delete_failures: u64,
}

pub(super) struct Guard(fs::File);
impl Drop for Guard {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.0);
    }
}
pub(super) fn lock(directory: &Path) -> Result<Guard> {
    named_lock(directory, "storage.lock")
}
pub(super) fn request_lock(directory: &Path) -> Result<Guard> {
    named_lock(directory, "request.lock")
}
fn named_lock(directory: &Path, name: &str) -> Result<Guard> {
    safe_dir(directory)?;
    let path = directory.join(name);
    match fs::symlink_metadata(&path) {
        Ok(_) => ensure_regular(&path)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)?;
    fs2::FileExt::try_lock_exclusive(&file).context("OTA local record busy")?;
    Ok(Guard(file))
}
pub(super) fn safe_dir(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => ensure!(plain_directory(path)?, "unsafe OTA directory"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => private_dir(path)?,
        Err(error) => return Err(error.into()),
    }
    Ok(())
}
fn reparse(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        let _ = metadata;
        false
    }
}
fn plain_directory(path: &Path) -> Result<bool> {
    let metadata = fs::symlink_metadata(path)?;
    Ok(metadata.is_dir() && !metadata.file_type().is_symlink() && !reparse(&metadata))
}
pub(super) fn ensure_regular(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink() && !reparse(&metadata),
        "unsafe OTA asset file"
    );
    Ok(())
}
pub(super) fn sync(directory: &Path) -> Result<()> {
    #[cfg(unix)]
    fs::File::open(directory)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = directory;
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct ActiveDownload {
    update_id: String,
    generation: i64,
    path: PathBuf,
}
pub(super) fn protect_download(
    directory: &Path,
    request: &super::Request,
    path: &Path,
) -> Result<()> {
    atomic_json(
        &directory.join("active-download.json"),
        &ActiveDownload {
            update_id: request.id.clone(),
            generation: request.generation,
            path: path.into(),
        },
    )
}
pub(super) fn release_download(directory: &Path) -> Result<()> {
    let path = directory.join("active-download.json");
    if path.try_exists()? {
        fs::remove_file(path)?;
        sync(directory)?;
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct LastUse {
    schema_version: u32,
    asset_sha256: String,
    used_at: SystemTime,
}
pub(super) fn record_use(directory: &Path) -> Result<()> {
    atomic_json(
        &directory.join("last-use.json"),
        &LastUse {
            schema_version: 1,
            asset_sha256: directory
                .file_name()
                .and_then(|v| v.to_str())
                .context("asset digest missing")?
                .into(),
            used_at: SystemTime::now(),
        },
    )
}
fn last_use(path: &Path, digest: &str) -> Result<SystemTime> {
    ensure!(
        fs::metadata(path)?.len() <= 4096,
        "oversized OTA use record"
    );
    let value: LastUse = read_json(path)?;
    ensure!(
        value.schema_version == 1 && value.asset_sha256 == digest,
        "invalid OTA use record"
    );
    Ok(value.used_at)
}

pub fn garbage_collect(
    directory: &Path,
    active: &[PathBuf],
    pressure: GcPressure,
) -> Result<GcReport> {
    let _guard = lock(directory)?;
    collect_locked(directory, active, pressure)
}
pub(super) fn collect_locked(
    directory: &Path,
    active: &[PathBuf],
    pressure: GcPressure,
) -> Result<GcReport> {
    let versions = directory.join("versions");
    safe_dir(&versions)?;
    let versions = fs::canonicalize(versions)?;
    let trash = directory.join("trash");
    safe_dir(&trash)?;
    // 状态缺失或损坏时不能猜测当前/回滚资产。
    let has_state = directory.join("state.json").try_exists()?;
    let state = if has_state {
        snapshot(directory)?
    } else {
        State::default()
    };
    let mut protected = HashSet::new();
    for path in [
        &state.current.path,
        &state.previous.path,
        &state.candidate.path,
    ] {
        if !path.is_empty() {
            protected.insert(fs::canonicalize(path).context("protected OTA asset missing")?);
        }
    }
    for path in active {
        protected.insert(fs::canonicalize(path).context("active OTA asset missing")?);
    }
    let active_marker = directory.join("active-download.json");
    if active_marker.try_exists()? {
        let download: ActiveDownload = read_json(&active_marker)?;
        if download.update_id == state.update_id
            && download.generation == state.generation
            && matches!(
                state.phase.as_str(),
                "downloading" | "draining" | "activating"
            )
        {
            protected
                .insert(fs::canonicalize(download.path).context("active OTA download missing")?);
        }
    }
    let mut report = GcReport::default();
    let mut candidates = Vec::new();
    for entry in fs::read_dir(&versions)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.len() != 64
            || !name
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || !plain_directory(&path)?
        {
            report.skipped_unknown += 1;
            continue;
        }
        if protected
            .iter()
            .any(|p| p == &path || p.parent() == Some(path.as_path()))
        {
            report.protected_assets += 1;
            continue;
        }
        if !has_state {
            report.skipped_unknown += 1;
            continue;
        }
        let entries = fs::read_dir(&path)?.collect::<std::io::Result<Vec<_>>>()?;
        let mut bytes = 0u64;
        let mut modified = SystemTime::UNIX_EPOCH;
        let mut complete = false;
        let mut known = true;
        let mut used_at = None;
        for child in &entries {
            let child_path = child.path();
            let name = child.file_name();
            let name = name.to_string_lossy();
            if ensure_regular(&child_path).is_err()
                || (name != binary_name()
                    && name != "last-use.json"
                    && !(name.starts_with("download-") && name.ends_with(".part")))
            {
                known = false;
                break;
            }
            let metadata = fs::metadata(&child_path)?;
            bytes = bytes
                .checked_add(metadata.len())
                .context("OTA cache size overflow")?;
            modified = modified.max(metadata.modified()?);
            if name == "last-use.json" {
                match last_use(&child_path, &entry.file_name().to_string_lossy()) {
                    Ok(time) => used_at = Some(time),
                    Err(_) => {
                        known = false;
                        break;
                    }
                }
            }
            if name == binary_name() {
                if metadata.len() > super::MAX_BINARY_SIZE
                    || !matches_directory_digest(
                        &child_path,
                        path.file_name()
                            .and_then(|v| v.to_str())
                            .unwrap_or_default(),
                    )?
                {
                    known = false;
                    break;
                }
                complete = true;
            }
        }
        if !known {
            report.skipped_unknown += 1;
            continue;
        }
        candidates.push((path, bytes, used_at.unwrap_or(modified), complete));
    }
    candidates.sort_by_key(|v| std::cmp::Reverse(v.2));
    let mut cached_bytes = 0u64;
    let mut complete_kept = 0;
    for (path, bytes, modified, complete) in candidates {
        let young = SystemTime::now()
            .duration_since(modified)
            .unwrap_or_default()
            < Duration::from_secs(86400);
        let keep = pressure == GcPressure::Normal
            && if complete {
                complete_kept < 2 && cached_bytes.saturating_add(bytes) <= 512 << 20
            } else {
                young && cached_bytes.saturating_add(bytes) <= 512 << 20
            };
        if keep {
            cached_bytes = cached_bytes.saturating_add(bytes);
            if complete {
                complete_kept += 1;
            }
            continue;
        }
        // 受同一锁保护的状态在扫描期间不变，迁移备份位于versions之外。
        let moved = trash.join(path.file_name().context("asset directory missing name")?);
        if moved.try_exists()? {
            report.delete_failures += 1;
            continue;
        }
        if fs::rename(&path, &moved).is_err() {
            report.delete_failures += 1;
            continue;
        }
        sync(&versions)?;
        sync(&trash)?;
        if remove_known_directory(&moved).is_ok() {
            report.removed_assets += 1;
            report.removed_bytes += bytes;
        } else {
            report.delete_failures += 1;
        }
    }
    // 中断后的trash只允许删除受管普通文件，禁止递归跟随未知目录。
    for entry in fs::read_dir(&trash)? {
        let path = entry?.path();
        if !has_state {
            report.skipped_unknown += 1;
            continue;
        }
        if !path
            .file_name()
            .and_then(|v| v.to_str())
            .is_some_and(|name| {
                name.len() == 64
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
        {
            report.skipped_unknown += 1;
            continue;
        }
        let version_path = versions.join(path.file_name().context("trash name missing")?);
        if protected
            .iter()
            .any(|p| p == &version_path || p.parent() == Some(version_path.as_path()))
        {
            continue;
        }
        if remove_known_directory(&path).is_err() {
            report.skipped_unknown += 1;
        }
    }
    Ok(report)
}
fn remove_known_directory(path: &Path) -> Result<()> {
    ensure!(plain_directory(path)?, "unsafe OTA trash directory");
    let entries = fs::read_dir(path)?.collect::<std::io::Result<Vec<_>>>()?;
    for entry in &entries {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        ensure!(
            name == binary_name()
                || name == "last-use.json"
                || (name.starts_with("download-") && name.ends_with(".part")),
            "unknown OTA trash file"
        );
        ensure_regular(&entry.path())?;
        if name == "last-use.json" {
            last_use(
                &entry.path(),
                path.file_name()
                    .and_then(|v| v.to_str())
                    .unwrap_or_default(),
            )?;
        }
        if name == binary_name() {
            ensure!(
                entry.metadata()?.len() <= super::MAX_BINARY_SIZE
                    && matches_directory_digest(
                        &entry.path(),
                        path.file_name()
                            .and_then(|v| v.to_str())
                            .unwrap_or_default(),
                    )?,
                "unverified OTA trash asset"
            );
        }
    }
    for entry in entries {
        fs::remove_file(entry.path())?;
    }
    fs::remove_dir(path)?;
    sync(path.parent().context("trash parent missing")?)
}
fn matches_directory_digest(path: &Path, expected: &str) -> Result<bool> {
    let mut file = fs::File::open(path)?;
    let mut bytes = [0; 65536];
    let mut digest = Sha256::new();
    loop {
        let n = file.read(&mut bytes)?;
        if n == 0 {
            break;
        }
        digest.update(&bytes[..n]);
    }
    Ok(hex::encode(digest.finalize()) == expected)
}

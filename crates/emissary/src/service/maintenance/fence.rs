//! [INPUT]: 既有OTA目录身份、维护nonce与备份位置。
//! [OUTPUT]: 在私有权限就绪后原子发布运行时可读的维护围栏。
//! [POS]: 管理员维护与普通服务账户之间的文件发布边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::ota::activity::MaintenanceFence;
use anyhow::{Context, Result, ensure};
use std::{fs, io::Write, path::Path};

pub(super) fn publish(directory: &Path, value: &MaintenanceFence) -> Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    publish_with(directory, value, |file, parent| {
        // 身份和0600必须先于rename；普通服务从不读取管理员私有文件。
        std::os::unix::fs::fchown(file, Some(parent.uid()), Some(parent.gid()))
            .context("maintenance fence service ownership failed")?;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        Ok(())
    })
}

fn publish_with(
    directory: &Path,
    value: &MaintenanceFence,
    ownership: impl FnOnce(&fs::File, &fs::Metadata) -> Result<()>,
) -> Result<()> {
    let metadata = fs::symlink_metadata(directory)?;
    ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "unsafe maintenance fence directory"
    );
    let mut file = tempfile::NamedTempFile::new_in(directory)?;
    ownership(file.as_file(), &metadata)?;
    file.write_all(&serde_json::to_vec(value)?)?;
    file.as_file().sync_all()?;
    file.persist(directory.join("maintenance.json"))
        .map_err(|error| error.error)?;
    fs::File::open(directory)?
        .sync_all()
        .context("maintenance fence directory sync failed")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::read_json, ota::activity};

    #[test]
    fn denied_service_ownership_keeps_old_fence_unpublished() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("maintenance.json");
        fs::write(&path, b"original fence").unwrap();
        let next = MaintenanceFence {
            nonce: uuid::Uuid::new_v4(),
            backup: root.path().join("backup"),
        };
        let result = publish_with(root.path(), &next, |_, _| {
            anyhow::bail!("service ownership unavailable");
        });
        assert!(result.is_err());
        assert!(fs::read(&path).unwrap() == b"original fence");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn published_fence_uses_directory_identity_and_private_mode() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let root = tempfile::tempdir().unwrap();
        let nonce = uuid::Uuid::new_v4();
        publish(
            root.path(),
            &MaintenanceFence {
                nonce,
                backup: root.path().join("backup"),
            },
        )
        .unwrap();
        let parent = fs::metadata(root.path()).unwrap();
        let published = fs::metadata(root.path().join("maintenance.json")).unwrap();
        assert_eq!(
            (published.uid(), published.gid()),
            (parent.uid(), parent.gid())
        );
        assert_eq!(published.permissions().mode() & 0o7777, 0o600);
        assert_eq!(
            activity::maintenance_nonce(root.path()).unwrap(),
            Some(nonce)
        );
        let saved: MaintenanceFence = read_json(&root.path().join("maintenance.json")).unwrap();
        assert_eq!(saved.nonce, nonce);
    }

    #[cfg(unix)]
    #[test]
    fn root_published_fence_is_readable_by_existing_unprivileged_identity() {
        use std::os::unix::{
            fs::{MetadataExt, PermissionsExt},
            process::CommandExt,
        };
        let uid = std::process::Command::new("id").arg("-u").output().unwrap();
        if uid.stdout != b"0\n" {
            eprintln!("root-to-service read proof skipped: current process is not root");
            return;
        }
        let account = std::process::Command::new("id")
            .args(["-u", "nobody"])
            .output()
            .unwrap();
        if !account.status.success() {
            eprintln!("root-to-service read proof skipped: no existing nobody identity");
            return;
        }
        let service_uid: u32 = String::from_utf8(account.stdout)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let group = std::process::Command::new("id")
            .args(["-g", "nobody"])
            .output()
            .unwrap();
        assert!(group.status.success());
        let service_gid: u32 = String::from_utf8(group.stdout)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_ne!(service_uid, 0);
        let root = tempfile::tempdir().unwrap();
        std::os::unix::fs::chown(root.path(), Some(service_uid), Some(service_gid)).unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let nonce = uuid::Uuid::new_v4();
        publish(
            root.path(),
            &MaintenanceFence {
                nonce,
                backup: root.path().join("backup"),
            },
        )
        .unwrap();
        assert_eq!(
            fs::metadata(root.path().join("maintenance.json"))
                .unwrap()
                .uid(),
            service_uid
        );
        let output = std::process::Command::new("cat")
            .arg(root.path().join("maintenance.json"))
            .gid(service_gid)
            .uid(service_uid)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "unprivileged identity cannot read maintenance fence"
        );
        let fence: MaintenanceFence = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(fence.nonce, nonce);
    }
}

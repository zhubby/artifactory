//! [INPUT]: 依赖已回收 Worker 的受管目录和本机文件系统。
//! [OUTPUT]: 删除任务目录；Windows 访问受限时仅摘除目录链接，不修改 ACL。
//! [POS]: 源码生命周期的删除边界；链接目标始终由标准库保护。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use anyhow::{Context, Result};
use std::{fs, path::Path};

pub(crate) fn remove_dir_all(path: &Path) -> Result<()> {
    let result = fs::remove_dir_all(path);
    #[cfg(windows)]
    let result = match result {
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            remove_directory_links(path)?;
            match fs::remove_dir_all(path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                result => result,
            }
        }
        result => result,
    };
    result.with_context(|| format!("remove managed directory {}", path.display()))
}

#[cfg(windows)]
fn remove_directory_links(path: &Path) -> Result<()> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    // 缓存链接拒绝 LIST_DIRECTORY 和 SYNCHRONIZE；识别链接只需读属性。
    // 实际目录的句柄禁止删除共享，递归期间祖先目录不能被换成链接。
    let directory = fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .with_context(|| format!("inspect managed directory {}", path.display()))?;
    let attributes = directory.metadata()?.file_attributes();
    if attributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
        return Ok(());
    }
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        drop(directory);
        // RemoveDirectory 删除链接本身，不枚举或修改目标目录。
        return fs::remove_dir(path)
            .with_context(|| format!("remove managed directory link {}", path.display()));
    }
    for entry in fs::read_dir(path)
        .with_context(|| format!("enumerate managed directory {}", path.display()))?
    {
        let entry = entry?;
        if entry.metadata()?.file_attributes() & FILE_ATTRIBUTE_DIRECTORY != 0 {
            remove_directory_links(&entry.path())?;
        }
    }
    Ok(())
}

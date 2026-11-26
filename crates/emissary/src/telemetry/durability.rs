//! [INPUT]: 目录内已完成的rename与unlink。
//! [OUTPUT]: 记录目录同步债务，失败后重试且不回退内存索引。
//! [POS]: 遥测队列的命名空间持久化边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use anyhow::Result;
use std::path::Path;
#[derive(Default)]
pub(super) struct DirectorySync {
    pending: bool,
    #[cfg(test)]
    pub fail_next: bool,
}
impl DirectorySync {
    pub fn changed(&mut self, path: &Path) -> Result<()> {
        self.pending = true;
        self.flush(path)
    }
    pub fn flush(&mut self, path: &Path) -> Result<()> {
        if !self.pending {
            return Ok(());
        }
        #[cfg(test)]
        if std::mem::take(&mut self.fail_next) {
            anyhow::bail!("injected directory sync failure");
        }
        #[cfg(unix)]
        std::fs::File::open(path)?.sync_all()?;
        #[cfg(not(unix))]
        let _ = path;
        self.pending = false;
        Ok(())
    }
}

/// 写入正文后先更新调用方索引；目录同步由changed单独提交。
pub(super) fn write_record(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("record requires parent"))?;
    crate::config::private_dir(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|error| error.error)?;
    Ok(())
}

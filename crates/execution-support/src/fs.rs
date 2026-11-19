//! [INPUT]: 依赖目标路径、文件字节与创建策略。
//! [OUTPUT]: 写入私有文件并同步文件内容。
//! [POS]: 执行器保存临时凭据和脚本的共享文件边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use anyhow::Result;
use std::{fs, io::Write, path::Path};

pub fn private_write(path: &Path, bytes: &[u8], executable: bool, exclusive: bool) -> Result<()> {
    #[cfg(windows)]
    let _ = executable;
    let mut options = fs::OpenOptions::new();
    options
        .write(true)
        .create_new(exclusive)
        .create(!exclusive)
        .truncate(!exclusive);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(if executable { 0o700 } else { 0o600 });
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

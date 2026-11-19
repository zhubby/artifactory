//! [INPUT]: 依赖临时目录和共享私有文件写入。
//! [OUTPUT]: 验证独占创建、覆盖截断和文件权限。
//! [POS]: 执行器凭据与脚本落盘边界的回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_execution_support::fs::private_write;

#[test]
fn exclusive_files_cannot_replace_existing_credentials() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("credential");
    private_write(&path, b"original", false, true).unwrap();
    assert!(private_write(&path, b"replacement", false, true).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"original");
    private_write(&path, b"new", false, false).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), b"new");
}

#[cfg(unix)]
#[test]
fn credentials_and_scripts_have_private_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    for (name, executable, mode) in [("credential", false, 0o600), ("script", true, 0o700)] {
        let path = directory.path().join(name);
        private_write(&path, b"private", executable, true).unwrap();
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            mode
        );
    }
}

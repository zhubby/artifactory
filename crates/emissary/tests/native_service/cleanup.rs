//! [INPUT]: 原生账户只读查询、有限命令与测试目录。
//! [OUTPUT]: 清理后确认账户和目录消失；查询失败阻断验收。
//! [POS]: 生命周期与持久补报共用的成功门禁，不执行资源删除。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::{USER, command};
use anyhow::{Result, ensure};
use std::{
    path::Path,
    process::{Command, Output},
    time::Duration,
};

pub(super) fn verify(directory: &Path) -> Result<()> {
    if cfg!(target_os = "linux") {
        // useradd创建本地账户；直接读本地账户库，避免把id查询失败当作不存在。
        return verify_local_accounts(directory, Path::new("/etc/passwd"));
    }
    let lookup = if cfg!(windows) {
        let mut cmd = Command::new("pwsh");
        cmd.args(["-NoProfile", "-NonInteractive", "-Command",
            "$ErrorActionPreference='Stop'; try { if(@(Get-LocalUser | Where-Object Name -eq 'artifactory-ci').Count){exit 0}else{exit 1} } catch {exit 2}"]);
        cmd
    } else if cfg!(target_os = "macos") {
        let mut cmd = Command::new("dscl");
        cmd.args([".", "-list", "/Users"]);
        cmd
    } else {
        anyhow::bail!("native cleanup platform unsupported");
    };
    let output = command::run(lookup, Duration::from_secs(10))?;
    verify_observation(directory, &output, cfg!(target_os = "macos"))
}

fn verify_local_accounts(directory: &Path, accounts: &Path) -> Result<()> {
    let records = std::fs::read_to_string(accounts)?;
    let mut count = 0;
    for line in records.lines().filter(|line| !line.is_empty()) {
        let fields: Vec<_> = line.split(':').collect();
        ensure!(
            fields.len() == 7
                && !fields[0].is_empty()
                && fields[2].parse::<u32>().is_ok()
                && fields[3].parse::<u32>().is_ok(),
            "native local account database invalid"
        );
        ensure!(fields[0] != USER, "native test account remains");
        count += 1;
    }
    ensure!(count > 0, "native local account database empty");
    verify_directory(directory)
}

fn verify_observation(directory: &Path, output: &Output, account_list: bool) -> Result<()> {
    if account_list {
        ensure!(output.status.success(), "native account enumeration failed");
        ensure!(
            !std::str::from_utf8(&output.stdout)?
                .lines()
                .any(|line| line == USER),
            "native test account remains"
        );
    } else {
        ensure!(
            output.status.code() == Some(1),
            "native test account remains or lookup failed"
        );
    }
    verify_directory(directory)
}

fn verify_directory(directory: &Path) -> Result<()> {
    verify_directory_observation(directory.try_exists())
}

fn verify_directory_observation(exists: std::io::Result<bool>) -> Result<()> {
    ensure!(!exists?, "native test directory remains after cleanup");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(code: u32, stdout: &[u8]) -> Output {
        #[cfg(unix)]
        let status = {
            use std::os::unix::process::ExitStatusExt;
            std::process::ExitStatus::from_raw((code as i32) << 8)
        };
        #[cfg(windows)]
        let status = {
            use std::os::windows::process::ExitStatusExt;
            std::process::ExitStatus::from_raw(code)
        };
        Output {
            status,
            stdout: stdout.into(),
            stderr: Vec::new(),
        }
    }

    #[test]
    fn residual_account_and_failed_lookup_block_success() {
        let root = tempfile::tempdir().unwrap();
        let absent = root.path().join("removed");
        for code in [0, 2, 127] {
            assert!(verify_observation(&absent, &output(code, b""), false).is_err());
        }
        assert!(verify_observation(&absent, &output(1, b""), false).is_ok());
        assert!(verify_observation(&absent, &output(0, b"root\nartifactory-ci\n"), true).is_err());
        assert!(verify_observation(&absent, &output(2, b""), true).is_err());
        assert!(verify_observation(&absent, &output(0, &[0xff]), true).is_err());
        assert!(verify_observation(&absent, &output(0, b"root\n"), true).is_ok());
    }

    #[test]
    fn residual_directory_and_failed_path_query_block_success() {
        let root = tempfile::tempdir().unwrap();
        assert!(verify_observation(root.path(), &output(1, b""), false).is_err());
        let file = root.path().join("file");
        std::fs::write(&file, b"recovery").unwrap();
        assert!(
            verify_directory_observation(Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "fixture path query failed"
            )))
            .is_err()
        );
        assert!(root.path().exists());
        assert_eq!(std::fs::read(file).unwrap(), b"recovery");
    }

    #[test]
    fn local_account_read_failure_or_invalid_records_block_success() {
        let root = tempfile::tempdir().unwrap();
        let absent = root.path().join("removed");
        let accounts = root.path().join("passwd");
        assert!(verify_local_accounts(&absent, &accounts).is_err());
        for rows in [
            "",
            "lookup failed",
            "root:x:invalid:0:root:/root:/bin/sh\n",
            "root:x:0:0:root:/root:/bin/sh\nartifactory-ci:x:42:42:test:/tmp:/bin/false\n",
        ] {
            std::fs::write(&accounts, rows).unwrap();
            assert!(verify_local_accounts(&absent, &accounts).is_err());
        }
        std::fs::write(&accounts, [0xff]).unwrap();
        assert!(verify_local_accounts(&absent, &accounts).is_err());
        std::fs::write(&accounts, "root:x:0:0:root:/root:/bin/sh\n").unwrap();
        assert!(verify_local_accounts(&absent, &accounts).is_ok());
        assert!(verify_local_accounts(root.path(), &accounts).is_err());
    }
}

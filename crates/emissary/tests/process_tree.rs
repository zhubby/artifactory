//! [INPUT]: 依赖真实 Emissary 二进制和缺失配置路径。
//! [OUTPUT]: 验证前台与服务入口在启动失败时退出。
//! [POS]: 节点进程启动契约；通用回收回归位于 execution-support。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
#[test]
fn service_run_exits_when_emissary_cannot_start() {
    let directory = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_artifactory-emissary"))
        .args(["run", "--config"])
        .arg(directory.path().join("missing.json"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    #[cfg(unix)]
    {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_artifactory-emissary"))
            .args(["service-run", "--config"])
            .arg(directory.path().join("missing.json"))
            .output()
            .unwrap();
        assert!(!output.status.success());
    }
}

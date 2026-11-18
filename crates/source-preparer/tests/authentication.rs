//! [INPUT]: 依赖实际 init 二进制及 Git askpass 请求。
//! [OUTPUT]: 验证跨来源的凭据请求返回空值。
//! [POS]: Git/LFS 重定向与认证来源回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
#[test]
fn askpass_releases_credentials_only_to_the_authorized_origin() {
    for (prompt, expected) in [
        ("Username for 'https://git.example': ", "robot"),
        (
            "Password for 'https://robot@git.example/team/repo': ",
            "private-token",
        ),
        ("Password for 'https://other.example': ", ""),
        ("Password for 'http://git.example': ", ""),
        ("Password for 'https://git.example:8443': ", ""),
        ("Password: ", ""),
    ] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_artifactory-source-preparer"))
            .arg(prompt)
            .env("ARTIFACTORY_GIT_ASKPASS", "1")
            .env("ARTIFACTORY_GIT_AUTH_ORIGIN", "https://git.example")
            .env("ARTIFACTORY_GIT_USERNAME", "robot")
            .env("ARTIFACTORY_GIT_PASSWORD", "private-token")
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().trim(),
            expected,
            "{prompt}"
        );
    }
}

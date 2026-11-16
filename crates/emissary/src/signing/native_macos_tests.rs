//! [INPUT]: 依赖 macOS 自带 Terminal.app 与真实 codesign、security。
//! [OUTPUT]: 在夹具证书链有效期内验证提取、信任和错误指纹拒绝。
//! [POS]: 最终应用验签的原生回归；固定夹具时钟，不接触私钥。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;

#[tokio::test]
async fn native_macos_verification_checks_real_certificate_identity() {
    let directory = tempfile::tempdir().unwrap();
    let mut cfg = Config {
        root: directory.path().into(),
        ..Default::default()
    };
    let root = Path::new("/System/Applications/Utilities");
    let bundle = root.join("Terminal.app");
    let prefix = directory.path().join("expected-");
    native(
        &cfg,
        "codesign",
        &[
            "--display",
            &format!("--extract-certificates={}", prefix.display()),
            bundle.to_str().unwrap(),
        ],
        &CancellationToken::new(),
    )
    .await
    .expect("extract the system application's independent reference certificate");
    let certificate = fs::read(format!("{}0", prefix.display())).unwrap();
    let (_, parsed) = x509_parser::parse_x509_certificate(&certificate).unwrap();
    let mut start = parsed.validity().not_before.timestamp();
    let mut end = parsed.validity().not_after.timestamp();
    let mut trust_args = vec!["verify-cert".to_owned(), "-p".into(), "codeSign".into()];
    for index in 0..16 {
        let path = format!("{}{index}", prefix.display());
        if !Path::new(&path).exists() {
            break;
        }
        let certificate = fs::read(&path).unwrap();
        let (_, certificate) = x509_parser::parse_x509_certificate(&certificate).unwrap();
        start = start.max(certificate.validity().not_before.timestamp());
        end = end.min(certificate.validity().not_after.timestamp());
        trust_args.extend(["-c".into(), path]);
    }
    assert!(
        start < end,
        "fixture certificates have no shared validity window"
    );
    let date = std::process::Command::new("/bin/date")
        .args([
            "-u",
            "-r",
            &(start + (end - start) / 2).to_string(),
            "+%Y-%m-%d-%H:%M:%S",
        ])
        .output()
        .unwrap();
    assert!(date.status.success());
    let date = String::from_utf8(date.stdout).unwrap();
    let date = date.trim();
    trust_args.extend(["-d".into(), date.into()]);
    let trust = std::process::Command::new("/usr/bin/security")
        .args(&trust_args)
        .output()
        .unwrap();
    assert!(
        trust.status.success(),
        "system fixture trust failed at {date}: {}{}",
        String::from_utf8_lossy(&trust.stdout),
        String::from_utf8_lossy(&trust.stderr),
    );
    // 旧 runner 的系统证书会过期。仅固定夹具验证时钟，生产仍使用当前时间。
    let security = directory.path().join("security");
    crate::config::private_write(
        &security,
        format!("#!/bin/sh\nexec /usr/bin/security \"$@\" -d {date}\n").as_bytes(),
        true,
        true,
    )
    .unwrap();
    cfg.tools
        .insert("security".into(), security.to_str().unwrap().into());
    let mut binding = Binding {
        profile: "managed:native-macos-test".into(),
        platform: "macos".into(),
        credential_id: "native-test".into(),
        version_id: "native-test".into(),
        notarize: false,
        metadata: pb::signing::Metadata {
            subject: parsed.subject().to_string(),
            fingerprint_sha256: hex::encode(Sha256::digest(&certificate)),
            not_before: 0,
            not_after: i64::MAX,
            identity: "system signing fixture".into(),
            team_id: parsed
                .subject()
                .iter_organizational_unit()
                .next()
                .and_then(|value| value.as_str().ok())
                .map(str::to_owned),
            alias: None,
            purpose: None,
            notarization_available: false,
        },
    };
    let output = Output {
        path: "fixture.zip".into(),
        bundle: Some("Terminal.app".into()),
    };
    check_bundle(&cfg, root, &output, &binding, &CancellationToken::new())
        .await
        .expect("a trusted application must pass native certificate extraction and verification");
    binding.metadata.fingerprint_sha256 = "0".repeat(64);
    let error = check_bundle(&cfg, root, &output, &binding, &CancellationToken::new())
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("macOS signing certificate mismatch"),
        "{error}"
    );
}

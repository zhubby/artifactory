//! [INPUT]: 依赖 macOS 自带 Terminal.app 与真实 codesign、security。
//! [OUTPUT]: 验证证书链提取、信任校验和错误指纹拒绝。
//! [POS]: 最终应用验签的原生回归；只读取系统应用，不接触私钥。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;

#[tokio::test]
async fn native_macos_verification_checks_real_certificate_identity() {
    let directory = tempfile::tempdir().unwrap();
    let cfg = Config {
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

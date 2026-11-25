//! [INPUT]: 依赖发布 CLI、隔离六平台字节夹具与独立测试签名密钥。
//! [OUTPUT]: 验证统一版本签名清单和错误版本的无写入拒绝。
//! [POS]: 原生发布边界回归，不运行夹具二进制或发布资产。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_emissary::ota;
use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
use std::{fs, process::Command};

const PLATFORMS: [(&str, &str); 6] = [
    ("linux", "amd64"),
    ("linux", "arm64"),
    ("darwin", "amd64"),
    ("darwin", "arm64"),
    ("windows", "amd64"),
    ("windows", "arm64"),
];

#[test]
fn publisher_signs_the_unified_product_version_and_all_asset_bytes() {
    let directory = tempfile::tempdir().unwrap();
    for (os, arch) in PLATFORMS {
        fs::write(
            directory.path().join(ota::asset_name(os, arch)),
            format!("test-only-{os}-{arch}"),
        )
        .unwrap();
    }
    let seed = [71; 32];
    let key = SigningKey::from_bytes(&seed);
    let output = Command::new(env!("CARGO_BIN_EXE_emissary-release"))
        .args(["--directory", directory.path().to_str().unwrap()])
        .args(["--version", env!("CARGO_PKG_VERSION")])
        .args(["--commit", &"a".repeat(40)])
        .env(
            "ARTIFACTORY_EMISSARY_RELEASE_SIGNING_SEED",
            STANDARD.encode(seed),
        )
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let raw = fs::read(directory.path().join("emissary-manifest.json")).unwrap();
    let signature = fs::read_to_string(directory.path().join("emissary-manifest.sig")).unwrap();
    let signature_bytes = STANDARD.decode(&signature).unwrap();
    VerifyingKey::from_bytes(&key.verifying_key().to_bytes())
        .unwrap()
        .verify_strict(&raw, &Signature::from_slice(&signature_bytes).unwrap())
        .unwrap();
    let manifest: ota::Manifest = serde_json::from_slice(&raw).unwrap();
    assert_eq!(manifest.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(manifest.commit, "a".repeat(40));
    assert_eq!(manifest.assets.len(), 6);
    let keys = [STANDARD.encode(key.verifying_key().to_bytes())];
    for (os, arch) in PLATFORMS {
        let asset = ota::verify(&raw, &signature, &keys, os, arch).unwrap();
        let bytes = fs::read(directory.path().join(&asset.name)).unwrap();
        ota::verify_bytes(&bytes, &asset).unwrap();
    }
}

#[test]
fn publisher_rejects_a_release_tag_before_reading_secrets_or_writing_assets() {
    let directory = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_emissary-release"))
        .args(["--directory", directory.path().to_str().unwrap()])
        .args([
            "--version",
            &format!("emissary-v{}", env!("CARGO_PKG_VERSION")),
        ])
        .args(["--commit", &"a".repeat(40)])
        .env_remove("ARTIFACTORY_EMISSARY_RELEASE_SIGNING_SEED")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unified workspace version"));
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
}

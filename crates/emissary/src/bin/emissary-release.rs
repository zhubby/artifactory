//! [INPUT]: 依赖包版本、固定源码提交、六平台资产与发布签名密钥。
//! [OUTPUT]: 生成统一产品版本的签名清单及资产摘要。
//! [POS]: 原生发布入口；Git 标签不作为运行版本。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use anyhow::{Context, Result, ensure};
use artifactory_emissary::ota::{self, Asset, Manifest};
use base64::{Engine, engine::general_purpose::STANDARD};
use clap::Parser;
use ed25519_dalek::{Signer, SigningKey};
use std::{fs, path::PathBuf};
#[derive(Parser)]
struct Args {
    #[arg(long)]
    directory: PathBuf,
    #[arg(long)]
    version: String,
    #[arg(long)]
    commit: String,
}
fn validate_metadata(version: &str, commit: &str) -> Result<()> {
    ensure!(
        version == env!("CARGO_PKG_VERSION"),
        "release version must match the unified workspace version {}",
        env!("CARGO_PKG_VERSION")
    );
    ensure!(
        commit.len() == 40 && commit.bytes().all(|c| c.is_ascii_hexdigit()),
        "release commit must be a full Git object ID"
    );
    Ok(())
}
fn main() -> Result<()> {
    let args = Args::parse();
    validate_metadata(&args.version, &args.commit)?;
    let secret = std::env::var("ARTIFACTORY_EMISSARY_RELEASE_SIGNING_SEED")
        .context("release signing seed environment variable required")?;
    let bytes = STANDARD.decode(secret)?;
    let seed: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("release seed must be 32 bytes"))?;
    let key = SigningKey::from_bytes(&seed);
    let mut assets = Vec::new();
    for (os, arch) in [
        ("linux", "amd64"),
        ("linux", "arm64"),
        ("darwin", "amd64"),
        ("darwin", "arm64"),
        ("windows", "amd64"),
        ("windows", "arm64"),
    ] {
        let bytes = fs::read(args.directory.join(ota::asset_name(os, arch)))?;
        let asset = Asset::from_bytes(os, arch, &bytes);
        ensure!(
            asset.size > 0 && asset.size <= ota::MAX_BINARY_SIZE,
            "invalid release size"
        );
        assets.push(asset);
    }
    let manifest = Manifest {
        version: args.version,
        commit: args.commit,
        protocol_min: 1,
        protocol_max: 1,
        assets,
    };
    let raw = serde_json::to_vec_pretty(&manifest)?;
    fs::write(args.directory.join("emissary-manifest.json"), &raw)?;
    fs::write(
        args.directory.join("emissary-manifest.sig"),
        STANDARD.encode(key.sign(&raw).to_bytes()),
    )?;
    println!("signed Emissary manifest for six platforms");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_uses_the_workspace_product_version() {
        let version = env!("CARGO_PKG_VERSION");
        let commit = "a".repeat(40);
        assert!(validate_metadata(version, &commit).is_ok());
        for invalid in [
            format!("v{version}"),
            format!("emissary-v{version}"),
            format!("{version}+build"),
            format!("{version}.0"),
        ] {
            assert!(validate_metadata(&invalid, &commit).is_err(), "{invalid}");
        }
    }

    #[test]
    fn release_rejects_incomplete_or_non_hex_source_identity() {
        for invalid in ["a".repeat(39), "a".repeat(41), "z".repeat(40)] {
            assert!(validate_metadata(env!("CARGO_PKG_VERSION"), &invalid).is_err());
        }
    }
}

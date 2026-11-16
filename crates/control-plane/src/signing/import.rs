//! [INPUT]: 依赖 PKCS12/JKS 解密、X.509 解析及 Ring 公私钥匹配。
//! [OUTPUT]: 验证可用私钥并提取公开证书元数据。
//! [POS]: 管理接口的导入边界；原始解析错误不进入公开响应。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::{Material, Metadata};
use crate::crypto;
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use p12_keystore::{KeyStore, KeyStoreEntry, Pkcs12ImportPolicy};
use ring::{
    rand::SystemRandom,
    signature::{self, KeyPair},
};
use x509_parser::prelude::*;

pub const MAX_MATERIAL_BYTES: usize = 2 * 1024 * 1024;

pub fn inspect(material: &Material) -> Result<Metadata> {
    let bytes = STANDARD
        .decode(material.file_base64())
        .context("invalid signing file")?;
    ensure!(
        !bytes.is_empty() && bytes.len() <= MAX_MATERIAL_BYTES,
        "signing file exceeds 2 MiB"
    );
    let (key, certificate, alias, purpose, notary) = match material {
        Material::MacosPkcs12 {
            password, notary, ..
        } => {
            password_valid(password)?;
            if let Some(notary) = notary {
                ensure!(
                    !notary.key_id.is_empty()
                        && notary.key_id.len() <= 64
                        && notary.key_id.bytes().all(|b| b.is_ascii_alphanumeric()),
                    "invalid notary key ID"
                );
                uuid::Uuid::parse_str(&notary.issuer_id)?;
                let pem = pem_key(&notary.private_key)?;
                ensure!(
                    signature::EcdsaKeyPair::from_pkcs8(
                        &signature::ECDSA_P256_SHA256_ASN1_SIGNING,
                        &pem,
                        &SystemRandom::new()
                    )
                    .is_ok(),
                    "invalid notary private key"
                );
            }
            let store = KeyStore::from_pkcs12(&bytes, password, Pkcs12ImportPolicy::Strict)?;
            let entries: Vec<_> = store
                .entries()
                .filter_map(|(_, entry)| match entry {
                    KeyStoreEntry::PrivateKeyChain(c) => Some(c),
                    _ => None,
                })
                .collect();
            ensure!(
                entries.len() == 1,
                "macOS PKCS12 must contain exactly one private key identity"
            );
            let chain = entries[0];
            let cert = chain.certs().first().context("certificate missing")?;
            (
                chain.key().as_der().to_vec(),
                cert.as_der().to_vec(),
                None,
                None,
                notary.is_some(),
            )
        }
        Material::AndroidKeystore {
            store_password,
            key_password,
            alias,
            purpose,
            ..
        } => {
            password_valid(store_password)?;
            password_valid(key_password)?;
            ensure!(
                !alias.is_empty() && alias.len() <= 200 && !alias.contains(['\0', '\n', '\r']),
                "invalid keystore alias"
            );
            let (key, cert) = if bytes.starts_with(&[0xfe, 0xed, 0xfe, 0xed]) {
                let mut store = jks::KeyStore::new();
                store.load(bytes.as_slice(), store_password.as_bytes())?;
                let entry = store.get_private_key_entry(alias, key_password.as_bytes())?;
                (
                    entry.private_key,
                    entry
                        .certificate_chain
                        .first()
                        .context("certificate missing")?
                        .content
                        .clone(),
                )
            } else {
                ensure!(
                    store_password == key_password,
                    "PKCS12 store and key passwords must match"
                );
                let store =
                    KeyStore::from_pkcs12(&bytes, store_password, Pkcs12ImportPolicy::Strict)?;
                let entry = store
                    .entries()
                    .find(|(name, _)| name.eq_ignore_ascii_case(alias))
                    .map(|(_, e)| e)
                    .context("alias unavailable")?;
                let KeyStoreEntry::PrivateKeyChain(chain) = entry else {
                    anyhow::bail!("alias has no private key")
                };
                (
                    chain.key().as_der().to_vec(),
                    chain
                        .certs()
                        .first()
                        .context("certificate missing")?
                        .as_der()
                        .to_vec(),
                )
            };
            (key, cert, Some(alias.clone()), Some(purpose.clone()), false)
        }
    };
    let (_, cert) = X509Certificate::from_der(&certificate)
        .map_err(|_| anyhow::anyhow!("invalid certificate"))?;
    ensure!(
        cert.validity().is_valid(),
        "certificate is not currently valid"
    );
    let public = cert.public_key().subject_public_key.data.as_ref();
    let matched = signature::RsaKeyPair::from_pkcs8(&key)
        .is_ok_and(|k| k.public_key().as_ref() == public)
        || [
            &signature::ECDSA_P256_SHA256_ASN1_SIGNING,
            &signature::ECDSA_P384_SHA384_ASN1_SIGNING,
        ]
        .into_iter()
        .any(|alg| {
            signature::EcdsaKeyPair::from_pkcs8(alg, &key, &SystemRandom::new())
                .is_ok_and(|k| k.public_key().as_ref() == public)
        })
        || signature::Ed25519KeyPair::from_pkcs8(&key)
            .is_ok_and(|k| k.public_key().as_ref() == public);
    ensure!(
        matched,
        "certificate does not match a supported RSA/EC private key"
    );
    let identity = cert
        .subject()
        .iter_common_name()
        .next()
        .and_then(|v| v.as_str().ok())
        .unwrap_or_default()
        .to_owned();
    let team_id = cert
        .subject()
        .iter_organizational_unit()
        .next()
        .and_then(|v| v.as_str().ok())
        .map(str::to_owned);
    if material.platform() == "macos" {
        ensure!(
            identity.starts_with("Developer ID Application:")
                && team_id
                    .as_ref()
                    .is_some_and(|t| t.len() == 10 && t.bytes().all(|b| b.is_ascii_alphanumeric())),
            "Developer ID Application identity required"
        );
        ensure!(
            cert.extended_key_usage()?
                .is_some_and(|u| u.value.code_signing),
            "code signing certificate required"
        );
        ensure!(
            cert.extensions()
                .iter()
                .any(|e| e.oid.to_id_string() == "1.2.840.113635.100.6.1.13"),
            "Developer ID Application certificate extension required"
        );
    }
    Ok(Metadata {
        subject: cert.subject().to_string(),
        fingerprint_sha256: crypto::hash(&certificate),
        not_before: cert.validity().not_before.timestamp(),
        not_after: cert.validity().not_after.timestamp(),
        identity,
        team_id,
        alias,
        purpose,
        notarization_available: notary,
    })
}

fn password_valid(password: &str) -> Result<()> {
    ensure!(
        password.len() <= 4096 && !password.contains(['\0', '\n', '\r']),
        "invalid signing password"
    );
    Ok(())
}
fn pem_key(pem: &str) -> Result<Vec<u8>> {
    ensure!(
        pem.len() <= 16384 && pem.starts_with("-----BEGIN PRIVATE KEY-----"),
        "PKCS8 notary key required"
    );
    Ok(STANDARD.decode(
        pem.lines()
            .filter(|l| !l.starts_with("-----"))
            .collect::<String>(),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use p12_keystore::{Certificate, PrivateKey, PrivateKeyChain};
    fn material() -> Material {
        let key = rcgen::KeyPair::generate().unwrap();
        let cert = rcgen::CertificateParams::new(vec!["signing.test".into()])
            .unwrap()
            .self_signed(&key)
            .unwrap();
        let mut store = KeyStore::new();
        store.add_entry(
            "release",
            KeyStoreEntry::PrivateKeyChain(PrivateKeyChain::new(
                vec![1],
                PrivateKey::from_der(&key.serialize_der()).unwrap(),
                [Certificate::from_der(cert.der()).unwrap()],
            )),
        );
        Material::AndroidKeystore {
            file_base64: STANDARD.encode(store.writer("password").write().unwrap()),
            store_password: "password".into(),
            key_password: "password".into(),
            alias: "release".into(),
            purpose: super::super::AndroidPurpose::Application,
        }
    }
    #[test]
    fn imports_real_pkcs12_and_rejects_wrong_password_or_alias() {
        let mut material = material();
        let metadata = inspect(&material).unwrap();
        assert_eq!(metadata.alias.as_deref(), Some("release"));
        assert_eq!(metadata.fingerprint_sha256.len(), 64);
        if let Material::AndroidKeystore { alias, .. } = &mut material {
            *alias = "wrong".into();
        }
        assert!(inspect(&material).is_err());
        if let Material::AndroidKeystore {
            alias,
            store_password,
            ..
        } = &mut material
        {
            *alias = "release".into();
            *store_password = "wrong".into();
        }
        assert!(inspect(&material).is_err());
    }
    #[test]
    #[ignore = "requires native JDK keytool"]
    fn native_jks_import_checks_distinct_key_password() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("release.jks");
        let status = std::process::Command::new("keytool")
            .args([
                "-genkeypair",
                "-alias",
                "release",
                "-keyalg",
                "RSA",
                "-keysize",
                "2048",
                "-validity",
                "365",
                "-dname",
                "CN=Signing Test",
                "-storetype",
                "JKS",
                "-storepass",
                "store-test",
                "-keypass",
                "private-test",
                "-keystore",
            ])
            .arg(&path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
        let mut material = Material::AndroidKeystore {
            file_base64: STANDARD.encode(std::fs::read(path).unwrap()),
            store_password: "store-test".into(),
            key_password: "private-test".into(),
            alias: "release".into(),
            purpose: super::super::AndroidPurpose::Application,
        };
        let metadata = inspect(&material).unwrap();
        assert_eq!(metadata.alias.as_deref(), Some("release"));
        if let Material::AndroidKeystore { key_password, .. } = &mut material {
            *key_password = "wrong-test".into();
        }
        assert!(inspect(&material).is_err());
    }
}

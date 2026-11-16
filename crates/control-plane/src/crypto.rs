//! [INPUT]: 依赖 AES-GCM、随机数与 SHA-256。
//! [OUTPUT]: 提供认证摘要、凭据加密及带上下文的信封加密。
//! [POS]: 控制平面秘密存储的密码学边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use anyhow::{Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use rand::RngCore;
use sha2::{Digest, Sha256};

pub fn hash(value: impl AsRef<[u8]>) -> String {
    hex::encode(Sha256::digest(value.as_ref()))
}

pub fn token() -> String {
    let mut bytes = [0_u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

pub fn seal(key: &[u8; 32], value: &[u8]) -> Result<String> {
    let mut nonce = [0_u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce);
    let encrypted = Aes256Gcm::new_from_slice(key)
        .unwrap()
        .encrypt(Nonce::from_slice(&nonce), value)
        .map_err(|_| anyhow::anyhow!("credential encryption failed"))?;
    Ok(STANDARD.encode([nonce.to_vec(), encrypted].concat()))
}

pub fn open(key: &[u8; 32], value: &str) -> Result<Vec<u8>> {
    let bytes = STANDARD.decode(value)?;
    ensure!(bytes.len() >= 28, "invalid encrypted credential");
    Aes256Gcm::new_from_slice(key)
        .unwrap()
        .decrypt(Nonce::from_slice(&bytes[..12]), &bytes[12..])
        .map_err(|_| anyhow::anyhow!("credential authentication failed"))
}

pub fn seal_context(key: &[u8; 32], value: &[u8], context: &[u8]) -> Result<String> {
    let mut nonce = [0_u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce);
    let encrypted = Aes256Gcm::new_from_slice(key)?
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: value,
                aad: context,
            },
        )
        .map_err(|_| anyhow::anyhow!("signing encryption failed"))?;
    Ok(STANDARD.encode([nonce.to_vec(), encrypted].concat()))
}

pub fn open_context(key: &[u8; 32], value: &str, context: &[u8]) -> Result<Vec<u8>> {
    let bytes = STANDARD.decode(value)?;
    ensure!(bytes.len() >= 28, "invalid signing ciphertext");
    Aes256Gcm::new_from_slice(key)?
        .decrypt(
            Nonce::from_slice(&bytes[..12]),
            Payload {
                msg: &bytes[12..],
                aad: context,
            },
        )
        .map_err(|_| anyhow::anyhow!("signing authentication failed"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signing_envelopes_cannot_move_between_tenants_versions_or_keys() {
        let key = [13; 32];
        let value = b"private signing material";
        let context = b"tenant-one/credential-one/version-one";
        let encrypted = seal_context(&key, value, context).unwrap();
        assert_eq!(open_context(&key, &encrypted, context).unwrap(), value);
        for wrong in [
            b"tenant-two/credential-one/version-one".as_slice(),
            b"tenant-one/credential-two/version-one".as_slice(),
            b"tenant-one/credential-one/version-two".as_slice(),
        ] {
            assert!(open_context(&key, &encrypted, wrong).is_err());
        }
        assert!(open_context(&[14; 32], &encrypted, context).is_err());
        assert!(!encrypted.contains("private signing material"));
        assert_ne!(encrypted, seal_context(&key, value, context).unwrap());
    }
}

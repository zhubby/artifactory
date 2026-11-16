//! [INPUT]: 依赖 AES-GCM 上下文加密及部署提供的独立密钥环。
//! [OUTPUT]: 提供版本化材料的信封加密与解密。
//! [POS]: 签名秘密落库边界；数据库不保存解密主密钥。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::crypto;
use anyhow::{Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Clone)]
pub struct KeyRing {
    active: String,
    keys: BTreeMap<String, [u8; 32]>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    key_id: String,
    wrapped_key: String,
    ciphertext: String,
}

impl KeyRing {
    pub fn load(path: &Path) -> Result<Self> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Input {
            active: String,
            keys: BTreeMap<String, String>,
        }
        let metadata = std::fs::metadata(path)?;
        ensure!(
            metadata.is_file() && metadata.len() <= 32768,
            "invalid signing key file"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            ensure!(
                metadata.permissions().mode() & 0o077 == 0,
                "signing key file must be private (0600)"
            );
        }
        let bytes = Zeroizing::new(std::fs::read(path)?);
        let input: Input = serde_json::from_slice(&bytes)?;
        let mut keys = BTreeMap::new();
        for (id, value) in input.keys {
            ensure!(!id.is_empty() && id.len() <= 64, "invalid signing key ID");
            keys.insert(
                id,
                STANDARD
                    .decode(value)?
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("signing keys require 32 bytes"))?,
            );
        }
        ensure!(
            keys.contains_key(&input.active),
            "active signing key is missing"
        );
        Ok(Self {
            active: input.active,
            keys,
        })
    }

    pub fn seal(
        &self,
        tenant: Uuid,
        credential: Uuid,
        version: Uuid,
        bytes: &[u8],
    ) -> Result<serde_json::Value> {
        let context = serde_json::to_vec(&(tenant, credential, version, &self.active))?;
        let mut data_key = Zeroizing::new([0; 32]);
        rand::thread_rng().fill_bytes(data_key.as_mut());
        Ok(serde_json::to_value(Envelope {
            key_id: self.active.clone(),
            wrapped_key: crypto::seal_context(
                &self.keys[&self.active],
                data_key.as_ref(),
                &context,
            )?,
            ciphertext: crypto::seal_context(&data_key, bytes, &context)?,
        })?)
    }

    pub fn open(
        &self,
        tenant: Uuid,
        credential: Uuid,
        version: Uuid,
        value: serde_json::Value,
    ) -> Result<Zeroizing<Vec<u8>>> {
        let envelope: Envelope = serde_json::from_value(value)?;
        let context = serde_json::to_vec(&(tenant, credential, version, &envelope.key_id))?;
        let key = self
            .keys
            .get(&envelope.key_id)
            .ok_or_else(|| anyhow::anyhow!("signing decryption key unavailable"))?;
        let unwrapped = Zeroizing::new(crypto::open_context(key, &envelope.wrapped_key, &context)?);
        let data_key = Zeroizing::new(<[u8; 32]>::try_from(unwrapped.as_slice())?);
        Ok(Zeroizing::new(crypto::open_context(
            &data_key,
            &envelope.ciphertext,
            &context,
        )?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn envelope_binds_all_resource_ids_and_retains_old_decryption_keys() {
        let ring = KeyRing {
            active: "one".into(),
            keys: [("one".into(), [1; 32])].into(),
        };
        let (t, c, v) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let encrypted = ring.seal(t, c, v, b"secret").unwrap();
        assert_eq!(&*ring.open(t, c, v, encrypted.clone()).unwrap(), b"secret");
        assert!(ring.open(Uuid::new_v4(), c, v, encrypted.clone()).is_err());
        assert!(ring.open(t, Uuid::new_v4(), v, encrypted.clone()).is_err());
        assert!(ring.open(t, c, Uuid::new_v4(), encrypted.clone()).is_err());
        let next = KeyRing {
            active: "two".into(),
            keys: [("one".into(), [1; 32]), ("two".into(), [2; 32])].into(),
        };
        assert_eq!(&*next.open(t, c, v, encrypted).unwrap(), b"secret");
    }
}

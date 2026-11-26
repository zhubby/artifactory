//! [INPUT]: 当前节点会话、持久冻结发行与有界续签请求。
//! [OUTPUT]: 在节点锁内复验授权，仅返回原资产对象键。
//! [POS]: 下载票据续签的事务授权边界，不变更更新目标。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::ControlStore;
use crate::protocol::EmissaryAssetRenewRequest;
use anyhow::{Context, Result, ensure};
use artifactory_database::{params, sql};
use sha2::{Digest, Sha256};
use toasty::stmt::Type;
use uuid::Uuid;

impl ControlStore {
    pub(crate) async fn emissary_asset_renewal(
        &self,
        node: Uuid,
        session: Uuid,
        input: &EmissaryAssetRenewRequest,
    ) -> Result<String> {
        let update = Uuid::parse_str(&input.update_id)?;
        ensure!(
            Uuid::parse_str(&input.request_id).is_ok()
                && input.generation > 0
                && valid_digest(&input.manifest_sha256)
                && valid_digest(&input.asset_sha256)
                && input.asset_size > 0
                && input.asset_size <= 128 << 20
                && matches!(input.os.as_str(), "linux" | "darwin" | "windows")
                && matches!(input.arch.as_str(), "amd64" | "arm64"),
            "invalid frozen asset request"
        );
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        let node_row = sql::optional_record(
            &mut tx,
            "SELECT os,arch FROM nodes WHERE id=$1 AND active_session=$2 AND NOT revoked AND certificate_expires_at>clock_timestamp() AND last_seen>clock_timestamp()-interval '30 seconds' FOR UPDATE",
            params![node, session],
            &[("os", Type::String), ("arch", Type::String)],
        )
        .await?
        .context("node session expired")?;
        ensure!(
            node_row.decode::<String>("os")? == input.os
                && node_row.decode::<String>("arch")? == input.arch,
            "node platform changed"
        );
        let frozen = sql::optional_record(
            &mut tx,
            "SELECT r.manifest,r.manifest_hash FROM emissary_updates u JOIN emissary_releases r ON r.id=u.release_id WHERE u.id=$1 AND u.node_id=$2 AND u.generation=$3 AND u.finished_at IS NULL AND NOT u.cancel_requested AND u.status IN ('offered','downloading')",
            params![update, node, input.generation],
            &[("manifest", Type::String), ("manifest_hash", Type::String)],
        )
        .await?
        .context("update is not downloading")?;
        let manifest: String = frozen.decode("manifest")?;
        let hash: String = frozen.decode("manifest_hash")?;
        ensure!(
            hash == input.manifest_sha256
                && hex::encode(Sha256::digest(manifest.as_bytes())) == hash,
            "frozen manifest changed"
        );
        let manifest: crate::ota::Manifest = serde_json::from_str(&manifest)?;
        let asset = manifest
            .assets
            .iter()
            .find(|a| a.os == input.os && a.arch == input.arch)
            .context("frozen platform missing")?;
        let expected_name = format!(
            "artifactory-emissary-{}-{}{}",
            input.os,
            input.arch,
            if input.os == "windows" { ".exe" } else { "" }
        );
        ensure!(
            asset.name == expected_name
                && asset.sha256 == input.asset_sha256
                && asset.size == input.asset_size,
            "frozen asset changed"
        );
        let authorized: bool = sql::scalar(&mut tx,"SELECT EXISTS(SELECT 1 FROM nodes n JOIN emissary_updates u ON u.node_id=n.id WHERE n.id=$1 AND n.active_session=$2 AND NOT n.revoked AND n.certificate_expires_at>clock_timestamp() AND n.last_seen>clock_timestamp()-interval '30 seconds' AND u.id=$3 AND u.generation=$4 AND u.finished_at IS NULL AND NOT u.cancel_requested AND u.status IN ('offered','downloading'))",params![node,session,update,input.generation]).await?;
        ensure!(authorized, "renewal authorization expired while waiting");
        tx.commit().await?;
        Ok(format!("emissary-releases/{hash}/{}", asset.name))
    }
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

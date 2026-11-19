//! [INPUT]: 依赖冻结交付、镜像摘要图和递增发布租约。
//! [OUTPUT]: 领取发布身份，授权冻结镜像的 Manifest 写入。
//! [POS]: 正式版本绑定独立于临时租约；租约失效不释放版本。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::ControlStore;
use anyhow::{Result, ensure};
use artifactory_database::{params, sql};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

const LEASE_SECONDS: i32 = 600;

#[derive(Deserialize)]
pub(crate) struct ReleasePublisherScope {
    pub generation: i64,
    pub ttl_seconds: u32,
    pub source_repositories: Vec<String>,
    pub destination_repositories: Vec<String>,
}

impl ControlStore {
    pub(crate) async fn acquire_release_publisher(
        &self,
        release: Uuid,
        token_hash: &str,
    ) -> Result<Option<i64>> {
        ensure!(token_hash.len() == 64, "invalid publisher token hash");
        Ok(sql::optional_scalar(
            &mut self.pool.clone(),
            "INSERT INTO release_publication_leases(release_id,generation,token_hash,expires_at) SELECT r.id,1,$2,clock_timestamp()+$3*interval '1 second' FROM releases r JOIN release_deliveries d ON d.release_id=r.id JOIN projects p ON p.id=r.project_id JOIN tenants t ON t.id=r.tenant_id WHERE r.id=$1 AND r.status='publishing' AND p.status='active' AND t.status='active' ON CONFLICT(release_id) DO UPDATE SET generation=release_publication_leases.generation+1,token_hash=excluded.token_hash,expires_at=excluded.expires_at WHERE release_publication_leases.expires_at<=clock_timestamp() RETURNING generation",
            params![release, token_hash, LEASE_SECONDS],
        )
        .await?)
    }

    pub(crate) async fn renew_release_publisher(
        &self,
        release: Uuid,
        generation: i64,
    ) -> Result<bool> {
        Ok(sql::execute(
            &mut self.pool.clone(),
            "UPDATE release_publication_leases l SET expires_at=clock_timestamp()+$3*interval '1 second' FROM releases r JOIN projects p ON p.id=r.project_id JOIN tenants t ON t.id=r.tenant_id WHERE l.release_id=$1 AND l.generation=$2 AND l.expires_at>clock_timestamp() AND r.id=l.release_id AND r.status='publishing' AND p.status='active' AND t.status='active'",
            params![release, generation, LEASE_SECONDS],
        )
        .await? == 1)
    }

    pub(crate) async fn finish_release_publisher(
        &self,
        release: Uuid,
        generation: i64,
    ) -> Result<()> {
        sql::execute(
            &mut self.pool.clone(),
            "UPDATE release_publication_leases SET expires_at=clock_timestamp(),token_hash='' WHERE release_id=$1 AND generation=$2",
            params![release, generation],
        )
        .await?;
        Ok(())
    }

    pub(crate) async fn release_publisher_scope(
        &self,
        release: Uuid,
        token: &str,
    ) -> Result<Option<ReleasePublisherScope>> {
        let value: Option<Value> = sql::optional_scalar(
            &mut self.pool.clone(),
            "SELECT jsonb_build_object('generation',l.generation,'ttl_seconds',least(60,greatest(1,floor(extract(epoch FROM l.expires_at-clock_timestamp()))))::int,'source_repositories',coalesce((SELECT jsonb_agg(DISTINCT a.repository) FROM release_images i JOIN image_artifacts a ON a.id=i.image_id WHERE i.release_id=r.id),'[]'::jsonb),'destination_repositories',coalesce((SELECT jsonb_agg(DISTINCT i.repository) FROM release_images i WHERE i.release_id=r.id),'[]'::jsonb)) FROM release_publication_leases l JOIN releases r ON r.id=l.release_id JOIN release_deliveries d ON d.release_id=r.id JOIN projects p ON p.id=r.project_id JOIN tenants t ON t.id=r.tenant_id WHERE l.release_id=$1 AND l.token_hash=$2 AND l.expires_at>clock_timestamp() AND r.status='publishing' AND p.status='active' AND t.status='active'",
            params![release, crate::crypto::hash(token)],
        )
        .await?;
        value
            .map(serde_json::from_value)
            .transpose()
            .map_err(Into::into)
    }

    pub(crate) async fn authorize_release_manifest(
        &self,
        release: Uuid,
        generation: i64,
        repository: &str,
        reference: &str,
        digest: &str,
    ) -> Result<bool> {
        if !artifactory_image_protocol::repository(repository)
            || !artifactory_image_protocol::digest(digest)
        {
            return Ok(false);
        }
        let binding: Option<Value> = sql::optional_scalar(
            &mut self.pool.clone(),
            "SELECT jsonb_build_object('version',i.version,'root_digest',i.root_digest,'manifest_digests',i.manifest_digests) FROM release_images i JOIN releases r ON r.id=i.release_id JOIN release_deliveries d ON d.release_id=r.id JOIN release_publication_leases l ON l.release_id=r.id JOIN projects p ON p.id=r.project_id JOIN tenants t ON t.id=r.tenant_id WHERE r.id=$1 AND l.generation=$2 AND i.repository=$3 AND l.expires_at>clock_timestamp() AND r.status='publishing' AND p.status='active' AND t.status='active'",
            params![release, generation, repository],
        )
        .await?;
        Ok(binding.is_some_and(|binding| manifest_allowed(&binding, reference, digest)))
    }
}

fn manifest_allowed(binding: &Value, reference: &str, digest: &str) -> bool {
    let whitelisted = binding["manifest_digests"]
        .as_array()
        .is_some_and(|digests| {
            digests
                .iter()
                .any(|allowed| allowed.as_str() == Some(digest))
        });
    whitelisted
        && ((reference == binding["version"].as_str().unwrap_or("")
            && binding["root_digest"].as_str() == Some(digest))
            || (artifactory_image_protocol::digest(reference) && reference == digest))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn frozen_manifest_graph_only_accepts_root_version_and_matching_digest_references() {
        let root = format!("sha256:{}", "a".repeat(64));
        let child = format!("sha256:{}", "b".repeat(64));
        let other = format!("sha256:{}", "c".repeat(64));
        let binding =
            json!({"version":"1.2.3-rc.1","root_digest":root,"manifest_digests":[root,child]});
        assert!(manifest_allowed(&binding, "1.2.3-rc.1", &root));
        assert!(manifest_allowed(&binding, &root, &root));
        assert!(manifest_allowed(&binding, &child, &child));
        assert!(!manifest_allowed(&binding, "latest", &root));
        assert!(!manifest_allowed(&binding, "1.2.3-rc.1", &child));
        assert!(!manifest_allowed(&binding, &root, &child));
        assert!(!manifest_allowed(&binding, &other, &other));
    }
}

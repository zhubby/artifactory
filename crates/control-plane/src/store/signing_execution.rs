//! [INPUT]: 依赖节点 session、任务租约、当前授权和固定凭据绑定。
//! [OUTPUT]: 提供材料领取与最终制品验签证据登记。
//! [POS]: 设备签名 RPC 的数据库授权边界；复核锁等待后的状态。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::ControlStore;
use crate::signing::{Binding, Evidence, KeyRing};
use artifactory_database::{params, sql};
use serde_json::{Value, json};
use tonic::Status;
use uuid::Uuid;

pub struct SigningAccess<'a> {
    pub node: Uuid,
    pub attempt: Uuid,
    pub session: Uuid,
    pub token: &'a str,
    pub profile: &'a str,
    pub version: Uuid,
}

async fn authorize(
    tx: &mut toasty::db::Transaction<'_>,
    access: &SigningAccess<'_>,
) -> Result<(Uuid, Binding, Value), Status> {
    // 先锁住所有授权资源，再用新快照校验。撤权与领取具有确定的先后关系。
    sql::execute(tx,"SELECT a.id FROM attempts a JOIN nodes n ON n.id=a.node_id JOIN jobs j ON j.id=a.job_id JOIN builds b ON b.id=j.build_id JOIN projects p ON p.id=b.project_id JOIN tenants t ON t.id=p.tenant_id JOIN signing_profiles s ON s.tenant_id=p.tenant_id AND s.name=$3 JOIN signing_credentials c ON c.id=s.credential_id JOIN signing_versions v ON v.credential_id=c.id AND v.id=$4 WHERE a.id=$1 AND a.node_id=$2 FOR SHARE OF a,n,p,t,s,c,v,b,j",params![access.attempt,access.node,access.profile,access.version]).await.map_err(super::executions::signing_internal)?;
    let value:Option<Value>=sql::optional_scalar(tx,r#"SELECT jsonb_build_object('tenant_id',b.tenant_id,'binding',binding,'envelope',v.envelope)
      FROM attempts a JOIN nodes n ON n.id=a.node_id JOIN jobs j ON j.id=a.job_id JOIN builds b ON b.id=j.build_id JOIN projects p ON p.id=b.project_id
      JOIN tenants t ON t.id=b.tenant_id
      JOIN signing_profiles s ON s.tenant_id=b.tenant_id AND s.name=$5
      JOIN signing_credentials c ON c.tenant_id=b.tenant_id AND c.id=s.credential_id
      JOIN signing_versions v ON v.credential_id=c.id AND v.id=$6,
      LATERAL jsonb_array_elements(coalesce(b.planning_context->'signing_defaults','[]')) binding
      WHERE a.id=$1 AND n.id=$2 AND n.active_session=$3 AND a.lease_token=$4
      AND a.status='running' AND a.lease_expires_at>clock_timestamp() AND NOT b.cancel_requested
      AND NOT n.revoked AND n.certificate_expires_at>clock_timestamp() AND n.connection_owner IS NOT NULL
      AND n.tenant_id=b.tenant_id AND p.tenant_id=b.tenant_id AND p.status='active' AND t.status='active'
      AND build_node_allowed(b.id,n.id) AND s.name=ANY(n.signing_profiles) AND s.name=ANY(p.signing_profiles)
      AND j.target_config->'signing_profiles' ? s.name AND s.enabled AND c.enabled AND v.enabled
      AND binding->>'profile'=s.name AND binding->>'version_id'=v.id::text AND binding->>'credential_id'=c.id::text
      AND binding->>'platform'=j.target_config->>'application_platform'
      AND (v.metadata->>'not_before')::bigint<=extract(epoch FROM clock_timestamp())
      AND (v.metadata->>'not_after')::bigint>extract(epoch FROM clock_timestamp())"#,
      params![access.attempt,access.node,access.session,access.token,access.profile,access.version]).await.map_err(super::executions::signing_internal)?;
    let value = value
        .ok_or_else(|| Status::permission_denied("signing authorization expired or revoked"))?;
    let tenant = serde_json::from_value(value["tenant_id"].clone())
        .map_err(super::executions::signing_internal)?;
    let binding = serde_json::from_value(value["binding"].clone())
        .map_err(super::executions::signing_internal)?;
    Ok((tenant, binding, value["envelope"].clone()))
}

impl ControlStore {
    pub async fn acquire_signing_material(
        &self,
        access: SigningAccess<'_>,
        step: u32,
        keys: &KeyRing,
    ) -> Result<String, Status> {
        let mut db = self.pool.clone();
        let mut tx = db
            .transaction()
            .await
            .map_err(super::executions::signing_internal)?;
        let (tenant, binding, envelope) = authorize(&mut tx, &access).await?;
        let active:bool=sql::scalar(&mut tx,"SELECT EXISTS(SELECT 1 FROM step_runs WHERE attempt_id=$1 AND step_index=$2 AND phase='sign' AND status='running')",params![access.attempt,step as i32]).await.map_err(super::executions::signing_internal)?;
        if !active {
            return Err(Status::failed_precondition("signing step not active"));
        }
        let bytes = keys
            .open(
                tenant,
                Uuid::parse_str(&binding.credential_id)
                    .map_err(super::executions::signing_internal)?,
                access.version,
                envelope,
            )
            .map_err(|_| Status::failed_precondition("signing decryption key unavailable"))?;
        sql::execute(&mut tx,"INSERT INTO audit_events(tenant_id,actor,action,resource_id,details) VALUES($1,$2,'signing_material_acquired',$3,$4)",params![tenant,access.node.to_string(),access.version.to_string(),json!({"attempt_id":access.attempt,"profile":access.profile,"step_index":step})]).await.map_err(super::executions::signing_internal)?;
        tx.commit()
            .await
            .map_err(super::executions::signing_internal)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| Status::internal("invalid signing material"))
    }

    pub async fn record_signing_evidence(
        &self,
        node: Uuid,
        attempt: Uuid,
        session: Uuid,
        token: &str,
        evidence: Evidence,
    ) -> Result<(), Status> {
        let version = Uuid::parse_str(&evidence.version_id)
            .map_err(|_| Status::invalid_argument("invalid signing version"))?;
        let access = SigningAccess {
            node,
            attempt,
            session,
            token,
            profile: &evidence.profile,
            version,
        };
        let mut db = self.pool.clone();
        let mut tx = db
            .transaction()
            .await
            .map_err(super::executions::signing_internal)?;
        let (tenant, binding, _) = authorize(&mut tx, &access).await?;
        if evidence.sha256.len() != 64
            || !evidence.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            || evidence.fingerprint_sha256 != binding.metadata.fingerprint_sha256
            || (binding.notarize && !evidence.notarized)
        {
            return Err(Status::invalid_argument(
                "signing evidence does not match fixed identity",
            ));
        }
        let complete:bool=sql::scalar(&mut tx,"SELECT (SELECT count(*) FROM step_runs WHERE attempt_id=$1 AND status='succeeded')=jsonb_array_length(j.target_config->'steps') FROM attempts a JOIN jobs j ON j.id=a.job_id WHERE a.id=$1",params![attempt]).await.map_err(super::executions::signing_internal)?;
        if !complete {
            return Err(Status::failed_precondition(
                "all build steps must finish before final signing verification",
            ));
        }
        let allowed:bool=sql::scalar(&mut tx,"SELECT EXISTS(SELECT 1 FROM attempts a JOIN jobs j ON j.id=a.job_id,LATERAL jsonb_array_elements(coalesce(j.target_config->'signing_outputs','[]')) o WHERE a.id=$1 AND o->>'path'=$2)",params![attempt,&evidence.path]).await.map_err(super::executions::signing_internal)?;
        if !allowed {
            return Err(Status::invalid_argument("undeclared signing output"));
        }
        let value = json!(evidence);
        sql::execute(&mut tx,"INSERT INTO signing_evidence(tenant_id,attempt_id,path,sha256,evidence) VALUES($1,$2,$3,$4,$5) ON CONFLICT(attempt_id,path) DO NOTHING",params![tenant,attempt,&evidence.path,&evidence.sha256,&value]).await.map_err(super::executions::signing_internal)?;
        let saved: Value = sql::scalar(
            &mut tx,
            "SELECT evidence FROM signing_evidence WHERE attempt_id=$1 AND path=$2",
            params![attempt, &evidence.path],
        )
        .await
        .map_err(super::executions::signing_internal)?;
        if saved != value {
            return Err(Status::already_exists(
                "signing output evidence is immutable",
            ));
        }
        sql::execute(&mut tx,"INSERT INTO audit_events(tenant_id,actor,action,resource_id,details) VALUES($1,$2,'signing_output_verified',$3,$4)",params![tenant,node.to_string(),attempt.to_string(),value]).await.map_err(super::executions::signing_internal)?;
        tx.commit()
            .await
            .map_err(super::executions::signing_internal)?;
        Ok(())
    }
}

//! [INPUT]: 依赖当前管理员 Session、租户设备与操作类型契约。
//! [OUTPUT]: 锁后复核准入与会话，持久结果并清扫过期正文。
//! [POS]: 独立于构建 attempt 的只读操作持久化边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::ControlStore;
use crate::{
    auth::Auth,
    error::{ApiError, Result},
};
use artifactory_database::{params, sql};
use artifactory_emissary_protocol::operations::*;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeOperation {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub node_id: Uuid,
    pub actor_id: Uuid,
    pub request_id: Uuid,
    pub kind: OperationKind,
    pub schema: u32,
    pub input: Value,
    pub input_digest: String,
    pub startup_id: Uuid,
    pub control_session: Uuid,
    pub state: OperationState,
    pub sequence: u64,
    pub reason: Option<String>,
    pub delivery_state: String,
    pub cancel_requested_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub execution_cutoff: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub result: Option<OperationResult>,
    pub result_digest: Option<String>,
    pub result_expires_at: Option<DateTime<Utc>>,
}
#[derive(Clone, Debug)]
pub struct NodeOperationDelivery {
    pub envelope: OperationEnvelope,
    pub cancel_requested: bool,
    pub resume: OperationResume,
    pub execute_allowed: bool,
}
fn permission(auth: &Auth) -> Result<()> {
    if auth.token_projects.is_some() || !auth.tenant_admin() {
        return Err(ApiError::Forbidden);
    }
    Ok(())
}
async fn authorize(db: &mut dyn toasty::db::Executor, auth: &Auth) -> Result<()> {
    permission(auth)?;
    let valid:Option<String>=sql::optional_scalar(db,"SELECT s.token_hash FROM sessions s JOIN tenant_members m ON m.user_id=s.user_id JOIN tenants t ON t.id=m.tenant_id WHERE s.token_hash=$1 AND s.user_id=$2 AND s.expires_at>clock_timestamp() AND m.tenant_id=$3 AND m.role IN ('owner','admin') AND t.status='active' FOR SHARE OF s,m,t",params![&auth.credential_hash,auth.user_id,auth.tenant_id]).await?;
    if valid.is_none() {
        return Err(ApiError::Forbidden);
    }
    Ok(())
}
fn decode(value: Value) -> Result<NodeOperation> {
    Ok(serde_json::from_value(value).map_err(anyhow::Error::from)?)
}
const PROJECTION: &str = "to_jsonb(o)-'credential_hash'-'admission_held'-'next_delivery_at' || jsonb_build_object('result',CASE WHEN o.result_expires_at>clock_timestamp() THEN o.result ELSE NULL END)";
impl ControlStore {
    pub async fn create_node_operation(
        &self,
        auth: &Auth,
        node: Uuid,
        input: &OperationCreate,
    ) -> Result<NodeOperation> {
        permission(auth)?;
        let typed = input
            .validated()
            .map_err(|error| ApiError::BadRequest(error.to_string()))?;
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        let active: Option<String> = sql::optional_scalar(
            &mut tx,
            "SELECT status FROM tenants WHERE id=$1 FOR SHARE",
            params![auth.tenant_id],
        )
        .await?;
        if active.as_deref() != Some("active") {
            return Err(ApiError::Forbidden);
        }
        authorize(&mut tx, auth).await?;
        let _: Uuid = sql::optional_scalar(
            &mut tx,
            "SELECT id FROM nodes WHERE id=$1 AND tenant_id=$2 FOR UPDATE",
            params![node, auth.tenant_id],
        )
        .await?
        .ok_or(ApiError::NotFound)?;
        authorize(&mut tx, auth).await?;
        let n:Value=sql::scalar(&mut tx,"SELECT to_jsonb(n)||jsonb_build_object('online',certificate_expires_at>clock_timestamp() AND last_seen>clock_timestamp()-interval '30 seconds' AND connection_owner IS NOT NULL) FROM nodes n WHERE id=$1",params![node]).await?;
        let old:Option<Value>=sql::optional_scalar(&mut tx,&format!("SELECT {PROJECTION} FROM node_operations o WHERE tenant_id=$1 AND node_id=$2 AND request_id=$3"),params![auth.tenant_id,node,input.request_id]).await?;
        if let Some(old) = old {
            let old = decode(old)?;
            if old.input_digest != typed.digest() {
                return Err(ApiError::Conflict(
                    "operation request_id input conflict".into(),
                ));
            }
            tx.commit().await?;
            return Ok(old);
        }
        if n["revoked"] == true
            || n["draining"] == true
            || n["ota_draining"] == true
            || n["online"] != true
        {
            return Err(ApiError::Conflict("device unavailable or offline".into()));
        }
        if !n["capabilities"]
            .as_array()
            .is_some_and(|values| values.iter().any(|value| value == CAPABILITY))
        {
            return Err(ApiError::Conflict("node_operations_unsupported".into()));
        }
        let startup: Uuid = serde_json::from_value(n["startup_id"].clone())
            .map_err(|_| ApiError::Conflict("device startup unavailable".into()))?;
        let session: Uuid = serde_json::from_value(n["active_session"].clone())
            .map_err(|_| ApiError::Conflict("device session unavailable".into()))?;
        let busy: bool = sql::scalar(
            &mut tx,
            "SELECT EXISTS(SELECT 1 FROM node_operations WHERE node_id=$1 AND admission_held)",
            params![node],
        )
        .await?;
        if busy {
            return Err(ApiError::Conflict(
                "another node operation is active".into(),
            ));
        }
        let id = Uuid::new_v4();
        let now: DateTime<Utc> =
            sql::scalar(&mut tx, "SELECT clock_timestamp()", params![]).await?;
        sql::execute(&mut tx,"INSERT INTO node_operations(id,tenant_id,node_id,actor_id,credential_hash,request_id,kind,schema,input,input_digest,startup_id,control_session,issued_at,expires_at,execution_cutoff) VALUES($1,$2,$3,$4,$5,$6,$7,1,$8,$9,$10,$11,$12,$12+interval '30 seconds',$12+interval '60 seconds')",params![id,auth.tenant_id,node,auth.user_id,&auth.credential_hash,input.request_id,input.kind.as_str(),typed.value(),typed.digest(),startup,session,now]).await?;
        sql::execute(&mut tx,"INSERT INTO audit_events(tenant_id,actor,action,resource_id,details) VALUES($1,$2,'node_operation_created',$3,$4)",params![auth.tenant_id,auth.user_id.to_string(),id.to_string(),json!({"node_id":node,"kind":input.kind,"input_digest":typed.digest()})]).await?;
        let value: Value = sql::scalar(
            &mut tx,
            &format!("SELECT {PROJECTION} FROM node_operations o WHERE id=$1"),
            params![id],
        )
        .await?;
        tx.commit().await?;
        decode(value)
    }
    pub async fn node_operation(&self, auth: &Auth, id: Uuid) -> Result<NodeOperation> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        authorize(&mut tx, auth).await?;
        let value: Value = sql::optional_scalar(
            &mut tx,
            &format!("SELECT {PROJECTION} FROM node_operations o WHERE id=$1 AND tenant_id=$2"),
            params![id, auth.tenant_id],
        )
        .await?
        .ok_or(ApiError::NotFound)?;
        tx.commit().await?;
        decode(value)
    }
    pub async fn list_node_operations(
        &self,
        auth: &Auth,
        node: Uuid,
        cursor: Option<Uuid>,
        limit: u32,
    ) -> Result<Value> {
        if !(1..=100).contains(&limit) {
            return Err(ApiError::BadRequest(
                "operation limit must be between 1 and 100".into(),
            ));
        }
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        authorize(&mut tx, auth).await?;
        let found: bool = sql::scalar(
            &mut tx,
            "SELECT EXISTS(SELECT 1 FROM nodes WHERE id=$1 AND tenant_id=$2)",
            params![node, auth.tenant_id],
        )
        .await?;
        if !found {
            return Err(ApiError::NotFound);
        }
        let before: Option<Value> = if let Some(cursor) = cursor {
            Some(sql::optional_scalar(&mut tx,"SELECT jsonb_build_object('created_at',created_at,'id',id) FROM node_operations WHERE id=$1 AND node_id=$2 AND tenant_id=$3",params![cursor,node,auth.tenant_id]).await?.ok_or_else(||ApiError::BadRequest("invalid operation cursor".into()))?)
        } else {
            None
        };
        let values: Vec<Value> = if let Some(before) = before {
            let time: DateTime<Utc> = serde_json::from_value(before["created_at"].clone())
                .map_err(anyhow::Error::from)?;
            sql::scalars(&mut tx,&format!("SELECT {PROJECTION} FROM node_operations o WHERE node_id=$1 AND tenant_id=$2 AND (created_at,id)<($3,$4) ORDER BY created_at DESC,id DESC LIMIT $5"),params![node,auth.tenant_id,time,cursor.unwrap(),i64::from(limit)+1]).await?
        } else {
            sql::scalars(&mut tx,&format!("SELECT {PROJECTION} FROM node_operations o WHERE node_id=$1 AND tenant_id=$2 ORDER BY created_at DESC,id DESC LIMIT $3"),params![node,auth.tenant_id,i64::from(limit)+1]).await?
        };
        let mut items: Vec<NodeOperation> =
            values.into_iter().map(decode).collect::<Result<_>>()?;
        let has_more = items.len() > limit as usize;
        items.truncate(limit as usize);
        let next_cursor = has_more.then(|| items.last().unwrap().id);
        tx.commit().await?;
        Ok(json!({"items":items,"next_cursor":next_cursor}))
    }
    pub async fn cancel_node_operation(&self, auth: &Auth, id: Uuid) -> Result<NodeOperation> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        authorize(&mut tx, auth).await?;
        let operation: Value = sql::optional_scalar(
            &mut tx,
            "SELECT to_jsonb(o) FROM node_operations o WHERE id=$1 AND tenant_id=$2 FOR UPDATE",
            params![id, auth.tenant_id],
        )
        .await?
        .ok_or(ApiError::NotFound)?;
        authorize(&mut tx, auth).await?;
        if operation["finished_at"].is_null() {
            sql::execute(&mut tx,"UPDATE node_operations SET cancel_requested_at=COALESCE(cancel_requested_at,clock_timestamp()),next_delivery_at=clock_timestamp(),updated_at=clock_timestamp() WHERE id=$1",params![id]).await?;
            sql::execute(&mut tx,"INSERT INTO audit_events(tenant_id,actor,action,resource_id,details) VALUES($1,$2,'node_operation_cancel_requested',$3,$4)",params![auth.tenant_id,auth.user_id.to_string(),id.to_string(),json!({"node_id":operation["node_id"]})]).await?;
        }
        let value: Value = sql::scalar(
            &mut tx,
            &format!("SELECT {PROJECTION} FROM node_operations o WHERE id=$1"),
            params![id],
        )
        .await?;
        tx.commit().await?;
        decode(value)
    }
    /// 只由当前连接归属副本领取；新会话重授权不延长截止。
    pub async fn pending_node_operations(
        &self,
        owner: Uuid,
        limit: usize,
    ) -> Result<Vec<NodeOperationDelivery>> {
        self.reconcile_node_operations().await?;
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        let rows:Vec<Value>=sql::scalars(&mut tx,"SELECT to_jsonb(o)||jsonb_build_object('current_session',n.active_session,'current_startup',n.startup_id) FROM node_operations o JOIN nodes n ON n.id=o.node_id JOIN tenants t ON t.id=o.tenant_id JOIN sessions s ON s.token_hash=o.credential_hash AND s.user_id=o.actor_id JOIN tenant_members m ON m.user_id=o.actor_id AND m.tenant_id=o.tenant_id WHERE (o.created_at>clock_timestamp()-interval '7 days' OR o.finished_at>clock_timestamp()-interval '7 days') AND (o.next_delivery_at<=clock_timestamp() OR o.control_session<>n.active_session) AND n.connection_owner=$1 AND n.active_session IS NOT NULL AND n.startup_id IS NOT NULL AND NOT n.revoked AND NOT n.draining AND n.certificate_expires_at>clock_timestamp() AND n.last_seen>clock_timestamp()-interval '30 seconds' AND n.capabilities @> '[\"node-operations-v1\"]'::jsonb AND t.status='active' AND s.expires_at>clock_timestamp() AND m.role IN ('owner','admin') AND (NOT n.ota_draining OR o.cancel_requested_at IS NOT NULL) ORDER BY o.next_delivery_at,o.created_at LIMIT $2 FOR UPDATE OF o SKIP LOCKED",params![owner,limit.clamp(1,100) as i64]).await?;
        let mut deliveries = Vec::new();
        for row in rows {
            let op = decode(row.clone())?;
            let current: Uuid = serde_json::from_value(row["current_session"].clone())
                .map_err(anyhow::Error::from)?;
            let envelope = OperationEnvelope {
                operation_id: op.id,
                tenant_id: op.tenant_id,
                node_id: op.node_id,
                startup_id: op.startup_id,
                control_session: current,
                actor_id: op.actor_id,
                kind: op.kind,
                schema: op.schema,
                input: op.input,
                input_digest: op.input_digest,
                issued_at_ms: op.issued_at.timestamp_millis(),
                expires_at_ms: op.expires_at.timestamp_millis(),
            };
            envelope.validate()?;
            sql::execute(&mut tx,"UPDATE node_operations SET control_session=$2,next_delivery_at=clock_timestamp()+interval '3 seconds',delivery_state=CASE WHEN delivery_state IN ('confirmed','outcome_unknown') THEN delivery_state ELSE 'awaiting_result' END,updated_at=clock_timestamp() WHERE id=$1",params![op.id,current]).await?;
            let current_startup: Uuid = serde_json::from_value(row["current_startup"].clone())
                .map_err(anyhow::Error::from)?;
            let resume = OperationResume {
                operation_id: envelope.operation_id,
                tenant_id: envelope.tenant_id,
                node_id: envelope.node_id,
                original_startup_id: envelope.startup_id,
                current_startup_id: current_startup,
                control_session: current,
                input_digest: envelope.input_digest.clone(),
            };
            let execute_allowed = op.finished_at.is_none() && row["admission_held"] == true;
            deliveries.push(NodeOperationDelivery {
                envelope,
                resume,
                execute_allowed,
                cancel_requested: op.cancel_requested_at.is_some(),
            });
        }
        tx.commit().await?;
        Ok(deliveries)
    }
    pub async fn node_operation_session_valid(&self, node: Uuid, session: Uuid) -> Result<bool> {
        Ok(sql::scalar(&mut self.pool.clone(),"SELECT EXISTS(SELECT 1 FROM nodes WHERE id=$1 AND active_session=$2 AND NOT revoked AND certificate_expires_at>clock_timestamp())",params![node,session]).await?)
    }
    pub async fn node_operation_delivery_authorized(
        &self,
        envelope: &OperationEnvelope,
        owner: Uuid,
    ) -> Result<Option<bool>> {
        Ok(sql::optional_scalar(&mut self.pool.clone(),"SELECT o.cancel_requested_at IS NOT NULL FROM node_operations o JOIN nodes n ON n.id=o.node_id JOIN tenants t ON t.id=o.tenant_id JOIN sessions s ON s.token_hash=o.credential_hash AND s.user_id=o.actor_id JOIN tenant_members m ON m.user_id=o.actor_id AND m.tenant_id=o.tenant_id WHERE o.id=$1 AND o.control_session=$2 AND n.active_session=$2 AND n.connection_owner=$3 AND n.startup_id IS NOT NULL AND NOT n.revoked AND NOT n.draining AND n.certificate_expires_at>clock_timestamp() AND n.last_seen>clock_timestamp()-interval '30 seconds' AND t.status='active' AND s.expires_at>clock_timestamp() AND m.role IN ('owner','admin') AND (o.created_at>clock_timestamp()-interval '7 days' OR o.finished_at>clock_timestamp()-interval '7 days') AND (NOT n.ota_draining OR o.cancel_requested_at IS NOT NULL)",params![envelope.operation_id,envelope.control_session,owner]).await?)
    }
    pub async fn record_node_operation_update(
        &self,
        node: Uuid,
        session: Uuid,
        update: &OperationUpdate,
    ) -> Result<Option<OperationAck>> {
        update
            .validate()
            .map_err(|error| ApiError::BadRequest(error.to_string()))?;
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        // 与节点会话替换共用节点锁；等待操作锁后再检查真实时间。
        let locked: Option<Uuid> = sql::optional_scalar(
            &mut tx,
            "SELECT id FROM nodes WHERE id=$1 FOR UPDATE",
            params![node],
        )
        .await?;
        if locked.is_none() {
            return Err(ApiError::NotFound);
        }
        let row: Value = sql::optional_scalar(
            &mut tx,
            "SELECT to_jsonb(o) FROM node_operations o WHERE o.id=$1 AND o.node_id=$2 FOR UPDATE",
            params![update.operation_id, node],
        )
        .await?
        .ok_or(ApiError::NotFound)?;
        let current: bool = sql::scalar(&mut tx,
            "SELECT EXISTS(SELECT 1 FROM nodes WHERE id=$1 AND active_session=$2 AND NOT revoked AND certificate_expires_at>clock_timestamp())", params![node,session]).await?;
        if !current {
            return Err(ApiError::NotFound);
        }
        let old = decode(row.clone())?;
        if old.control_session != session {
            return Err(ApiError::Conflict(
                "operation session requires reauthorization".into(),
            ));
        }
        if old.startup_id != update.startup_id {
            return Err(ApiError::Conflict("operation startup mismatch".into()));
        }
        if old.state.terminal() {
            if old.result_digest != update.result_digest || old.state != update.state {
                return Err(ApiError::Conflict("operation terminal conflict".into()));
            }
            sql::execute(&mut tx,"UPDATE node_operations SET next_delivery_at=clock_timestamp()+interval '7 days' WHERE id=$1",params![old.id]).await?;
            tx.commit().await?;
            return Ok(old.result_digest.map(|result_digest| OperationAck {
                operation_id: old.id,
                result_digest,
            }));
        }
        if update.sequence <= old.sequence {
            tx.commit().await?;
            return Ok(None);
        }
        if update.state == OperationState::Accepted && old.state == OperationState::Running {
            return Err(ApiError::Conflict("operation state regression".into()));
        }
        if update.state.terminal() {
            let result = update
                .result
                .as_ref()
                .ok_or_else(|| ApiError::BadRequest("terminal operation result missing".into()))?;
            let requested = validate_input(old.kind, old.schema, &old.input)?;
            let actual: std::collections::BTreeSet<_> = result
                .probes
                .keys()
                .chain(result.skipped.keys())
                .cloned()
                .collect();
            if matches!(
                update.state,
                OperationState::Succeeded | OperationState::Failed
            ) && actual != requested.probes().into_iter().collect()
            {
                return Err(ApiError::BadRequest(
                    "operation result probe scope mismatch".into(),
                ));
            }
            if update.state == OperationState::Succeeded
                && old.kind == OperationKind::RefreshInventory
                && result.inventory.is_none()
            {
                return Err(ApiError::BadRequest(
                    "inventory refresh observation missing".into(),
                ));
            }
        }
        let result = update
            .result
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(anyhow::Error::from)?;
        let terminal = update.state.terminal();
        let reason = update.reason.map(|reason| {
            serde_json::to_value(reason)
                .expect("reason")
                .as_str()
                .unwrap()
                .to_owned()
        });
        sql::execute(&mut tx,"UPDATE node_operations SET state=$2,sequence=$3,reason=$4,result=$5,result_digest=$6,updated_at=clock_timestamp(),finished_at=CASE WHEN $7 THEN clock_timestamp() ELSE NULL END,result_expires_at=CASE WHEN $7 THEN clock_timestamp()+interval '24 hours' ELSE NULL END,admission_held=admission_held AND NOT $7,delivery_state=CASE WHEN $7 THEN 'confirmed' WHEN delivery_state='outcome_unknown' THEN delivery_state ELSE 'awaiting_result' END,next_delivery_at=CASE WHEN $7 THEN clock_timestamp()+interval '7 days' ELSE next_delivery_at END,execution_cutoff=CASE WHEN $2='accepted' THEN LEAST(execution_cutoff,clock_timestamp()+interval '30 seconds') ELSE execution_cutoff END WHERE id=$1",params![old.id,update.state.as_str(),i64::try_from(update.sequence).map_err(anyhow::Error::from)?,reason,result,&update.result_digest,terminal]).await?;
        if terminal {
            sql::execute(&mut tx,"INSERT INTO audit_events(tenant_id,actor,action,resource_id,details) VALUES($1,$2,'node_operation_finished',$3,$4)",params![old.tenant_id,old.actor_id.to_string(),old.id.to_string(),json!({"node_id":node,"state":update.state,"result_digest":update.result_digest})]).await?;
        }
        tx.commit().await?;
        Ok(if terminal {
            update
                .result_digest
                .clone()
                .map(|result_digest| OperationAck {
                    operation_id: old.id,
                    result_digest,
                })
        } else {
            None
        })
    }
    pub async fn reconcile_node_operations(&self) -> Result<()> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        // 断线不能证明设备执行终态；只释放过期准入锁。
        sql::execute(&mut tx,"UPDATE node_operations SET admission_held=false,delivery_state='outcome_unknown',updated_at=clock_timestamp() WHERE finished_at IS NULL AND admission_held AND execution_cutoff+interval '5 minutes'<clock_timestamp()",params![]).await?;
        sql::execute(&mut tx,"UPDATE node_operations o SET cancel_requested_at=COALESCE(o.cancel_requested_at,clock_timestamp()),next_delivery_at=clock_timestamp() FROM nodes n WHERE o.node_id=n.id AND o.finished_at IS NULL AND n.ota_draining AND o.cancel_requested_at IS NULL",params![]).await?;
        sql::execute(&mut tx,"UPDATE node_operations SET result=NULL WHERE result IS NOT NULL AND result_expires_at<=clock_timestamp()",params![]).await?;
        sql::execute(&mut tx,"DELETE FROM node_operations WHERE created_at<clock_timestamp()-interval '90 days' AND NOT admission_held",params![]).await?;
        tx.commit().await?;
        Ok(())
    }
    pub async fn active_node_operations(&self, node: Uuid) -> Result<i64> {
        Ok(sql::scalar(
            &mut self.pool.clone(),
            "SELECT count(*) FROM node_operations WHERE node_id=$1 AND admission_held",
            params![node],
        )
        .await?)
    }
}

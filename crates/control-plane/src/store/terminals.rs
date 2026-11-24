//! [INPUT]: 依赖当前账户 Session、租户设备授权与终端实例租约。
//! [OUTPUT]: 提供终端原子准入、一次性附着、授权复核和终态审计。
//! [POS]: 终端服务唯一持久化边界；不存储字节流。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::ControlStore;
use crate::{
    auth::Auth,
    crypto,
    error::{ApiError, Result},
};
use artifactory_database::{params, sql};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
pub struct TerminalSession {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub node_id: Uuid,
    pub user_id: Uuid,
    #[serde(skip_serializing)]
    pub credential_hash: String,
    pub renderer_id: Uuid,
    pub owner: Uuid,
    pub node_session: Uuid,
    pub startup_id: Uuid,
    pub profile: String,
    pub term: String,
    pub columns: u16,
    pub rows: u16,
    pub state: String,
    pub generation: i64,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub last_input_at: DateTime<Utc>,
    pub reason: Option<String>,
}

impl std::fmt::Debug for TerminalSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TerminalSession")
            .field("id", &self.id)
            .field("node_id", &self.node_id)
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalCreate {
    pub idempotency_key: Uuid,
    pub renderer_id: Uuid,
    pub profile: String,
    pub columns: u16,
    pub rows: u16,
    pub term: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct TerminalInstance {
    pub id: Uuid,
    pub endpoint: String,
    pub certificate: String,
    pub public_key: String,
}

fn permission(auth: &Auth) -> Result<()> {
    if auth.token_projects.is_some() || !auth.tenant_admin() {
        return Err(ApiError::Forbidden);
    }
    Ok(())
}

impl ControlStore {
    pub async fn terminal_capabilities(&self, auth: &Auth, node: Uuid) -> Result<Value> {
        permission(auth)?;
        let value: Value = sql::optional_scalar(&mut self.pool.clone(),
            "SELECT jsonb_build_object('online',NOT revoked AND NOT draining AND NOT ota_draining AND certificate_expires_at>clock_timestamp() AND last_seen>clock_timestamp()-interval '30 seconds' AND connection_owner IS NOT NULL,'configuration',metadata->'emissary.terminal','capable',capabilities @> '[\"terminal-v1\"]'::jsonb) FROM nodes WHERE id=$1 AND tenant_id=$2",
            params![node,auth.tenant_id]).await?.ok_or(ApiError::NotFound)?;
        let config = &value["configuration"];
        let enabled = value["capable"] == true && config["enabled"] == true;
        Ok(json!({"enabled":enabled,"online":value["online"],
            "profiles":if enabled {config["profiles"].clone()} else {json!([])},
            "account":if enabled {config["account"].clone()} else {Value::Null},
            "reason":if !enabled {Some("terminal_disabled_or_unsupported")} else if value["online"]!=true {Some("device_offline")} else {None}}))
    }

    pub async fn create_terminal(
        &self,
        auth: &Auth,
        node: Uuid,
        input: &TerminalCreate,
    ) -> Result<TerminalSession> {
        permission(auth)?;
        if input.idempotency_key.is_nil()
            || input.renderer_id.is_nil()
            || artifactory_emissary_protocol::terminal::validate_size(
                input.columns.into(),
                input.rows.into(),
            )
            .is_err()
            || input.term != "xterm-256color"
            || input.profile.is_empty()
            || input.profile.len() > 64
        {
            return Err(ApiError::BadRequest(
                "invalid terminal configuration".into(),
            ));
        }
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        sql::execute(
            &mut tx,
            "SELECT pg_advisory_xact_lock(hashtextextended($1,851))",
            params![auth.user_id.to_string()],
        )
        .await?;
        let tenant_active: Option<String> = sql::optional_scalar(
            &mut tx,
            "SELECT status FROM tenants WHERE id=$1 FOR SHARE",
            params![auth.tenant_id],
        )
        .await?;
        if tenant_active.as_deref() != Some("active") {
            return Err(ApiError::Forbidden);
        }
        let authorized:bool=sql::scalar(&mut tx,"SELECT EXISTS(SELECT 1 FROM sessions s JOIN tenant_members m ON m.user_id=s.user_id JOIN tenants t ON t.id=m.tenant_id WHERE s.token_hash=$1 AND s.user_id=$2 AND s.expires_at>clock_timestamp() AND m.tenant_id=$3 AND m.role IN ('owner','admin') AND t.status='active')",params![&auth.credential_hash,auth.user_id,auth.tenant_id]).await?;
        if !authorized {
            return Err(ApiError::Forbidden);
        }
        let previous:Option<Value>=sql::optional_scalar(&mut tx,"SELECT to_jsonb(s) FROM terminal_sessions s WHERE credential_hash=$1 AND idempotency_key=$2",params![&auth.credential_hash,input.idempotency_key]).await?;
        if let Some(previous) = previous {
            let previous: TerminalSession =
                serde_json::from_value(previous).map_err(anyhow::Error::from)?;
            if previous.node_id != node
                || previous.renderer_id != input.renderer_id
                || previous.profile != input.profile
            {
                return Err(ApiError::Conflict(
                    "idempotency key belongs to another terminal".into(),
                ));
            }
            tx.commit().await?;
            return Ok(previous);
        }
        let node_value:Value=sql::optional_scalar(&mut tx,"SELECT to_jsonb(n) || jsonb_build_object('terminal_online',n.certificate_expires_at>clock_timestamp() AND n.last_seen>clock_timestamp()-interval '30 seconds' AND n.connection_owner IS NOT NULL) FROM nodes n JOIN tenants t ON t.id=n.tenant_id WHERE n.id=$1 AND n.tenant_id=$2 AND t.status='active' FOR UPDATE OF n",params![node,auth.tenant_id]).await?.ok_or(ApiError::NotFound)?;
        if node_value["revoked"] == true
            || node_value["draining"] == true
            || node_value["ota_draining"] == true
        {
            return Err(ApiError::Conflict("device unavailable".into()));
        }
        let configuration = &node_value["metadata"]["emissary.terminal"];
        let capable = node_value["capabilities"]
            .as_array()
            .is_some_and(|capabilities| capabilities.iter().any(|value| value == "terminal-v1"));
        if !capable || configuration["enabled"] != true || node_value["terminal_online"] != true {
            return Err(ApiError::Conflict(
                "device terminal is disabled or offline".into(),
            ));
        }
        if !configuration["profiles"]
            .as_array()
            .is_some_and(|p| p.iter().any(|p| p["id"] == input.profile))
        {
            return Err(ApiError::BadRequest("unknown shell profile".into()));
        }
        let owner: Uuid = serde_json::from_value(node_value["connection_owner"].clone())
            .map_err(|_| ApiError::Conflict("device offline".into()))?;
        let live:Option<Uuid>=sql::optional_scalar(&mut tx,"SELECT id FROM terminal_instances WHERE id=$1 AND expires_at>clock_timestamp() FOR UPDATE",params![owner]).await?;
        if live.is_none() {
            return Err(ApiError::Conflict("terminal owner unavailable".into()));
        }
        let user_count:i64=sql::scalar(&mut tx,"SELECT count(*) FROM terminal_sessions WHERE user_id=$1 AND finished_at IS NULL AND expires_at>clock_timestamp()",params![auth.user_id]).await?;
        let node_count:i64=sql::scalar(&mut tx,"SELECT count(*) FROM terminal_sessions WHERE node_id=$1 AND finished_at IS NULL AND expires_at>clock_timestamp()",params![node]).await?;
        if user_count >= 2 || node_count >= 2 {
            return Err(ApiError::TooManyRequests(30));
        }
        let owned:i64=sql::scalar(&mut tx,"SELECT count(*) FROM terminal_sessions WHERE owner=$1 AND finished_at IS NULL AND expires_at>clock_timestamp()",params![owner]).await?;
        if owned >= 64 {
            return Err(ApiError::TooManyRequests(30));
        }
        let id = Uuid::new_v4();
        let node_session: Uuid = serde_json::from_value(node_value["active_session"].clone())
            .map_err(|_| ApiError::Conflict("device offline".into()))?;
        let startup: Uuid = serde_json::from_value(node_value["startup_id"].clone())
            .map_err(|_| ApiError::Conflict("device startup unavailable".into()))?;
        sql::execute(&mut tx,"INSERT INTO terminal_sessions(id,tenant_id,node_id,user_id,credential_hash,idempotency_key,renderer_id,owner,node_session,startup_id,profile,term,columns,rows) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)",params![id,auth.tenant_id,node,auth.user_id,&auth.credential_hash,input.idempotency_key,input.renderer_id,owner,node_session,startup,&input.profile,&input.term,i32::from(input.columns),i32::from(input.rows)]).await?;
        sql::execute(&mut tx,"INSERT INTO audit_events(tenant_id,actor,action,resource_id,details) VALUES($1,$2,'terminal_created',$3,$4)",params![auth.tenant_id,auth.user_id.to_string(),id.to_string(),json!({"node_id":node})]).await?;
        let value: Value = sql::scalar(
            &mut tx,
            "SELECT to_jsonb(s) FROM terminal_sessions s WHERE id=$1",
            params![id],
        )
        .await?;
        tx.commit().await?;
        Ok(serde_json::from_value(value).map_err(anyhow::Error::from)?)
    }

    pub async fn terminal_session(&self, id: Uuid) -> Result<TerminalSession> {
        let value: Value = sql::optional_scalar(
            &mut self.pool.clone(),
            "SELECT to_jsonb(s) FROM terminal_sessions s WHERE id=$1",
            params![id],
        )
        .await?
        .ok_or(ApiError::NotFound)?;
        Ok(serde_json::from_value(value).map_err(anyhow::Error::from)?)
    }

    pub async fn terminal_authorized(&self, id: Uuid) -> Result<bool> {
        self.terminal_access(id, true).await
    }

    /// 持续校验输出读取权限；普通进程退出仍允许排空已授权尾部。
    pub async fn terminal_access_valid(&self, id: Uuid) -> Result<bool> {
        self.terminal_access(id, false).await
    }

    async fn terminal_access(&self, id: Uuid, active: bool) -> Result<bool> {
        Ok(sql::scalar(&mut self.pool.clone(),"SELECT EXISTS(SELECT 1 FROM terminal_sessions x JOIN sessions s ON s.token_hash=x.credential_hash AND s.user_id=x.user_id JOIN tenant_members m ON m.tenant_id=x.tenant_id AND m.user_id=x.user_id JOIN tenants t ON t.id=x.tenant_id JOIN nodes n ON n.id=x.node_id JOIN terminal_instances i ON i.id=x.owner WHERE x.id=$1 AND ((x.finished_at IS NULL AND x.state IN ('pending','opening','active')) OR (NOT $2 AND x.state='closed' AND x.reason IN ('shell_exited','process_exited') AND x.finished_at>clock_timestamp()-interval '30 seconds')) AND x.expires_at>clock_timestamp() AND x.last_input_at>clock_timestamp()-interval '15 minutes' AND s.expires_at>clock_timestamp() AND m.role IN ('owner','admin') AND t.status='active' AND n.tenant_id=x.tenant_id AND n.active_session=x.node_session AND n.startup_id=x.startup_id AND n.connection_owner=x.owner AND NOT n.revoked AND NOT n.draining AND NOT n.ota_draining AND n.certificate_expires_at>clock_timestamp() AND n.last_seen>clock_timestamp()-interval '30 seconds' AND i.expires_at>clock_timestamp())",params![id,active]).await?)
    }

    pub async fn terminal_ticket(&self, auth: &Auth, id: Uuid, renderer: Uuid) -> Result<String> {
        permission(auth)?;
        let terminal = self.terminal_session(id).await?;
        if terminal.user_id != auth.user_id
            || terminal.tenant_id != auth.tenant_id
            || terminal.credential_hash != auth.credential_hash
            || terminal.renderer_id != renderer
        {
            return Err(ApiError::Forbidden);
        }
        if !self.terminal_authorized(id).await? {
            return Err(ApiError::Forbidden);
        }
        let ticket = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        sql::execute(&mut self.pool.clone(),"UPDATE terminal_sessions SET ticket_hash=$2,ticket_expires_at=clock_timestamp()+interval '30 seconds' WHERE id=$1 AND finished_at IS NULL",params![id,crypto::hash(&ticket)]).await?;
        Ok(ticket)
    }

    pub async fn consume_terminal_ticket(
        &self,
        id: Uuid,
        user: Uuid,
        credential: &str,
        renderer: Uuid,
        ticket: &str,
    ) -> Result<TerminalSession> {
        if ticket.len() != 64 || !self.terminal_authorized(id).await? {
            return Err(ApiError::Forbidden);
        }
        let value:Option<Value>=sql::optional_scalar(&mut self.pool.clone(),"UPDATE terminal_sessions x SET ticket_hash=NULL,ticket_expires_at=NULL,generation=generation+1,attached_at=clock_timestamp(),updated_at=clock_timestamp() WHERE id=$1 AND user_id=$2 AND credential_hash=$3 AND renderer_id=$4 AND ticket_hash=$5 AND ticket_expires_at>clock_timestamp() AND finished_at IS NULL RETURNING to_jsonb(x)",params![id,user,credential,renderer,crypto::hash(ticket)]).await?;
        Ok(serde_json::from_value(value.ok_or(ApiError::Forbidden)?)
            .map_err(anyhow::Error::from)?)
    }

    pub async fn terminal_state(&self, id: Uuid, state: &str) -> Result<()> {
        sql::execute(&mut self.pool.clone(),"UPDATE terminal_sessions SET state=$2,updated_at=clock_timestamp() WHERE id=$1 AND finished_at IS NULL AND state IN ('pending','opening','active')",params![id,state]).await?;
        Ok(())
    }

    pub async fn terminal_input(&self, id: Uuid) -> Result<()> {
        sql::execute(&mut self.pool.clone(),"UPDATE terminal_sessions SET last_input_at=clock_timestamp() WHERE id=$1 AND finished_at IS NULL",params![id]).await?;
        Ok(())
    }

    pub async fn close_terminal(
        &self,
        id: Uuid,
        reason: &str,
        code: Option<i32>,
        signal: Option<&str>,
    ) -> Result<()> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        let item:Option<Value>=sql::optional_scalar(&mut tx,"UPDATE terminal_sessions x SET state='closed',finished_at=clock_timestamp(),updated_at=clock_timestamp(),reason=$2,exit_code=$3,exit_signal=$4,ticket_hash=NULL,ticket_expires_at=NULL WHERE id=$1 AND finished_at IS NULL RETURNING to_jsonb(x)",params![id,reason,code,signal]).await?;
        if let Some(item) = item {
            let tenant: Uuid =
                serde_json::from_value(item["tenant_id"].clone()).map_err(anyhow::Error::from)?;
            sql::execute(&mut tx,"INSERT INTO audit_events(tenant_id,actor,action,resource_id,details) VALUES($1,'terminal-runtime','terminal_closed',$2,$3)",params![tenant,id.to_string(),json!({"reason":reason,"code":code,"signal":signal})]).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn finish_terminal(
        &self,
        id: Uuid,
        exit: &crate::protocol::terminal::TerminalExit,
        account: Option<&str>,
        input_bytes: u64,
        output_bytes: u64,
    ) -> Result<()> {
        self.close_terminal(id, &exit.reason, exit.code, exit.signal.as_deref())
            .await?;
        sql::execute(&mut self.pool.clone(),"UPDATE audit_events SET details=details||$2 WHERE action='terminal_closed' AND resource_id=$1",params![id.to_string(),json!({"account":account,"input_bytes":input_bytes,"output_bytes":output_bytes})]).await?;
        Ok(())
    }

    pub async fn register_terminal_instance(&self, instance: &TerminalInstance) -> Result<()> {
        sql::execute(&mut self.pool.clone(),"INSERT INTO terminal_instances(id,endpoint,certificate,public_key,expires_at) VALUES($1,$2,$3,$4,clock_timestamp()+interval '15 seconds') ON CONFLICT(id) DO UPDATE SET expires_at=excluded.expires_at WHERE terminal_instances.endpoint=excluded.endpoint AND terminal_instances.certificate=excluded.certificate AND terminal_instances.public_key=excluded.public_key",params![instance.id,&instance.endpoint,&instance.certificate,&instance.public_key]).await?;
        Ok(())
    }

    pub async fn terminal_instance(&self, id: Uuid) -> Result<TerminalInstance> {
        let value:Value=sql::optional_scalar(&mut self.pool.clone(),"SELECT jsonb_build_object('id',id,'endpoint',endpoint,'certificate',certificate,'public_key',public_key) FROM terminal_instances WHERE id=$1 AND expires_at>clock_timestamp()",params![id]).await?.ok_or(ApiError::BadGateway("terminal owner unavailable".into()))?;
        Ok(serde_json::from_value(value).map_err(anyhow::Error::from)?)
    }

    pub async fn terminal_maintenance(&self) -> Result<()> {
        let ids:Vec<Uuid>=sql::scalars(&mut self.pool.clone(),"SELECT s.id FROM terminal_sessions s LEFT JOIN terminal_instances i ON i.id=s.owner WHERE s.finished_at IS NULL AND (s.expires_at<clock_timestamp() OR i.expires_at IS NULL OR i.expires_at<clock_timestamp() OR (s.state='pending' AND s.created_at<clock_timestamp()-interval '30 seconds')) LIMIT 64",params![]).await?;
        for id in ids {
            self.close_terminal(id, "session_expired", None, None)
                .await?;
        }
        sql::execute(
            &mut self.pool.clone(),
            "DELETE FROM terminal_sessions WHERE finished_at<clock_timestamp()-interval '30 days'",
            params![],
        )
        .await?;
        sql::execute(
            &mut self.pool.clone(),
            "DELETE FROM terminal_instances WHERE expires_at<clock_timestamp()-interval '1 day'",
            params![],
        )
        .await?;
        Ok(())
    }
}

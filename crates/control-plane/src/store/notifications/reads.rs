//! [INPUT]: 依赖个人聚合行、当前权限与事件文案目录。
//! [OUTPUT]: 提供可见范围、首次接收后的时间线、回执与订阅。
//! [POS]: 每次数据库读取重新校验资源权限。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::{super::ControlStore, *};
use artifactory_database::{params, sql};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};

const ACCESS: &str =
    "notification_user_can_read(i.user_id,i.tenant_id,i.project_id,i.resource_type,i.resource_id)";
const ITEM: &str = "to_jsonb(i)||jsonb_build_object('tenant_name',t.name,'tenant_slug',t.slug,'project_name',p.name,'project_slug',p.slug)";
fn cursor(raw: Option<&str>) -> Result<(Option<DateTime<Utc>>, Option<Uuid>)> {
    let Some(raw) = raw else {
        return Ok((None, None));
    };
    let parsed = raw.split_once('|').and_then(|(time, id)| {
        Some((
            DateTime::parse_from_rfc3339(time).ok()?.with_timezone(&Utc),
            Uuid::parse_str(id).ok()?,
        ))
    });
    parsed
        .map(|(a, b)| (Some(a), Some(b)))
        .ok_or_else(|| ApiError::BadRequest("invalid notification cursor".into()))
}
fn page_cursor(item: &NotificationItem) -> String {
    format!("{}|{}", item.updated_at, item.id)
}
fn item(mut value: Value) -> Result<NotificationItem> {
    let safe = value["resource_type"] == "account_safe";
    let (title, summary) = crate::notifications::catalog::render(
        value["event_type"].as_str().unwrap_or_default(),
        &value["payload"],
    );
    value["title"] = json!(title);
    value["summary"] = json!(summary);
    value["url"] = Value::Null;
    if safe {
        for key in [
            "tenant_id",
            "tenant_name",
            "tenant_slug",
            "project_id",
            "project_name",
            "project_slug",
            "resource_id",
        ] {
            value[key] = Value::Null;
        }
        if value["category"] != "delivery" {
            value["needs_action"] = json!(false);
        }
    } else {
        let url = crate::notifications::catalog::resource_url(&value);
        value["url"] = json!(url);
    }
    serde_json::from_value(value).map_err(|e| ApiError::Internal(e.into()))
}
async fn summary_with(
    executor: &mut dyn toasty::db::Executor,
    user: Uuid,
) -> Result<NotificationSummary> {
    let result:Value=sql::scalar(executor,&format!("SELECT jsonb_build_object('unread_count',count(*) FILTER(WHERE i.attention_revision>i.read_revision),'needs_action_count',count(*) FILTER(WHERE i.needs_action AND (i.resource_type<>'account_safe' OR i.category='delivery')),'watermark',COALESCE((SELECT watermark FROM notification_accounts WHERE user_id=$1),0)) FROM notification_inbox i WHERE i.user_id=$1 AND {ACCESS}"),params![user]).await?;
    Ok(serde_json::from_value(result).map_err(anyhow::Error::from)?)
}
impl ControlStore {
    pub async fn notification_scopes(&self, user: Uuid) -> Result<NotificationScopes> {
        // Scope options come from the full current inbox, never one page or
        // historical payload names. Both arrays share this statement snapshot.
        let value:Value=sql::scalar(&mut self.pool.clone(),&format!(r#"
            WITH visible AS MATERIALIZED (
                SELECT DISTINCT i.tenant_id,i.project_id FROM notification_inbox i
                WHERE i.user_id=$1 AND i.resource_type<>'account_safe' AND {ACCESS}
            ) SELECT jsonb_build_object(
                'tenants',COALESCE((SELECT jsonb_agg(to_jsonb(t) ORDER BY t.name,t.id)
                    FROM (SELECT id,name,slug FROM tenants tenant
                        WHERE EXISTS(SELECT 1 FROM visible v WHERE v.tenant_id=tenant.id)) t),'[]'::jsonb),
                'projects',COALESCE((SELECT jsonb_agg(to_jsonb(p) ORDER BY p.name,p.id)
                    FROM (SELECT id,name,slug,tenant_id FROM projects project
                        WHERE EXISTS(SELECT 1 FROM visible v WHERE v.project_id=project.id)) p),'[]'::jsonb))
        "#),params![user]).await?;
        Ok(serde_json::from_value(value).map_err(anyhow::Error::from)?)
    }
    pub async fn notification_summary(&self, user: Uuid) -> Result<NotificationSummary> {
        summary_with(&mut self.pool.clone(), user).await
    }
    pub async fn list_notifications(
        &self,
        user: Uuid,
        filter: NotificationFilter,
    ) -> Result<NotificationPage> {
        category(filter.category.as_deref())?;
        let (time, id) = cursor(filter.cursor.as_deref())?;
        let limit = filter.limit.unwrap_or(30).clamp(1, 100);
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        sql::execute(
            &mut tx,
            "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY",
            params![],
        )
        .await?;
        let values:Vec<Value>=sql::scalars(&mut tx,&format!("SELECT {ITEM} FROM notification_inbox i LEFT JOIN tenants t ON t.id=i.tenant_id LEFT JOIN projects p ON p.id=i.project_id WHERE i.user_id=$1 AND {ACCESS} AND ($2::text IS NULL OR i.category=$2) AND ($3::uuid IS NULL OR (i.tenant_id=$3 AND i.resource_type<>'account_safe')) AND ($4::uuid IS NULL OR (i.project_id=$4 AND i.resource_type<>'account_safe')) AND (NOT COALESCE($5,false) OR i.attention_revision>i.read_revision) AND (NOT COALESCE($6,false) OR (i.needs_action AND (i.resource_type<>'account_safe' OR i.category='delivery'))) AND ($7::timestamptz IS NULL OR (i.updated_at,i.id)<($7,$8)) ORDER BY i.updated_at DESC,i.id DESC LIMIT $9"),params![user,filter.category,filter.tenant_id,filter.project_id,filter.unread,filter.needs_action,time,id,limit+1]).await?;
        let mut items = values.into_iter().map(item).collect::<Result<Vec<_>>>()?;
        let more = items.len() > limit as usize;
        items.truncate(limit as usize);
        let next_cursor = if more {
            items.last().map(page_cursor)
        } else {
            None
        };
        let summary = summary_with(&mut tx, user).await?;
        tx.commit().await?;
        Ok(NotificationPage {
            items,
            next_cursor,
            watermark: summary.watermark,
            unread_count: summary.unread_count,
            needs_action_count: summary.needs_action_count,
        })
    }
    pub async fn read_notification(
        &self,
        user: Uuid,
        id: Uuid,
        revision: i64,
    ) -> Result<NotificationSummary> {
        if revision < 0 {
            return Err(ApiError::BadRequest("invalid attention revision".into()));
        }
        let changed=sql::execute(&mut self.pool.clone(),&format!("UPDATE notification_inbox i SET read_revision=GREATEST(read_revision,LEAST(attention_revision,$3)) WHERE id=$1 AND user_id=$2 AND {ACCESS}"),params![id,user,revision]).await?;
        if changed == 0 {
            return Err(ApiError::NotFound);
        }
        self.notification_summary(user).await
    }
    pub async fn notification_detail(
        &self,
        user: Uuid,
        id: Uuid,
        after: Option<String>,
    ) -> Result<NotificationDetail> {
        let raw:Value=sql::optional_scalar(&mut self.pool.clone(),&format!("SELECT {ITEM} FROM notification_inbox i LEFT JOIN tenants t ON t.id=i.tenant_id LEFT JOIN projects p ON p.id=i.project_id WHERE i.id=$1 AND i.user_id=$2 AND {ACCESS}"),params![id,user]).await?.ok_or(ApiError::NotFound)?;
        let aggregate = raw["aggregate_key"].as_str().unwrap_or_default().to_owned();
        let first_received = raw["first_received_revision"].as_i64().unwrap_or_default();
        let item = item(raw)?;
        let (time, cursor_id) = cursor(after.as_deref())?;
        // 首次收到事项后保留静默更新，避免取消关注后详情缺少恢复结果。
        let events:Vec<Value>=sql::scalars(&mut self.pool.clone(),"SELECT jsonb_build_object('id',e.id,'event_type',e.event_type,'payload',e.payload,'occurred_at',e.occurred_at) FROM notification_events e WHERE e.aggregate_key=$1 AND e.projected_at IS NOT NULL AND e.last_error IS DISTINCT FROM 'unsupported_event_schema' AND e.aggregate_version>=$5 AND notification_user_can_read($2,e.tenant_id,e.project_id,e.resource_type,e.resource_id) AND ($3::timestamptz IS NULL OR (e.occurred_at,e.id)>($3,$4)) ORDER BY e.occurred_at,e.id LIMIT 101",params![aggregate,user,time,cursor_id,first_received]).await?;
        let more = events.len() > 100;
        let events = events
            .into_iter()
            .take(100)
            .map(|v| {
                let (title, summary) = crate::notifications::catalog::render(
                    v["event_type"].as_str().unwrap_or_default(),
                    &v["payload"],
                );
                Ok(NotificationTimelineEvent {
                    id: serde_json::from_value(v["id"].clone()).map_err(anyhow::Error::from)?,
                    event_type: v["event_type"].as_str().unwrap_or_default().into(),
                    title,
                    summary,
                    occurred_at: v["occurred_at"].as_str().unwrap_or_default().into(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let next_cursor = if more {
            events.last().map(|e| format!("{}|{}", e.occurred_at, e.id))
        } else {
            None
        };
        Ok(NotificationDetail {
            item,
            events,
            next_cursor,
        })
    }
    pub async fn read_all_notifications(
        &self,
        user: Uuid,
        input: ReadAllInput,
    ) -> Result<NotificationSummary> {
        category(input.category.as_deref())?;
        if input.watermark < 0 {
            return Err(ApiError::BadRequest(
                "invalid notification watermark".into(),
            ));
        }
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        sql::execute(
            &mut tx,
            "INSERT INTO notification_accounts(user_id) VALUES($1) ON CONFLICT DO NOTHING",
            params![user],
        )
        .await?;
        let current: i64 = sql::scalar(
            &mut tx,
            "SELECT watermark FROM notification_accounts WHERE user_id=$1 FOR UPDATE",
            params![user],
        )
        .await?;
        if input.watermark > current {
            return Err(ApiError::BadRequest(
                "notification watermark is ahead of current state".into(),
            ));
        }
        sql::execute(&mut tx,&format!("UPDATE notification_inbox i SET read_revision=GREATEST(read_revision,attention_revision) WHERE user_id=$1 AND attention_watermark<=$2 AND {ACCESS} AND ($3::text IS NULL OR category=$3) AND ($4::uuid IS NULL OR (tenant_id=$4 AND resource_type<>'account_safe')) AND ($5::uuid IS NULL OR (project_id=$5 AND resource_type<>'account_safe'))"),params![user,input.watermark,input.category,input.tenant_id,input.project_id]).await?;
        tx.commit().await?;
        self.notification_summary(user).await
    }
    pub async fn notification_subscription(
        &self,
        user: Uuid,
        project: Uuid,
    ) -> Result<ProjectSubscription> {
        let allowed:bool=sql::scalar(&mut self.pool.clone(),"SELECT EXISTS(SELECT 1 FROM projects WHERE id=$1 AND notification_user_can_read($2,tenant_id,id,'project',id))",params![project,user]).await?;
        if !allowed {
            return Err(ApiError::NotFound);
        }
        Ok(ProjectSubscription{following:sql::scalar(&mut self.pool.clone(),"SELECT EXISTS(SELECT 1 FROM notification_project_subscriptions WHERE user_id=$1 AND project_id=$2)",params![user,project]).await?})
    }
    pub async fn set_notification_subscription(
        &self,
        user: Uuid,
        project: Uuid,
        following: bool,
    ) -> Result<ProjectSubscription> {
        self.notification_subscription(user, project).await?;
        if following {
            sql::execute(&mut self.pool.clone(),"INSERT INTO notification_project_subscriptions(user_id,project_id) SELECT $1,p.id FROM projects p WHERE p.id=$2 AND notification_user_can_read($1,p.tenant_id,p.id,'project',p.id) ON CONFLICT DO NOTHING",params![user,project]).await?;
        } else {
            sql::execute(
                &mut self.pool.clone(),
                "DELETE FROM notification_project_subscriptions WHERE user_id=$1 AND project_id=$2",
                params![user, project],
            )
            .await?;
        }
        self.notification_subscription(user, project).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn operational_items_link_to_existing_settings_routes() {
        for (kind, category, event, project, expected) in [
            (
                "project_admin",
                "permission",
                "permission.resources.changed",
                true,
                "/tenants/team/projects/app/settings/resources",
            ),
            (
                "tenant_admin",
                "delivery",
                "delivery.channel.changed",
                false,
                "/tenants/team/settings/integrations",
            ),
            (
                "device",
                "device",
                "device.status.changed",
                false,
                "/tenants/team/nodes",
            ),
            (
                "credential",
                "credential",
                "credential.status.changed",
                false,
                "/tenants/team/settings",
            ),
        ] {
            let id = Uuid::new_v4();
            let value = json!({"id":id,"category":category,"event_type":event,"severity":"warning","tenant_id":id,"tenant_name":"Team","tenant_slug":"team","project_id":if project{Some(id)}else{None},"project_name":if project{Some("App")}else{None},"project_slug":if project{Some("app")}else{None},"resource_id":id,"resource_type":kind,"aggregate_key":"credential:test","payload":{},"occurred_at":"2026-11-23T00:00:00Z","updated_at":"2026-11-23T00:00:00Z","content_revision":1,"attention_revision":1,"read_revision":0,"needs_action":true});
            assert_eq!(
                item(value).unwrap().url.as_deref(),
                Some(expected),
                "route for {kind}"
            );
        }
    }
}

//! [INPUT]: 隔离 PostgreSQL、全部业务迁移和真实 ControlStore。
//! [OUTPUT]: 验证节点锁等待期间的取消、权限和租约边界。
//! [POS]: 执行存储并发回归；直接调用事件与心跳事务。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::{ControlStore, EventReceipt, ExecutionEvent};
use anyhow::{Context, Result, ensure};
use artifactory_database::{PoolSettings, connect, params, sql, with_options};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::task::JoinHandle;
use tonic::Code;
use uuid::Uuid;

struct Database {
    admin: toasty::Db,
    pool: toasty::Db,
    schema: String,
    store: ControlStore,
}

impl Database {
    async fn new() -> Result<Self> {
        let url = std::env::var("ARTIFACTORY_TEST_DATABASE_URL")?;
        let admin = connect(&url, Default::default(), PoolSettings::new(2)).await?;
        let schema = format!("execution_fences_{}", Uuid::new_v4().simple());
        sql::execute(
            &mut admin.clone(),
            &format!("CREATE SCHEMA {schema}"),
            params![],
        )
        .await?;
        let scoped = with_options(&url, &[("search_path", &schema)])?;
        let pool = connect(&scoped, super::models(), PoolSettings::new(8)).await?;
        artifactory_database::migrate(&pool, &super::MIGRATIONS).await?;
        let store = ControlStore::new(pool.clone());
        store
            .bootstrap_admin("execution-fence-owner".into(), "unused".into())
            .await?;
        Ok(Self {
            admin,
            pool,
            schema,
            store,
        })
    }

    async fn close(self) -> Result<()> {
        sql::execute(
            &mut self.admin.clone(),
            &format!("DROP SCHEMA {} CASCADE", self.schema),
            params![],
        )
        .await?;
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct Attempt {
    node: Uuid,
    session: Uuid,
    build: Uuid,
    id: Uuid,
}

impl Attempt {
    async fn create(db: &Database, kind: &str) -> Result<Self> {
        let project = Uuid::new_v4();
        let job = Uuid::new_v4();
        let attempt = Self {
            node: Uuid::new_v4(),
            session: Uuid::new_v4(),
            build: Uuid::new_v4(),
            id: Uuid::new_v4(),
        };
        let target = json!({"os":"linux","arch":"amd64","steps":[{"name":"probe","run":"true"}],"artifacts":[]});
        let status = if kind == "accept" {
            "leased"
        } else {
            "running"
        };
        sql::execute(&mut db.pool.clone(), "INSERT INTO projects(id,tenant_id,name,repository_url,pool_ids) VALUES($1,'00000000-0000-0000-0000-000000000001','execution-fence','https://example.invalid/probe.git','{00000000-0000-0000-0000-000000000001}')", params![project]).await?;
        sql::execute(&mut db.pool.clone(), "INSERT INTO nodes(id,name,os,arch,pool_id,certificate_hash,certificate_expires_at,active_session) VALUES($1,'execution-fence','linux','amd64','00000000-0000-0000-0000-000000000001',$2,now()+interval '1 day',$3)", params![attempt.node, attempt.node.to_string(), attempt.session]).await?;
        sql::execute(&mut db.pool.clone(), "WITH fixture_release AS (INSERT INTO releases(id,project_id,version,requested_ref,goal,target_requirements,targets,source_resolution_id) VALUES(gen_random_uuid(),$2,'0.0.0-test.'||replace($1::text,'-',''),'probe','test','[]','{}',$1) RETURNING id,version) INSERT INTO builds(id,project_id,requested_ref,status,source_repository_url,release_id,spec_revision,release_version) VALUES($1,$2,'probe','queued','https://example.invalid/probe.git', (SELECT id FROM fixture_release),1,(SELECT version FROM fixture_release))", params![attempt.build, project]).await?;
        sql::execute(&mut db.pool.clone(), "INSERT INTO jobs(id,build_id,target,target_config,status) VALUES($1,$2,'native',$3,$4)", params![job, attempt.build, target, status]).await?;
        sql::execute(&mut db.pool.clone(), "INSERT INTO attempts(id,job_id,node_id,lease_token,lease_expires_at,status,command_id) VALUES($1,$2,$3,'fixture',now()+interval '90 seconds',$4,$5)", params![attempt.id, job, attempt.node, status, Uuid::new_v4()]).await?;
        Ok(attempt)
    }

    fn spawn(
        self,
        store: ControlStore,
        kind: &'static str,
    ) -> JoinHandle<Result<Outcome, tonic::Status>> {
        tokio::spawn(async move {
            if kind == "heartbeat" {
                return store
                    .renew_attempt(self.node, self.session, self.id, "fixture")
                    .await
                    .map(Outcome::Renewed);
            }
            store
                .submit_execution_event(
                    self.node,
                    self.session,
                    self.id,
                    ExecutionEvent {
                        kind,
                        sequence: u64::from(kind == "finished"),
                        lease_token: "fixture",
                        payload_json: r#"{"status":"failed","error":"execution fence"}"#,
                    },
                )
                .await
                .map(|receipt| match receipt {
                    EventReceipt::Start => Outcome::Started,
                    EventReceipt::Ack(sequence) => Outcome::Acknowledged(sequence),
                })
        })
    }

    async fn snapshot(self, executor: &mut dyn toasty::Executor) -> Result<Value> {
        Ok(sql::scalar(executor, "SELECT jsonb_build_object('status',status,'sequence',last_sequence,'lease',lease_expires_at) FROM attempts WHERE id=$1", params![self.id]).await?)
    }
}

#[derive(Debug, PartialEq)]
enum Outcome {
    Started,
    Acknowledged(u64),
    Renewed(bool),
}

async fn waiting(pool: &toasty::Db, holder: i32) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let blocked: bool = sql::scalar(&mut pool.clone(), "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)) AND wait_event_type='Lock')", params![holder]).await?;
            if blocked {
                return Ok::<_, anyhow::Error>(());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await??;
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL; exercises real execution and cancellation transactions"]
async fn cancellation_commits_while_execution_waits_for_node() -> Result<()> {
    let db = Database::new().await?;
    let result = async {
        for kind in ["accept", "finished", "heartbeat"] {
            let attempt = Attempt::create(&db, kind).await?;
            let mut connection = db.pool.clone();
            let mut holder = connection.transaction().await?;
            let pid: i32 = sql::scalar(&mut holder, "SELECT pg_backend_pid()", params![]).await?;
            sql::execute(
                &mut holder,
                "SELECT id FROM nodes WHERE id=$1 FOR NO KEY UPDATE",
                params![attempt.node],
            )
            .await?;
            let event = attempt.spawn(db.store.clone(), kind);
            waiting(&db.pool, pid).await?;
            let cancellation =
                tokio::time::timeout(Duration::from_secs(2), db.store.cancel_build(attempt.build))
                    .await;
            // 先释放夹具锁，再传播错误，避免失败回归遗留阻塞事务。
            holder.commit().await?;
            let outcome = tokio::time::timeout(Duration::from_secs(5), event).await??;
            cancellation
                .context(format!("{kind}: cancellation waited for the blocked event"))??;
            let commands: i64 = sql::scalar(
                &mut db.pool.clone(),
                "SELECT count(*) FROM device_commands WHERE attempt_id=$1 AND kind='cancel'",
                params![attempt.id],
            )
            .await?;
            ensure!(
                commands == 1,
                "{kind}: cancellation command was not persisted"
            );
            match kind {
                "accept" => ensure!(
                    outcome.is_err_and(|s| s.code() == Code::FailedPrecondition),
                    "waiting accept ignored cancellation"
                ),
                "heartbeat" => ensure!(
                    outcome? == Outcome::Renewed(false),
                    "waiting heartbeat renewed canceled lease"
                ),
                _ => {
                    ensure!(
                        outcome? == Outcome::Acknowledged(1),
                        "waiting completion lost acknowledgement"
                    );
                    ensure!(
                        attempt.snapshot(&mut db.pool.clone()).await?["status"] == "canceled",
                        "waiting completion ignored cancellation"
                    );
                }
            }
        }
        Ok(())
    }
    .await;
    let cleanup = db.close().await;
    result.and(cleanup)
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL; preserves authorization and lease rejection after lock waits"]
async fn execution_lock_wait_preserves_authorization_and_lease_fences() -> Result<()> {
    let db = Database::new().await?;
    let result = async {
        for kind in ["accept", "finished", "heartbeat"] {
            for change in ["revoked", "certificate-expired", "session-superseded", "lease-expired"] {
                let attempt = Attempt::create(&db, kind).await?;
                let mut connection = db.pool.clone();
                let mut holder = connection.transaction().await?;
                let pid: i32 = sql::scalar(&mut holder, "SELECT pg_backend_pid()", params![]).await?;
                let (table, id) = if change == "lease-expired" {
                    ("attempts", attempt.id)
                } else {
                    ("nodes", attempt.node)
                };
                sql::execute(&mut holder, &format!("SELECT id FROM {table} WHERE id=$1 FOR NO KEY UPDATE"), params![id]).await?;
                let event = attempt.spawn(db.store.clone(), kind);
                waiting(&db.pool, pid).await?;
                if change == "revoked" {
                    sql::execute(&mut holder, "UPDATE nodes SET revoked=true WHERE id=$1", params![attempt.node]).await?;
                } else if change == "certificate-expired" {
                    sql::execute(&mut holder, "UPDATE nodes SET certificate_expires_at=clock_timestamp()-interval '1 second' WHERE id=$1", params![attempt.node]).await?;
                } else if change == "session-superseded" {
                    sql::execute(&mut holder, "UPDATE nodes SET active_session=$2 WHERE id=$1", params![attempt.node, Uuid::new_v4()]).await?;
                } else {
                    sql::execute(&mut holder, "UPDATE attempts SET lease_expires_at=clock_timestamp()-interval '1 second' WHERE id=$1", params![attempt.id]).await?;
                }
                let before = attempt.snapshot(&mut holder).await?;
                holder.commit().await?;
                let outcome = tokio::time::timeout(Duration::from_secs(5), event).await??;
                if change != "lease-expired" {
                    ensure!(outcome.is_err_and(|s| s.code() == Code::Unauthenticated), "{kind}/{change}: unauthorized node changed execution");
                } else if kind == "heartbeat" {
                    ensure!(outcome? == Outcome::Renewed(false), "expired lease renewed");
                } else {
                    ensure!(outcome.is_err_and(|s| s.code() == Code::FailedPrecondition), "{kind}: expired lease accepted event");
                }
                ensure!(attempt.snapshot(&mut db.pool.clone()).await? == before, "{kind}/{change}: rejected operation changed execution");
            }
        }
        Ok(())
    }.await;
    let cleanup = db.close().await;
    result.and(cleanup)
}

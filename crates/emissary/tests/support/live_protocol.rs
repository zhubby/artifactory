//! [INPUT]: 验收租户、Control 会话、数据库和设备 mTLS 配置。
//! [OUTPUT]: 提供租户请求、固定源码任务和协议行锁。
//! [POS]: live_protocol 场景复用此夹具；签名上传不附会话凭据。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use anyhow::{Context, Result, ensure};
use artifactory_database::{PoolSettings, connect};
use artifactory_database::{params, sql};
use artifactory_emissary::{client, config::Config, platform, protocol as pb};
use reqwest::{Client, Method};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{path::Path, time::Duration};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Code, Streaming, transport::Channel};
use uuid::Uuid;

pub(super) struct Environment {
    pub(super) http: Client,
    base: String,
    cookie: String,
    tenant_id: Uuid,
    pub(super) db: toasty::Db,
    pub(super) existing: Config,
    source: Value,
}
impl Environment {
    pub(super) async fn load() -> Result<Self> {
        let http = Client::builder().timeout(Duration::from_secs(30)).build()?;
        let base =
            std::env::var("ARTIFACTORY_LIVE_HTTP_URL").unwrap_or("http://127.0.0.1:8080".into());
        let login=http.post(format!("{base}/api/v1/auth/login")).header("x-artifactory-csrf","1").json(&json!({"username":std::env::var("ARTIFACTORY_BOOTSTRAP_USERNAME")?,"password":std::env::var("ARTIFACTORY_BOOTSTRAP_PASSWORD")?})).send().await?;
        ensure!(login.status().is_success(), "live test login failed");
        let cookie = login
            .headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .map(|h| {
                h.to_str()
                    .map(|s| s.split(';').next().unwrap_or("").to_owned())
            })
            .collect::<std::result::Result<Vec<_>, _>>()?
            .join("; ");
        let db = connect(
            &std::env::var("DATABASE_URL")?,
            Default::default(),
            PoolSettings::new(12).timeout(Duration::from_secs(10)),
        )
        .await?;
        let existing = Config::load(Path::new(&std::env::var(
            "ARTIFACTORY_LIVE_EMISSARY_CONFIG",
        )?))?;
        let mut env = Self {
            http,
            base,
            cookie,
            tenant_id: std::env::var("ARTIFACTORY_LIVE_TENANT_ID")?.parse()?,
            db,
            existing,
            source: Value::Null,
        };
        env.source = env
            .api(
                Method::GET,
                &format!(
                    "/projects/{}",
                    std::env::var("ARTIFACTORY_LIVE_PROJECT_ID")?
                ),
                Value::Null,
            )
            .await?;
        Ok(env)
    }
    pub(super) fn request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{}/api/v1{path}", self.base))
            .header(reqwest::header::COOKIE, &self.cookie)
            .header("x-artifactory-tenant", self.tenant_id.to_string())
            .header("x-artifactory-csrf", "1")
    }
    pub(super) async fn api(&self, method: Method, path: &str, body: Value) -> Result<Value> {
        let response = self.request(method, path).json(&body).send().await?;
        ensure!(
            response.status().is_success(),
            "acceptance API {path} returned {}",
            response.status()
        );
        Ok(response.json().await?)
    }
}
pub(super) struct Probe<'a> {
    env: &'a Environment,
    pub(super) cfg: Config,
    pub(super) settings: Value,
    pub(super) build: Uuid,
    pub(super) job: Uuid,
    pub(super) assignment: pb::Command,
    pub(super) control: pb::control_client::ControlClient<Channel>,
    pub(super) send: mpsc::Sender<pb::EmissaryEvent>,
    pub(super) stream: Streaming<pb::Command>,
    _directory: tempfile::TempDir,
}
impl<'a> Probe<'a> {
    pub(super) async fn new(env: &'a Environment) -> Result<Self> {
        let pool = env
            .api(
                Method::POST,
                "/pools",
                json!({"name":format!("emissary-port-{}",Uuid::new_v4())}),
            )
            .await?["id"]
            .as_str()
            .context("missing pool")?
            .to_owned();
        let token = env
            .api(Method::POST, "/enrollment-tokens", json!({"pool_id":pool}))
            .await?["token"]
            .as_str()
            .context("missing token")?
            .to_owned();
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("emissary.json");
        let cfg = client::enroll(
            &path,
            Config {
                endpoint: env.existing.endpoint.clone(),
                ca_file: env.existing.ca_file.clone(),
                root: directory.path().join("runtime"),
                name: "Rust protocol probe".into(),
                ..Default::default()
            },
            token.clone(),
        )
        .await?;
        let reuse = client::enroll(
            &directory.path().join("replay.json"),
            Config {
                root: directory.path().join("replay"),
                ..cfg.clone()
            },
            token,
        )
        .await;
        ensure!(
            reuse.is_err_and(|error| error
                .downcast_ref::<tonic::Status>()
                .is_some_and(|status| status.code() == Code::Unauthenticated)),
            "one-use enrollment replay did not reject a valid new CSR"
        );
        let profiles = env.source["signing_profiles"].clone();
        let settings = json!({"draining":false,"revoked":false,"slots":1,"labels":{},"signing_profiles":profiles,"pool_id":pool});
        env.api(
            Method::PATCH,
            &format!("/nodes/{}", cfg.emissary_id),
            settings.clone(),
        )
        .await?;
        let project=env.api(Method::POST,"/projects",json!({"name":"Rust protocol acceptance","repository_url":env.source["repository_url"],"credential_id":env.source["credential_id"],"signing_profiles":profiles,"pool_ids":[pool]})).await?["id"].as_str().context("missing project")?.parse::<Uuid>()?;
        let build = Uuid::new_v4();
        let job = Uuid::new_v4();
        let source = artifactory_source_checkout::SourceManifest::new(
            Uuid::new_v4(),
            vec![artifactory_source_checkout::SourceRepository {
                path: String::new(),
                upstream_url: env.source["repository_url"]
                    .as_str()
                    .context("source URL missing")?
                    .to_owned(),
                commit_sha: "a".repeat(40),
                tags: Default::default(),
                lfs_objects: vec![],
            }],
        )?;
        let (os, arch) = platform();
        let target = json!({"os":os,"arch":arch,"executor":"native","labels":{},"capabilities":[],"signing_profiles":profiles,"timeout_seconds":120,"steps":[{"name":"probe","phase":"build","shell":if cfg!(windows){"pwsh"}else{"sh"},"run":"echo probe","timeout_seconds":60}],"artifacts":["dist/probe"]});
        // 协议验收直接创建已规划任务。真实规划由 Kubernetes Pi 验收覆盖。
        sql::execute(&mut env.db.clone(), "INSERT INTO source_manifests(id,project_id,tenant_id,commit_sha,manifest,digest) VALUES($1,$2,$3,$4,$5,$6)", params![source.id, project, env.tenant_id, "a".repeat(40), serde_json::to_value(&source)?, &source.digest]).await?;
        sql::execute(&mut env.db.clone(), "INSERT INTO builds(id,project_id,requested_ref,commit_sha,config_snapshot,status,source_repository_url,tenant_id,source_manifest_id) VALUES($1,$2,'probe',$3,$4,'queued',$5,$6,$7)", params![build, project, "a".repeat(40), json!({"version":1,"parameters":{},"targets":{"native":target},"release":{"enabled":false}}), env.source["repository_url"].as_str().context("source URL missing")?, env.tenant_id, source.id]).await?;
        sql::execute(
            &mut env.db.clone(),
            "INSERT INTO jobs(id,build_id,target,target_config,tenant_id) VALUES($1,$2,'native',$3,$4)",
            params![job, build, target, env.tenant_id],
        )
        .await?;
        let control = client::control(&cfg).await?;
        let (send, stream) = open(&cfg, control.clone(), 1).await?;
        let mut probe = Self {
            env,
            cfg,
            settings,
            build,
            job,
            assignment: pb::Command::default(),
            control,
            send,
            stream,
            _directory: directory,
        };
        probe.assignment = probe.next("assign").await?;
        Ok(probe)
    }
    pub(super) fn event(&self, kind: &str) -> pb::EmissaryEvent {
        pb::EmissaryEvent {
            emissary_id: self.cfg.emissary_id.clone(),
            kind: kind.into(),
            attempt_id: self.assignment.attempt_id.clone(),
            lease_token: self.assignment.lease_token.clone(),
            command_id: Uuid::new_v4().to_string(),
            ..Default::default()
        }
    }
    pub(super) async fn next(&mut self, kind: &str) -> Result<pb::Command> {
        loop {
            let command = tokio::time::timeout(Duration::from_secs(20), self.stream.message())
                .await??
                .context("control stream closed")?;
            if command.kind == kind {
                return Ok(command);
            }
            ensure!(
                command.kind == "cancel" || command.kind == "emissary_health",
                "unexpected command {} while waiting for {kind}",
                command.kind
            );
        }
    }
    pub(super) async fn start(&mut self) -> Result<()> {
        let event = self.event("accept");
        let request = event.command_id.clone();
        self.send.send(event).await?;
        let grant = self.next("start").await?;
        ensure!(
            grant.request_id == request,
            "start response lost request anchor"
        );
        Ok(())
    }
    pub(super) async fn lock(&self, table: &str, id: &str) -> Result<(ProtocolLock, i32)> {
        ensure!(
            ["attempts", "nodes"].contains(&table),
            "only owned protocol rows may be locked"
        );
        let id = Uuid::parse_str(id)?;
        let table = table.to_owned();
        let mut db = self.env.db.clone();
        let (ready, initialized) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let mut tx = db.transaction().await?;
            let pid: i32 = sql::scalar(&mut tx, "SELECT pg_backend_pid()", params![]).await?;
            // 阻断事件更新与授权共享锁，允许取消命令的外键 KEY SHARE。
            sql::scalar::<Uuid>(
                &mut tx,
                &format!("SELECT id FROM {table} WHERE id=$1 FOR NO KEY UPDATE"),
                params![id],
            )
            .await?;
            let _ = ready.send(pid);
            if released.await.is_ok() {
                tx.commit().await?;
            }
            Ok(())
        });
        let pid = initialized.await?;
        Ok((ProtocolLock { release, task }, pid))
    }
    pub(super) async fn waiting(&self, pid: i32) -> Result<()> {
        tokio::time::timeout(Duration::from_secs(10),async{
            loop {let blocked:bool=sql::scalar(&mut self.env.db.clone(), "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)) AND wait_event_type='Lock')", params![pid]).await?;if blocked{return Ok::<_,anyhow::Error>(());}tokio::time::sleep(Duration::from_millis(25)).await;}
        }).await??;
        Ok(())
    }
    pub(super) async fn session(&self) -> Result<Uuid> {
        Ok(sql::scalar(
            &mut self.env.db.clone(),
            "SELECT active_session FROM nodes WHERE id=$1",
            params![Uuid::parse_str(&self.cfg.emissary_id)?],
        )
        .await?)
    }
    pub(super) async fn session_changed(&self, old: Uuid) -> Result<()> {
        tokio::time::timeout(Duration::from_secs(10), async {
            while self.session().await? == old {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            Ok::<_, anyhow::Error>(())
        })
        .await??;
        Ok(())
    }
    pub(super) async fn mutate(&mut self, change: &str) -> Result<()> {
        match change {
            "revoked" => {
                self.settings["revoked"] = json!(true);
                self.env
                    .api(
                        Method::PATCH,
                        &format!("/nodes/{}", self.cfg.emissary_id),
                        self.settings.clone(),
                    )
                    .await?;
            }
            "certificate-expired" => {
                sql::execute(&mut self.env.db.clone(), "UPDATE nodes SET certificate_expires_at=clock_timestamp()-interval '1 second' WHERE id=$1", params![Uuid::parse_str(&self.cfg.emissary_id)?]).await?;
            }
            "canceled" => {
                self.env
                    .api(
                        Method::POST,
                        &format!("/builds/{}/cancel", self.build),
                        json!({}),
                    )
                    .await?;
            }
            "session-superseded" => {
                let (tx, stream) = open(&self.cfg, self.control.clone(), 1).await?;
                self.send = tx;
                self.stream = stream;
            }
            "unchanged" => {}
            _ => anyhow::bail!("unknown fence change"),
        }
        Ok(())
    }
    pub(super) async fn cleanup(&self) -> Result<()> {
        let cancellation = self
            .env
            .api(
                Method::POST,
                &format!("/builds/{}/cancel", self.build),
                json!({}),
            )
            .await;
        if let Err(error) = cancellation {
            // 清理时任务已终结则容许重复取消。其他取消错误继续传播。
            let build = self
                .env
                .api(Method::GET, &format!("/builds/{}", self.build), Value::Null)
                .await?;
            if !matches!(
                build["status"].as_str(),
                Some("succeeded" | "failed" | "lost" | "canceled")
            ) {
                return Err(error);
            }
        }
        sql::execute(&mut self.env.db.clone(), "UPDATE attempts SET lease_expires_at=clock_timestamp()-interval '1 second' WHERE job_id=$1 AND status IN ('leased','running')", params![self.job]).await?;
        let mut settings = self.settings.clone();
        settings["revoked"] = json!(true);
        self.env
            .api(
                Method::PATCH,
                &format!("/nodes/{}", self.cfg.emissary_id),
                settings,
            )
            .await?;
        Ok(())
    }
    pub(super) async fn counts(&self) -> Result<(i64, i64)> {
        Ok(sql::tuple(&mut self.env.db.clone(), "SELECT (SELECT count(*) FROM uploads WHERE attempt_id=$1),(SELECT count(*) FROM artifacts WHERE attempt_id=$1)", params![Uuid::parse_str(&self.assignment.attempt_id)?]).await?)
    }
    pub(super) fn declaration(&self, path: &str, data: &[u8]) -> pb::ArtifactRequest {
        pb::ArtifactRequest {
            attempt_id: self.assignment.attempt_id.clone(),
            lease_token: self.assignment.lease_token.clone(),
            path: path.into(),
            size: data.len() as u64,
            sha256: hex::encode(Sha256::digest(data)),
        }
    }
    pub(super) async fn put(&self, ticket: &pb::UploadTicket, data: &[u8]) -> Result<()> {
        ensure!(
            self.env
                .http
                .put(&ticket.url)
                .body(data.to_vec())
                .send()
                .await?
                .status()
                .is_success(),
            "staging upload failed"
        );
        Ok(())
    }
}
pub(super) async fn open(
    cfg: &Config,
    mut control: pb::control_client::ControlClient<Channel>,
    major: u32,
) -> Result<(mpsc::Sender<pb::EmissaryEvent>, Streaming<pb::Command>)> {
    let (tx, rx) = mpsc::channel(64);
    tx.send(pb::EmissaryEvent {
        emissary_id: cfg.emissary_id.clone(),
        kind: "hello".into(),
        protocol_major: major,
        payload_json: json!({"capabilities":["git","bash","sh","pwsh","cc","codesign"]})
            .to_string(),
        ..Default::default()
    })
    .await?;
    let stream = control.connect(ReceiverStream::new(rx)).await?.into_inner();
    Ok((tx, stream))
}

// 锁任务持有独立事务。取消任务会回滚事务。
pub(super) struct ProtocolLock {
    release: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<Result<()>>,
}
impl ProtocolLock {
    pub(super) async fn commit(self) -> Result<()> {
        let _ = self.release.send(());
        self.task.await?
    }
}

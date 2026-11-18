//! [INPUT]: 依赖全新业务基线和隔离 PostgreSQL。
//! [OUTPUT]: 提供租户、源码清单与执行夹具。
//! [POS]: 数据库验收共用初始化边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
impl Database {
    pub(super) async fn new() -> Self {
        Self::initialize().await
    }
    pub(super) async fn with_tenant() -> Self {
        let db = Self::new().await;
        db.ensure_fixture_tenant().await;
        db
    }
    pub(super) async fn ensure_fixture_tenant(&self) {
        self.state
            .store
            .bootstrap_admin("fixture-owner".into(), "unused".into())
            .await
            .unwrap();
        sql::execute(&mut self.pool.clone(),"CREATE OR REPLACE FUNCTION grant_fixture_account() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN INSERT INTO tenant_members(tenant_id,user_id,role) VALUES('00000000-0000-0000-0000-000000000001',NEW.id,CASE WHEN NEW.role='admin' THEN 'admin' ELSE 'member' END) ON CONFLICT DO NOTHING; INSERT INTO project_members(tenant_id,project_id,user_id,role) SELECT tenant_id,id,NEW.id,NEW.role FROM projects ON CONFLICT DO NOTHING; RETURN NEW; END $$",params![]).await.unwrap();
        sql::execute(
            &mut self.pool.clone(),
            "DROP TRIGGER IF EXISTS fixture_account_grants ON users",
            params![],
        )
        .await
        .unwrap();
        sql::execute(&mut self.pool.clone(),"CREATE TRIGGER fixture_account_grants AFTER INSERT ON users FOR EACH ROW EXECUTE FUNCTION grant_fixture_account()",params![]).await.unwrap();
    }
    pub(super) async fn initialize() -> Self {
        let url = std::env::var("ARTIFACTORY_TEST_DATABASE_URL")
            .expect("ARTIFACTORY_TEST_DATABASE_URL is required for explicit database tests");
        let admin = connect(&url, Default::default(), PoolSettings::new(2))
            .await
            .unwrap();
        let schema = format!("test_{}", Uuid::new_v4().simple());
        sql::execute(
            &mut admin.clone(),
            &format!("CREATE SCHEMA {schema}"),
            params![],
        )
        .await
        .unwrap();
        let scoped = with_options(&url, &[("search_path", &schema)]).unwrap();
        let pool = connect(
            &scoped,
            artifactory_control_plane::store::models(),
            PoolSettings::new(10),
        )
        .await
        .unwrap();
        artifactory_database::migrate(&pool, &artifactory_control_plane::store::MIGRATIONS)
            .await
            .unwrap();
        let settings = Settings {
            database_url: scoped,
            http_bind: "127.0.0.1:0".into(),
            grpc_bind: "127.0.0.1:0".into(),
            public_url: "http://localhost:3000".into(),
            insecure_dev: true,
            registration_enabled: true,
            pki_dir: ".local/pki".into(),
            data_dir: std::env::temp_dir(),
            master_key: [1; 32],
            signing_keys: None,
            s3_endpoint: std::env::var("ARTIFACTORY_S3_ENDPOINT")
                .unwrap_or("http://localhost:19000".into()),
            s3_public_endpoint: std::env::var("ARTIFACTORY_S3_ENDPOINT")
                .unwrap_or("http://localhost:19000".into()),
            s3_bucket: std::env::var("ARTIFACTORY_S3_BUCKET").unwrap_or("test".into()),
            s3_region: "us-east-1".into(),
            s3_access_key: std::env::var("ARTIFACTORY_S3_ACCESS_KEY").unwrap_or("test".into()),
            s3_secret_key: std::env::var("ARTIFACTORY_S3_SECRET_KEY").unwrap_or("test".into()),
            telemetry: None,
            control: Default::default(),
        };
        let storage = Arc::new(Storage::new(&settings).unwrap());
        Self {
            state: AppState {
                coordination: Default::default(),
                store: artifactory_control_plane::store::ControlStore::new(pool.clone()),
                settings: Arc::new(settings),
                storage,
                connections: Arc::new(RwLock::new(HashMap::new())),
            },
            pool,
            admin,
            schema,
        }
    }
    pub(super) async fn close(self) {
        self.state.coordination.stop();
        // Stop background transactions before DDL teardown takes table locks.
        // Dropping a schema while maintenance crosses its tables can deadlock.
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let status = self.pool.pool().status();
                if status.available == status.size && status.waiting == 0 {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("fixture background transactions must finish before teardown");
        sql::execute(
            &mut self.admin.clone(),
            &format!("DROP SCHEMA {} CASCADE", self.schema),
            params![],
        )
        .await
        .unwrap();
    }
    pub(super) async fn fixture(&self) -> (Uuid, Uuid, Uuid) {
        self.ensure_fixture_tenant().await;
        let project = Uuid::new_v4();
        let build = Uuid::new_v4();
        let job = Uuid::new_v4();
        let recipe=Recipe::parse("version: 1\nrelease: {enabled: true}\ntargets:\n  app:\n    os: linux\n    arch: amd64\n    steps: [{name: compile, run: make}]\n    artifacts: [dist/app]\n").unwrap();
        let query = "INSERT INTO projects(id,tenant_id,name,repository_url,pool_ids) VALUES($1,'00000000-0000-0000-0000-000000000001','test','https://example.com/repo.git','{00000000-0000-0000-0000-000000000001}')";
        sql::execute(&mut self.pool.clone(), query, params![project])
            .await
            .unwrap();
        sql::execute(&mut self.pool.clone(), "INSERT INTO builds(id,project_id,requested_ref,commit_sha,config_snapshot,status,release_version,source_repository_url) VALUES($1,$2,'main',$3,$4,'queued','v1','https://example.com/repo.git')", params![build, project, "0".repeat(40), serde_json::to_value(&recipe).unwrap()]).await.unwrap();
        sql::execute(
            &mut self.pool.clone(),
            "INSERT INTO jobs(id,build_id,target,target_config) VALUES($1,$2,'app',$3)",
            params![
                job,
                build,
                serde_json::to_value(&recipe.targets["app"]).unwrap()
            ],
        )
        .await
        .unwrap();
        let manifest = Uuid::new_v4();
        self.source(project, manifest).await;
        sql::execute(
            &mut self.pool.clone(),
            "UPDATE builds SET source_manifest_id=$2 WHERE id=$1",
            params![build, manifest],
        )
        .await
        .unwrap();
        (project, build, job)
    }
    pub(super) async fn source(&self, project: Uuid, id: Uuid) {
        let manifest = artifactory_source_checkout::SourceManifest::new(
            id,
            vec![artifactory_source_checkout::SourceRepository {
                path: String::new(),
                upstream_url: "https://example.com/repo.git".into(),
                commit_sha: "0".repeat(40),
                tags: Default::default(),
                lfs_objects: vec![],
            }],
        )
        .unwrap();
        sql::execute(&mut self.pool.clone(), "INSERT INTO source_manifests(id,project_id,commit_sha,manifest,digest) VALUES($1,$2,$3,$4,$5)",params![id,project,"0".repeat(40),serde_json::to_value(&manifest).unwrap(),&manifest.digest]).await.unwrap();
    }
    pub(super) async fn node(&self) -> Uuid {
        self.ensure_fixture_tenant().await;
        let node = Uuid::new_v4();
        let session = Uuid::new_v4();
        let (sender, _receiver) = mpsc::channel::<Result<Command, tonic::Status>>(128);
        sql::execute(&mut self.pool.clone(), "INSERT INTO nodes(id,name,os,arch,pool_id,capabilities,certificate_hash,certificate_expires_at,active_session,last_seen,connection_owner) VALUES($1,'test','linux','amd64','00000000-0000-0000-0000-000000000001','[\"git\",\"bash\"]',$2,now()+interval '1 day',$3,now(),$4)", params![node, node.to_string(), session,self.state.coordination.id()]).await.unwrap();
        self.state
            .connections
            .write()
            .await
            .insert(node, Connection { session, sender });
        node
    }
}

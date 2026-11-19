//! [INPUT]: 依赖完整构建、固定源码、目标合同与不可变制品摘要。
//! [OUTPUT]: 冻结版本交付清单，保存正式名称并封存发布失败。
//! [POS]: Release 发布事务边界；Scheduler 在冻结后验证 Pod 并推广镜像。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::ControlStore;
use anyhow::{Context, Result, ensure};
use artifactory_database::{params, sql};
use serde_json::{Value, json};
use uuid::Uuid;
impl ControlStore {
    pub(crate) async fn publish_transaction(&self, build: Uuid) -> Result<Uuid> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        let release: Uuid = sql::scalar(
            &mut tx,
            "SELECT release_id FROM builds WHERE id=$1",
            params![build],
        )
        .await?;
        let r:Value=sql::scalar(&mut tx,"SELECT to_jsonb(r)||jsonb_build_object('project_slug',p.slug) FROM releases r JOIN projects p ON p.id=r.project_id WHERE r.id=$1 FOR UPDATE OF r",params![release]).await?;
        sql::execute(
            &mut tx,
            "SELECT require_active_project($1)",
            params![
                r["project_id"]
                    .as_str()
                    .context("missing project")?
                    .parse::<Uuid>()?
            ],
        )
        .await?;
        if let Some(existing) = sql::optional_scalar::<Uuid>(
            &mut tx,
            "SELECT build_id FROM release_deliveries WHERE release_id=$1",
            params![release],
        )
        .await?
        {
            ensure!(
                existing == build,
                "release delivery is already sealed to another build"
            );
            if r["status"] != "published" {
                sql::execute(
                    &mut tx,
                    "UPDATE releases SET status='publishing',publish_error=NULL WHERE id=$1",
                    params![release],
                )
                .await?;
                sql::execute(
                    &mut tx,
                    "UPDATE builds SET publish_status='pending',publish_error=NULL WHERE id=$1",
                    params![build],
                )
                .await?;
            }
            tx.commit().await?;
            return Ok(release);
        }
        ensure!(r["status"] == "draft", "release is sealed");
        sql::execute(
            &mut tx,
            "SELECT id FROM builds WHERE release_id=$1 ORDER BY id FOR UPDATE",
            params![release],
        )
        .await?;
        let b: Value = sql::scalar(
            &mut tx,
            "SELECT to_jsonb(b) FROM builds b WHERE id=$1",
            params![build],
        )
        .await?;
        ensure!(
            b["status"] == "succeeded" && b["cancel_requested"] == false,
            "only a successful build can be published"
        );
        ensure!(
            b["spec_revision"] == r["spec_revision"]
                && b["source_manifest_id"] == r["source_manifest_id"]
                && b["source_manifest_id"].is_string(),
            "build must use current release revision and fixed source"
        );
        let active:bool=sql::scalar(&mut tx,"SELECT EXISTS(SELECT 1 FROM builds WHERE release_id=$1 AND status NOT IN ('succeeded','failed','lost','canceled')) OR EXISTS(SELECT 1 FROM source_tasks WHERE release_id=$1 AND status IN ('queued','running')) OR EXISTS(SELECT 1 FROM planning_runs p JOIN builds b ON b.id=p.build_id WHERE b.release_id=$1 AND p.state IN ('queued','leased')) OR EXISTS(SELECT 1 FROM attempts a JOIN jobs j ON j.id=a.job_id JOIN builds b ON b.id=j.build_id WHERE b.release_id=$1 AND a.status IN ('leased','running')) OR EXISTS(SELECT 1 FROM image_executions e JOIN jobs j ON j.id=e.job_id JOIN builds b ON b.id=j.build_id WHERE b.release_id=$1 AND e.status IN ('preparing','running'))",params![release]).await?;
        ensure!(!active, "all release work must stop before publication");
        let jobs: Vec<Value> = sql::scalars(
            &mut tx,
            "SELECT to_jsonb(j) FROM jobs j WHERE build_id=$1 ORDER BY target",
            params![build],
        )
        .await?;
        let requirements = r["target_requirements"]
            .as_array()
            .context("release targets missing")?;
        ensure!(
            jobs.len() == requirements.len() && !jobs.is_empty(),
            "every release target requires a result"
        );
        let mut files = Vec::new();
        let mut images = Vec::new();
        let mut names = std::collections::BTreeSet::new();
        for requirement in requirements {
            let key = requirement["key"].as_str().context("target key missing")?;
            let job = jobs
                .iter()
                .find(|j| j["target"] == key)
                .context("release target missing")?;
            ensure!(job["status"] == "succeeded", "every target must succeed");
            let jobid = job["id"].as_str().context("job missing")?.parse::<Uuid>()?;
            if requirement["kind"] == "image" {
                let image:Value=sql::scalar(&mut tx,"SELECT to_jsonb(i)||jsonb_build_object('spec',e.spec,'results',e.results,'candidate_repository',e.candidate_repository,'formal_repository',e.destination_repository) FROM image_artifacts i JOIN image_executions e ON e.id=i.execution_id JOIN jobs j ON j.id=e.job_id JOIN builds b ON b.id=j.build_id WHERE j.id=$1 AND e.status='succeeded' AND e.operation='build' AND e.source_manifest_id=b.source_manifest_id AND e.plan_revision_id IS NOT DISTINCT FROM j.plan_revision_id AND e.id=(SELECT id FROM image_executions WHERE job_id=j.id AND operation='build' ORDER BY created_at DESC,id DESC LIMIT 1)",params![jobid]).await?;
                let root = image["digest"].as_str().context("image digest missing")?;
                let mut digests = std::collections::BTreeSet::from([root.to_owned()]);
                let results = image["results"]
                    .as_array()
                    .context("platform results missing")?;
                let required = requirement["platforms"]
                    .as_array()
                    .context("platform contract missing")?;
                ensure!(
                    required.len() == results.len()
                        && required
                            .iter()
                            .all(|p| results.iter().any(|r| r["platform"] == *p)),
                    "image platform contract is incomplete"
                );
                for result in results {
                    digests.insert(
                        result["digest"]
                            .as_str()
                            .context("platform digest missing")?
                            .to_owned(),
                    );
                }
                let whitelist = json!(digests);
                let imageid = image["id"]
                    .as_str()
                    .context("image identity missing")?
                    .parse::<Uuid>()?;
                let repository = format!(
                    "{}/{}/images/{}",
                    r["tenant_id"].as_str().context("tenant missing")?,
                    r["project_id"].as_str().context("project missing")?,
                    key
                );
                sql::execute(&mut tx,"INSERT INTO release_images(release_id,image_id,repository,version,root_digest,manifest_digests) VALUES($1,$2,$3,$4,$5,$6)",params![release,imageid,&repository,r["version"].as_str().unwrap(),root,&whitelist]).await?;
                images.push(json!({"image_id":imageid,"target":key,"repository":repository,"version":r["version"],"root_digest":root,"manifest_digests":whitelist,"platforms":results}));
            } else {
                let artifacts:Vec<Value>=sql::scalars(&mut tx,"SELECT to_jsonb(ar) FROM artifacts ar JOIN attempts a ON a.id=ar.attempt_id JOIN jobs j ON j.id=a.job_id JOIN builds b ON b.id=j.build_id WHERE j.id=$1 AND a.status='succeeded' AND a.source_manifest_id=b.source_manifest_id AND a.plan_revision_id IS NOT DISTINCT FROM j.plan_revision_id AND a.id=(SELECT id FROM attempts WHERE job_id=j.id ORDER BY created_at DESC,id DESC LIMIT 1) ORDER BY ar.path",params![jobid]).await?;
                ensure!(
                    !artifacts.is_empty(),
                    "native target has no verified artifacts"
                );
                for artifact in artifacts {
                    let path = artifact["path"].as_str().context("artifact path missing")?;
                    let suffix = if let Some(s) = requirement["format_suffix"].as_str() {
                        ensure!(path.ends_with(s), "artifact does not match required suffix");
                        s.to_owned()
                    } else {
                        crate::build::artifact_suffix(path).to_owned()
                    };
                    let filename = crate::build::delivery_filename(
                        r["project_slug"].as_str().unwrap(),
                        requirement["artifact_key"].as_str().unwrap_or(key),
                        r["version"].as_str().unwrap(),
                        requirement["application_platform"]
                            .as_str()
                            .or_else(|| requirement["os"].as_str())
                            .context("native platform missing")?,
                        requirement["arch"]
                            .as_str()
                            .context("native arch missing")?,
                        &suffix,
                    )
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                    ensure!(
                        names.insert(filename.clone()),
                        "delivery filenames must be unique"
                    );
                    let artifactid = artifact["id"].as_str().unwrap().parse::<Uuid>()?;
                    sql::execute(&mut tx,"INSERT INTO release_artifacts(release_id,artifact_id,delivery_filename) VALUES($1,$2,$3)",params![release,artifactid,&filename]).await?;
                    files.push(json!({"artifact_id":artifactid,"target":key,"delivery_filename":filename,"size":artifact["size"],"sha256":artifact["sha256"],"signing":artifact["signing"]}));
                }
            }
        }
        let source: Value = sql::scalar(
            &mut tx,
            "SELECT manifest FROM source_manifests WHERE id=$1",
            params![r["source_manifest_id"].as_str().unwrap().parse::<Uuid>()?],
        )
        .await?;
        let manifest = json!({"source_digest":source["digest"],"source_manifest":source,"schema_version":1,"release_id":release,"project_id":r["project_id"],"project":r["project_slug"],"version":r["version"],"build_id":build,"spec_revision":r["spec_revision"],"source_manifest_id":r["source_manifest_id"],"commit_sha":r["commit_sha"],"files":files,"images":images});
        sql::execute(&mut tx,"INSERT INTO release_deliveries(release_id,build_id,source_manifest_id,spec_revision,manifest) VALUES($1,$2,$3,$4,$5)",params![release,build,r["source_manifest_id"].as_str().unwrap().parse::<Uuid>()?,r["spec_revision"].as_i64().unwrap() as i32,manifest]).await?;
        sql::execute(
            &mut tx,
            "UPDATE releases SET status='publishing',publish_error=NULL WHERE id=$1",
            params![release],
        )
        .await?;
        sql::execute(
            &mut tx,
            "UPDATE builds SET publish_status='pending',publish_error=NULL WHERE id=$1",
            params![build],
        )
        .await?;
        tx.commit().await?;
        Ok(release)
    }
    pub(crate) async fn delivery_images(&self, release: Uuid) -> Result<Vec<Value>> {
        Ok(sql::scalars(&mut self.pool.clone(),"SELECT to_jsonb(r)||jsonb_build_object('candidate_repository',e.candidate_repository,'platforms',i.platforms) FROM release_images r JOIN image_artifacts i ON i.id=r.image_id JOIN image_executions e ON e.id=i.execution_id WHERE r.release_id=$1 ORDER BY r.repository",params![release]).await?)
    }
    pub(crate) async fn complete_release_publication(
        &self,
        release: Uuid,
        generation: i64,
    ) -> Result<()> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        let status: String = sql::scalar(
            &mut tx,
            "SELECT status FROM releases WHERE id=$1 FOR UPDATE",
            params![release],
        )
        .await?;
        if status == "published" {
            tx.commit().await?;
            return Ok(());
        }
        ensure!(status == "publishing", "release publication is not active");
        let active:bool=sql::scalar(&mut tx,"SELECT EXISTS(SELECT 1 FROM release_publication_leases WHERE release_id=$1 AND generation=$2 AND expires_at>clock_timestamp() FOR UPDATE)",params![release,generation]).await?;
        ensure!(active, "release publisher lease expired or superseded");
        sql::execute(&mut tx,"UPDATE releases SET status='published',published_at=coalesce(published_at,now()),publish_error=NULL WHERE id=$1",params![release]).await?;
        sql::execute(&mut tx,"UPDATE builds SET publish_status='succeeded',publish_error=NULL WHERE id=(SELECT build_id FROM release_deliveries WHERE release_id=$1)",params![release]).await?;
        sql::execute(&mut tx,"INSERT INTO publish_attempts(id,build_id,status) SELECT $2,build_id,'succeeded' FROM release_deliveries WHERE release_id=$1",params![release,Uuid::new_v4()]).await?;
        tx.commit().await?;
        Ok(())
    }
    pub(crate) async fn fail_release_publication(
        &self,
        release: Uuid,
        generation: i64,
        error: &str,
    ) -> Result<()> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        let status: String = sql::scalar(
            &mut tx,
            "SELECT status FROM releases WHERE id=$1 FOR UPDATE",
            params![release],
        )
        .await?;
        if status == "published" {
            tx.commit().await?;
            return Ok(());
        }
        ensure!(status == "publishing", "release publication is not active");
        let active:bool=sql::scalar(&mut tx,"SELECT EXISTS(SELECT 1 FROM release_publication_leases WHERE release_id=$1 AND generation=$2 AND expires_at>clock_timestamp() FOR UPDATE)",params![release,generation]).await?;
        ensure!(active, "release publisher lease expired or superseded");
        sql::execute(
            &mut tx,
            "UPDATE releases SET status='publish_failed',publish_error=$2 WHERE id=$1",
            params![release, error],
        )
        .await?;
        sql::execute(&mut tx,"UPDATE builds SET publish_status='failed',publish_error=$2 WHERE id=(SELECT build_id FROM release_deliveries WHERE release_id=$1)",params![release,error]).await?;
        sql::execute(&mut tx,"INSERT INTO publish_attempts(id,build_id,status,error) SELECT $2,build_id,'failed',$3 FROM release_deliveries WHERE release_id=$1",params![release,Uuid::new_v4(),error]).await?;
        tx.commit().await?;
        Ok(())
    }
    pub(crate) async fn pending_publications(&self) -> Result<Vec<Uuid>> {
        Ok(sql::scalars(&mut self.pool.clone(),"SELECT d.build_id FROM release_deliveries d JOIN releases r ON r.id=d.release_id WHERE r.status='publishing' ORDER BY d.created_at LIMIT 10",params![]).await?)
    }
}

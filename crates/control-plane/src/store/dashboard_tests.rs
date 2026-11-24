//! [INPUT]: 依赖隔离 PostgreSQL、固定读取时刻和真实概览 SQL。
//! [OUTPUT]: 验证全量统计、状态分母、活动去重和夏令时日期桶。
//! [POS]: 概览存储回归；固定时刻只由本模块传入私有查询入口。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use crate::auth::Auth;
use artifactory_database::{PoolSettings, connect, with_options};

struct Fixture {
    admin: toasty::Db,
    db: toasty::Db,
    schema: String,
    store: ControlStore,
}

impl Fixture {
    async fn new() -> anyhow::Result<Self> {
        let url = std::env::var("ARTIFACTORY_TEST_DATABASE_URL")?;
        let admin = connect(&url, Default::default(), PoolSettings::new(1)).await?;
        let schema = format!("dashboard_{}", Uuid::new_v4().simple());
        sql::execute(
            &mut admin.clone(),
            &format!("CREATE SCHEMA {schema}"),
            params![],
        )
        .await?;
        let db = connect(
            &with_options(&url, &[("search_path", &schema)])?,
            super::super::models(),
            PoolSettings::new(2),
        )
        .await?;
        artifactory_database::migrate(&db, &super::super::MIGRATIONS).await?;
        let store = ControlStore::new(db.clone());
        store
            .bootstrap_admin("dashboard-owner".into(), "unused".into())
            .await?;
        Ok(Self {
            admin,
            db,
            schema,
            store,
        })
    }

    async fn project(&self, name: &str) -> anyhow::Result<Uuid> {
        let project = Uuid::new_v4();
        sql::execute(
            &mut self.db.clone(),
            "INSERT INTO projects(id,tenant_id,name,repository_url) VALUES($1,$2,$3,'https://example.com/repo.git')",
            params![project, Uuid::from_u128(1), name],
        ).await?;
        sql::execute(
            &mut self.db.clone(),
            "INSERT INTO releases(id,project_id,version) VALUES(gen_random_uuid(),$1,'1.0.0')",
            params![project],
        )
        .await?;
        Ok(project)
    }

    async fn builds(
        &self,
        project: Uuid,
        status: &str,
        created: &str,
        finished: Option<&str>,
        count: i32,
    ) -> anyhow::Result<()> {
        sql::execute(
            &mut self.db.clone(),
            "INSERT INTO builds(id,project_id,requested_ref,status,source_repository_url,release_id,release_version,created_at,finished_at)
             SELECT gen_random_uuid(),$1,'main',$2,'https://example.com/repo.git',r.id,r.version,$3::timestamptz,$4::timestamptz
             FROM releases r CROSS JOIN generate_series(1,$5::int) WHERE r.project_id=$1",
            params![project,status,created,finished,count],
        ).await?;
        Ok(())
    }

    fn visible(&self, projects: Vec<Uuid>) -> ControlStore {
        let mut auth = Auth::scoped(Uuid::new_v4(), "viewer", projects, vec!["read".into()]);
        auth.tenant_id = Uuid::from_u128(1);
        self.store.for_auth(&auth)
    }

    async fn close(self) -> anyhow::Result<()> {
        sql::execute(
            &mut self.admin.clone(),
            &format!("DROP SCHEMA {} CASCADE", self.schema),
            params![],
        )
        .await?;
        Ok(())
    }
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn dashboard_counts_full_window_and_latest_completed_activity() -> anyhow::Result<()> {
    let f = Fixture::new().await?;
    let project = f.project("active").await?;
    let archived = f.project("archived").await?;
    let hidden = f.project("hidden").await?;
    f.builds(
        project,
        "succeeded",
        "2026-11-10T10:00:00Z",
        Some("2026-11-10T10:10:00Z"),
        105,
    )
    .await?;
    for status in ["failed", "lost", "canceled", "running"] {
        f.builds(project, status, "2026-11-23T10:00:00Z", None, 1)
            .await?;
    }
    f.builds(project, "queued", "2026-10-01T10:00:00Z", None, 1)
        .await?;
    f.builds(
        archived,
        "succeeded",
        "2026-10-01T10:00:00Z",
        Some("2026-11-24T10:00:00Z"),
        1,
    )
    .await?;
    f.builds(hidden, "failed", "2026-11-23T10:00:00Z", None, 50)
        .await?;
    sql::execute(
        &mut f.db.clone(),
        "UPDATE projects SET status='archived',archived_at=now() WHERE id=$1",
        params![archived],
    )
    .await?;
    let data = f
        .visible(vec![project, archived])
        .dashboard_at(
            None,
            30,
            "Asia/Shanghai",
            Some("2026-11-24T12:00:00Z".into()),
        )
        .await?;
    assert_eq!(data.summary.total_projects, 2);
    assert_eq!(data.summary.active_projects, 2);
    assert_eq!(data.summary.builds_created, 109);
    assert_eq!(data.summary.succeeded, 105);
    assert_eq!(data.summary.failed, 2);
    assert_eq!(data.summary.canceled, 1);
    assert_eq!(data.summary.ongoing, 1);
    assert_eq!(data.summary.current_ongoing, 2);
    assert_eq!(data.summary.median_turnaround_seconds, Some(600.0));
    assert!((data.summary.success_rate.unwrap() - 105.0 / 107.0).abs() < 1e-12);
    assert_eq!(data.daily.len(), 30);
    assert_eq!(data.daily.iter().map(|day| day.total).sum::<i64>(), 109);
    assert_eq!(
        data.daily
            .iter()
            .find(|day| day.date == "2026-11-10")
            .unwrap()
            .succeeded,
        105
    );
    assert_eq!(data.recent_projects.len(), 2);
    assert_eq!(data.recent_projects[0].id, archived);
    assert_eq!(data.recent_projects[0].status, "archived");
    assert_eq!(data.recent_builds.len(), 5);
    assert_eq!(data.recent_builds[0].project_id, archived);
    assert_eq!(data.window.end, data.window.generated_at);
    let scoped = f
        .visible(vec![project, archived])
        .dashboard_at(
            Some(archived),
            30,
            "UTC",
            Some("2026-11-24T12:00:00Z".into()),
        )
        .await?;
    assert_eq!(scoped.summary.total_projects, 1);
    assert_eq!(scoped.summary.builds_created, 0);
    assert_eq!(scoped.summary.active_projects, 1);
    assert_eq!(scoped.recent_projects[0].id, archived);
    f.close().await
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn dashboard_natural_days_cover_both_dst_transitions() -> anyhow::Result<()> {
    let f = Fixture::new().await?;
    let project = f.project("dst").await?;
    for created in [
        "2026-11-01T04:30:00Z",
        "2026-11-02T04:30:00Z",
        "2026-11-02T05:30:00Z",
        "2026-03-08T05:30:00Z",
        "2026-03-09T03:30:00Z",
        "2026-03-09T04:30:00Z",
    ] {
        f.builds(project, "canceled", created, None, 1).await?;
    }
    let store = f.visible(vec![project]);
    for (at, first, day, next) in [
        (
            "2026-11-03T12:00:00Z",
            "2026-10-28",
            "2026-11-01",
            "2026-11-02",
        ),
        (
            "2026-03-10T12:00:00Z",
            "2026-03-04",
            "2026-03-08",
            "2026-03-09",
        ),
    ] {
        let data = store
            .dashboard_at(None, 7, "America/New_York", Some(at.into()))
            .await?;
        assert_eq!(data.daily.len(), 7);
        assert_eq!(data.daily[0].date, first);
        assert_eq!(
            data.daily
                .iter()
                .find(|bucket| bucket.date == day)
                .unwrap()
                .total,
            2
        );
        assert_eq!(
            data.daily
                .iter()
                .find(|bucket| bucket.date == next)
                .unwrap()
                .total,
            1
        );
        assert_eq!(data.summary.builds_created, 3);
        assert_eq!(data.summary.success_rate, None);
        assert_eq!(data.summary.median_turnaround_seconds, None);
        assert!(
            data.daily
                .iter()
                .all(|day| day.median_turnaround_seconds.is_none())
        );
    }
    f.close().await
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn dashboard_empty_scopes_and_parameter_validation() -> anyhow::Result<()> {
    let f = Fixture::new().await?;
    let project = f.project("empty").await?;
    let store = f.visible(vec![]);
    let data = store.dashboard(None, 90, "UTC").await?;
    assert_eq!(data.daily.len(), 90);
    assert_eq!(data.summary.total_projects, 0);
    assert_eq!(data.summary.builds_created, 0);
    assert!(data.recent_projects.is_empty());
    assert!(data.recent_builds.is_empty());
    assert_eq!(data.summary.success_rate, None);
    assert!(matches!(
        store.dashboard(None, 8, "UTC").await,
        Err(ApiError::BadRequest(_))
    ));
    assert!(matches!(
        store.dashboard(None, 30, "Not/AZone").await,
        Err(ApiError::BadRequest(_))
    ));
    let empty = f.visible(vec![project]).dashboard(None, 7, "UTC").await?;
    assert_eq!(empty.summary.total_projects, 1);
    assert!(empty.daily.iter().all(|day| day.total == 0));
    f.close().await
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn dashboard_median_uses_only_successful_turnaround_samples() -> anyhow::Result<()> {
    let f = Fixture::new().await?;
    let project = f.project("median").await?;
    for finished in [
        "2026-11-23T10:01:40Z",
        "2026-11-23T10:03:20Z",
        "2026-11-23T10:16:40Z",
    ] {
        f.builds(
            project,
            "succeeded",
            "2026-11-23T10:00:00Z",
            Some(finished),
            1,
        )
        .await?;
    }
    for status in ["failed", "canceled"] {
        f.builds(
            project,
            status,
            "2026-11-23T10:00:00Z",
            Some("2026-11-23T11:00:00Z"),
            1,
        )
        .await?;
    }
    let data = f
        .visible(vec![project])
        .dashboard_at(None, 7, "UTC", Some("2026-11-24T12:00:00Z".into()))
        .await?;
    assert_eq!(data.summary.median_turnaround_seconds, Some(200.0));
    assert_eq!(data.summary.success_rate, Some(0.75));
    assert_eq!(
        data.daily
            .iter()
            .find(|day| day.date == "2026-11-23")
            .unwrap()
            .median_turnaround_seconds,
        Some(200.0)
    );
    f.close().await
}

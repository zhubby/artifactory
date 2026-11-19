//! [INPUT]: 依赖全新业务基线和隔离 PostgreSQL。
//! [OUTPUT]: 验证规划意图与取消版本的原子提交。
//! [POS]: 唯一 Kubernetes 规划路径的 outbox 回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;

#[tokio::test]
#[ignore = "requires isolated PostgreSQL"]
async fn planning_outbox_is_atomic_and_cancel_is_a_new_version() {
    let db = Database::with_tenant().await;
    let (_, build, _) = db.fixture().await;
    let run = Uuid::new_v4();
    let mut pool = db.pool.clone();
    let mut tx = pool.transaction().await.unwrap();
    sql::execute(
        &mut tx,
        "INSERT INTO planning_runs(id,build_id,purpose,model_config) VALUES($1,$2,'initial','{}')",
        params![run, build],
    )
    .await
    .unwrap();
    assert_eq!(
        sql::scalar::<i64>(
            &mut tx,
            "SELECT count(*) FROM planning_intents WHERE run_id=$1",
            params![run]
        )
        .await
        .unwrap(),
        1
    );
    tx.rollback().await.unwrap();
    assert_eq!(
        sql::scalar::<i64>(
            &mut db.pool.clone(),
            "SELECT count(*) FROM planning_intents WHERE run_id=$1",
            params![run]
        )
        .await
        .unwrap(),
        0
    );
    sql::execute(
        &mut db.pool.clone(),
        "INSERT INTO planning_runs(id,build_id,purpose,model_config) VALUES($1,$2,'initial','{}')",
        params![run, build],
    )
    .await
    .unwrap();
    for _ in 0..2 {
        sql::execute(
            &mut db.pool.clone(),
            "UPDATE planning_runs SET state='canceled' WHERE id=$1",
            params![run],
        )
        .await
        .unwrap();
    }
    let versions: Vec<i64> = sql::scalars(
        &mut db.pool.clone(),
        "SELECT version FROM planning_intents WHERE run_id=$1 ORDER BY version",
        params![run],
    )
    .await
    .unwrap();
    assert_eq!(versions, vec![1, 2]);
    db.close().await;
}

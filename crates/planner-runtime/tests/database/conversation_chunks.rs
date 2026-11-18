//! [INPUT]: 依赖隔离 PostgreSQL、真实提案和持久会话分页。
//! [OUTPUT]: 验证大审计分片、原子写入与幂等拒绝。
//! [POS]: 覆盖 Runtime 内部审计与 Worker 分片的统一写入边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn oversized_proposals_and_results_keep_complete_paginated_audit() {
    let s = state().await;
    let run = Uuid::new_v4();
    let input = intent(run);
    server::put_intent(&s, serde_json::to_value(&input).unwrap())
        .await
        .unwrap();
    let lease = owner(&s, run).await;
    let proposal = Uuid::new_v4();
    let text = "源码🦀\"\\\n\t\u{0001}".repeat(100_000);
    let arguments = json!({"plan":{"script":text}});
    assert!(serde_json::to_vec(&arguments).unwrap().len() > 1024 * 1024);
    let request =
        json!({"lease":lease,"request_id":proposal,"name":"submit_plan","arguments":arguments});
    server::tool_call(&s, request.clone()).await.unwrap();
    server::tool_call(&s, request.clone()).await.unwrap();
    let mut conflict = request;
    conflict["arguments"] = json!({"plan":"conflicting"});
    assert!(server::tool_call(&s, conflict).await.is_err());
    let delivery = s
        .store
        .deliveries()
        .await
        .unwrap()
        .into_iter()
        .find(|v| v.kind == "proposal")
        .unwrap();
    let result = json!({"accepted":false,"reason":text});
    s.store
        .acknowledge_delivery(delivery.id, run, &delivery.kind, &delivery.payload, &result)
        .await
        .unwrap();
    s.store
        .acknowledge_delivery(delivery.id, run, &delivery.kind, &delivery.payload, &result)
        .await
        .unwrap();
    let worker_chunk = json!({"message_id":Uuid::new_v4(),"index":0,"total":1,"kind":"message","text":"转义\"\\\n\t\u{0001}".repeat(20_000)});
    let event = json!({"lease":lease,"event_id":Uuid::new_v4(),"sequence":1,"kind":"conversation","payload":{"kind":"message_chunk","content":worker_chunk}});
    server::report_event(&s, event.clone()).await.unwrap();
    server::report_event(&s, event).await.unwrap();
    let tail = json!({"role":"assistant","content":"audit tail after oversized proposal"});
    server::report_event(&s,json!({"lease":lease,"event_id":Uuid::new_v4(),"sequence":2,"kind":"conversation","payload":{"kind":"message","content":tail}})).await.unwrap();
    let scope = json!({"run_id":run,"tenant_id":input.tenant_id,"project_id":input.project_id,"generation":1,"limit":100});
    let mut after = 0;
    let mut rows = Vec::new();
    let mut pages = 0;
    loop {
        let mut query = scope.clone();
        query["after"] = json!(after);
        let page = s.store.conversation_messages(query).await.unwrap();
        assert!(
            serde_json::to_vec(&page).unwrap().len() <= 1024 * 1024,
            "page exceeds wire budget"
        );
        let messages = page["messages"].as_array().unwrap();
        assert!(
            !messages.is_empty(),
            "oversized first row hid audit history"
        );
        for row in messages {
            assert!(row["sequence"].as_i64().unwrap() > after);
            assert!(serde_json::to_vec(row).unwrap().len() < 1024 * 1024);
        }
        after = messages.last().unwrap()["sequence"].as_i64().unwrap();
        rows.extend(messages.iter().cloned());
        pages += 1;
        if page["next_cursor"].is_null() {
            break;
        }
        assert_eq!(page["next_cursor"], after);
        assert!(pages < 100);
    }
    assert!(pages > 2, "large audit must cross multiple pages");
    let mut reconstructed =
        std::collections::BTreeMap::<String, (String, usize, Vec<(usize, String)>)>::new();
    let mut unchanged = 0;
    for row in &rows {
        if row["payload"] == worker_chunk {
            unchanged += 1;
            continue;
        }
        if row["kind"] != "message_chunk" {
            assert_eq!(row["payload"], tail);
            continue;
        }
        let p = &row["payload"];
        let entry = reconstructed
            .entry(p["message_id"].as_str().unwrap().into())
            .or_insert_with(|| {
                (
                    p["kind"].as_str().unwrap().into(),
                    p["total"].as_u64().unwrap() as usize,
                    Vec::new(),
                )
            });
        entry.2.push((
            p["index"].as_u64().unwrap() as usize,
            p["text"].as_str().unwrap().into(),
        ));
    }
    assert_eq!(
        unchanged, 1,
        "Worker chunks must remain unwrapped and idempotent"
    );
    let mut originals = std::collections::BTreeMap::new();
    for (_, (kind, total, mut parts)) in reconstructed {
        parts.sort_by_key(|p| p.0);
        assert_eq!(parts.len(), total);
        assert_eq!(
            parts.iter().map(|p| p.0).collect::<Vec<_>>(),
            (0..total).collect::<Vec<_>>()
        );
        let text = parts.into_iter().map(|p| p.1).collect::<String>();
        originals.insert(kind, serde_json::from_str::<Value>(&text).unwrap());
    }
    assert_eq!(originals.len(), 2);
    assert_eq!(originals["proposal_submitted"]["arguments"], arguments);
    assert_eq!(originals["proposal_result"]["result"], result);
    assert_eq!(rows.last().unwrap()["payload"], tail);
    let first_chunks:Vec<Value>=sql::scalars(&mut s.pool.clone(),"SELECT jsonb_build_object('event_id',event_id,'input_hash',input_hash,'payload',payload) FROM runtime_messages WHERE run_id=$1 AND kind='message_chunk' AND payload->>'index'='0' AND payload->>'kind' IN ('proposal_submitted','proposal_result')",params![run]).await.unwrap();
    assert_eq!(first_chunks.len(), 2);
    for chunk in first_chunks {
        let kind = chunk["payload"]["kind"].as_str().unwrap();
        assert_eq!(chunk["event_id"], chunk["payload"]["message_id"]);
        assert_eq!(
            chunk["input_hash"],
            crypto::hash(
                serde_json::to_vec(&json!({"kind":kind,"payload":originals[kind]})).unwrap()
            )
        );
    }
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn oversized_audit_failure_rolls_back_all_chunks_and_proposal() {
    let s = state().await;
    let run = Uuid::new_v4();
    server::put_intent(&s, serde_json::to_value(intent(run)).unwrap())
        .await
        .unwrap();
    let lease = owner(&s, run).await;
    sql::execute(&mut s.pool.clone(),"CREATE FUNCTION reject_audit_chunk() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.kind='message_chunk' AND (NEW.payload->>'index')::int=1 THEN RAISE EXCEPTION 'fixture chunk failure'; END IF; RETURN NEW; END $$",params![]).await.unwrap();
    sql::execute(&mut s.pool.clone(),"CREATE TRIGGER reject_audit_chunk BEFORE INSERT ON runtime_messages FOR EACH ROW EXECUTE FUNCTION reject_audit_chunk()",params![]).await.unwrap();
    let request = json!({"lease":lease,"request_id":Uuid::new_v4(),"name":"submit_plan","arguments":{"script":"🦀\"\n".repeat(200_000)}});
    assert!(
        server::tool_call(&s, request.clone()).await.is_err(),
        "second audit chunk failure must reject proposal"
    );
    for table in ["runtime_messages", "runtime_proposals", "runtime_outbox"] {
        let count: i64 = sql::scalar(
            &mut s.pool.clone(),
            &format!("SELECT count(*) FROM {table} WHERE run_id=$1"),
            params![run],
        )
        .await
        .unwrap();
        assert_eq!(count, 0, "{table} escaped rollback");
    }
    assert_eq!(
        server::read_run(&s, run).await.unwrap()["lifecycle"]["pending"],
        false
    );
    sql::execute(
        &mut s.pool.clone(),
        "DROP TRIGGER reject_audit_chunk ON runtime_messages",
        params![],
    )
    .await
    .unwrap();
    server::tool_call(&s, request).await.unwrap();
}

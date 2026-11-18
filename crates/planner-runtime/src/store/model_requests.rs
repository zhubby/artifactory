//! [INPUT]: 依赖 run grant、累计预算和模型协议。
//! [OUTPUT]: 原子预留模型调用与共享 RPM 时隙。
//! [POS]: 模型代理的计费幂等边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
impl RuntimeStore {
    pub async fn reserve_model(&self, request: ModelRequest<'_>) -> Result<ModelReservation> {
        let ModelRequest {
            run,
            generation,
            lease,
            request_id,
            path,
            value,
            hash,
        } = request;
        let mut tx_db = self.pool.clone();
        let mut tx = tx_db.transaction().await?;
        let row = locked(&mut tx, run).await?;
        let mut life = authorized(&row, lease)?;
        let cfg = config(self, &row)?.model;
        let expected = match cfg.api.as_str() {
            "openai-completions" => "v1/chat/completions",
            "openai-responses" => "v1/responses",
            "anthropic-messages" => "v1/messages",
            _ => "",
        };
        ensure!(
            path == expected && value["model"] == cfg.model,
            "model endpoint or identity mismatch"
        );
        ensure!(
            !value
                .as_object()
                .is_some_and(|o| o.contains_key("base_url") || o.contains_key("api_key")),
            "invalid model request"
        );
        if let Some(prior) = sql::optional_record(
            &mut tx,
            "SELECT input_hash FROM runtime_model_requests WHERE run_id=$1 AND request_id=$2",
            params![run, request_id],
            &[("input_hash", Type::String)],
        )
        .await?
        {
            ensure!(
                prior.decode::<String>("input_hash")? == hash,
                "model request ID reused"
            );
            return Ok(ModelReservation::Duplicate);
        }
        life.reserve(generation, &lease.uid, Utc::now().timestamp_millis())?;
        models::Profile::upsert_by_id(cfg.id)
            .or_ignore()
            .exec(&mut tx)
            .await?;
        let rpm = cfg.parameters["requests_per_minute"].as_u64().unwrap_or(0);
        let wait: i64 = if rpm > 0 {
            sql::scalar(&mut tx, "UPDATE runtime_profiles SET next_call_at=greatest(next_call_at,clock_timestamp())+make_interval(secs=>$2) WHERE id=$1 RETURNING greatest(0,ceil(extract(epoch FROM next_call_at-clock_timestamp())*1000-$2*1000)::bigint)", params![cfg.id, 60.0/rpm as f64+1.0]).await?
        } else {
            0
        };
        ensure!(
            Utc::now().timestamp_millis() + wait < life.deadline_ms.unwrap(),
            "profile queue exceeds deadline"
        );
        models::ModelRequest::create()
            .run_id(run)
            .request_id(request_id)
            .input_hash(hash)
            .generation(generation)
            .exec(&mut tx)
            .await?;
        save(&mut tx, run, &life).await?;
        tx.commit().await?;
        Ok(ModelReservation::Reserved {
            config: cfg,
            deadline_ms: life.deadline_ms.unwrap(),
            wait_ms: wait,
            endpoint: expected.into(),
        })
    }

    pub async fn model_response_received(&self, run: Uuid, request_id: Uuid) -> Result<()> {
        sql::execute(
            &mut self.pool.clone(),
            "UPDATE runtime_model_requests SET state='response' WHERE run_id=$1 AND request_id=$2",
            params![run, request_id],
        )
        .await?;
        Ok(())
    }
}

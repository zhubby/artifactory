//! [INPUT]: 依赖事务通知事件、收件箱投影、渠道适配器和应用停止信号。
//! [OUTPUT]: 提供事件目录和独立通知后台运行时。
//! [POS]: 控制平面内的通知基础设施；网络投递不占业务事务。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
pub mod catalog;
pub mod channels;

/// 收件箱、外发和扫描分别运行，外部网络故障不阻塞收件箱。
pub fn start(state: crate::state::AppState) {
    use std::time::Duration;
    let inbox = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = inbox.coordination.stop.cancelled() => break,
                _ = interval.tick() => {},
            }
            if let Err(_error) = inbox.store.project_notifications(128).await {
                tracing::warn!("notification inbox projection deferred");
            }
        }
    });
    for channel in ["verification", "email", "webhook"] {
        let delivery = state.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = delivery.coordination.stop.cancelled() => break,
                    _ = interval.tick() => {},
                }
                tokio::select! {
                    _ = delivery.coordination.stop.cancelled() => break,
                    result = channels::tick_channel(&delivery, channel) => {
                        if result.is_err() { tracing::warn!(channel, "notification channel reconciliation deferred"); }
                    }
                }
            }
        });
    }
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = state.coordination.stop.cancelled() => break,
                _ = interval.tick() => {},
            }
            if crate::store::notifications::maintenance::tick(&state)
                .await
                .is_err()
            {
                tracing::warn!("notification anomaly reconciliation deferred");
            }
        }
    });
}

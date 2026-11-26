//! [INPUT]: 依赖共享命令目录与当前连接的能力确认。
//! [OUTPUT]: 校验命令，拒收执行请求并累计显式可忽略通知。
//! [POS]: Runner入口；重连清除确认，进程内计数保留。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::protocol as pb;
use artifactory_emissary_protocol::commands::{self, DecodedCommand};
use std::time::Instant;

pub(super) enum Admission {
    Proceed,
    Consumed,
    Ignored,
    Rejected(Option<pb::EmissaryEvent>),
}
#[derive(Default)]
pub(super) struct Gate {
    accepted: bool,
    pub ota_observability: bool,
    pub started: Option<Instant>,
    pub received: Option<Instant>,
    pub server_time_ms: Option<i64>,
    pub control_session: Option<uuid::Uuid>,
    pub ignored_notifications: u64,
}
impl Gate {
    pub fn reset(&mut self, now: Instant) {
        *self = Self {
            started: Some(now),
            ignored_notifications: self.ignored_notifications,
            ..Default::default()
        };
    }
    pub fn admit(&mut self, command: &pb::Command, node: &str) -> Admission {
        match commands::decode(command) {
            Ok(DecodedCommand::IgnoredNotification) => {
                self.ignored_notifications = self.ignored_notifications.saturating_add(1);
                Admission::Ignored
            }
            Ok(DecodedCommand::CapabilitiesAck(ack)) => {
                // 当前流的确认不从磁盘恢复，也不跨重连继承。
                self.accepted = ack.accepts_rejections();
                self.ota_observability = ack
                    .responses
                    .iter()
                    .any(|r| r == artifactory_emissary_protocol::ota::CAPABILITY);
                self.server_time_ms = Some(ack.server_time_ms);
                self.received = Some(Instant::now());
                self.control_session = uuid::Uuid::parse_str(&command.signing_session_id).ok();
                Admission::Consumed
            }
            Ok(_) => Admission::Proceed,
            Err(rejection) => Admission::Rejected(self.accepted.then(|| pb::EmissaryEvent {
                emissary_id: node.into(),
                kind: "command_rejected".into(),
                command_id: rejection.command_id.clone(),
                protocol_major: 1,
                payload_json: serde_json::to_string(&rejection).expect("typed rejection"),
                ..Default::default()
            })),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicitly_ignorable_notification_never_proceeds_to_an_executor() {
        let mut gate = Gate::default();
        let notification = pb::Command {
            kind: "future_notice".into(),
            ignorable_notification: true,
            ..Default::default()
        };
        let mut ordinary = notification.clone();
        ordinary.ignorable_notification = false;
        assert!(matches!(
            gate.admit(&ordinary, "node"),
            Admission::Rejected(None)
        ));
        assert_eq!(gate.ignored_notifications, 0);
        assert!(matches!(
            gate.admit(&notification, "node"),
            Admission::Ignored
        ));
        assert_eq!(gate.ignored_notifications, 1);
        let invalid = pb::Command {
            assignment: Some(Default::default()),
            ..notification.clone()
        };
        assert!(matches!(
            gate.admit(&invalid, "node"),
            Admission::Rejected(None)
        ));
        assert_eq!(gate.ignored_notifications, 1, "执行信封不计为可忽略通知");
        gate.reset(Instant::now());
        assert_eq!(gate.ignored_notifications, 1, "重连不能抹除同进程通知计数");
        gate.ignored_notifications = u64::MAX;
        assert!(matches!(
            gate.admit(&notification, "node"),
            Admission::Ignored
        ));
        assert_eq!(gate.ignored_notifications, u64::MAX, "计数饱和而不溢出");
    }
    #[test]
    fn rejections_require_this_connections_ack_and_reset_on_reconnect() {
        let mut gate = Gate::default();
        let unknown = pb::Command {
            kind: "future_kind".into(),
            command_id: "id".into(),
            ..Default::default()
        };
        assert!(matches!(
            gate.admit(&unknown, "node"),
            Admission::Rejected(None)
        ));
        let ack = pb::Command { kind: "capabilities_ack".into(), payload_json: serde_json::json!({"schema_version":1,"server_time_ms":1,"responses":["command_rejected"]}).to_string(), ..Default::default() };
        assert!(matches!(gate.admit(&ack, "node"), Admission::Consumed));
        let Admission::Rejected(Some(event)) = gate.admit(&unknown, "node") else {
            panic!("confirmed peer requires rejection");
        };
        assert_eq!(event.kind, "command_rejected");
        assert_eq!(event.attempt_id, "");
        gate.reset(Instant::now());
        assert!(matches!(
            gate.admit(&unknown, "node"),
            Admission::Rejected(None)
        ));
    }
}

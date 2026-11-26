//! [INPUT]: 构建与设备命令的真实protobuf信封。
//! [OUTPUT]: 验证静态目录和严格类型解码的拒绝边界。
//! [POS]: 命令字符串边界的跨模块契约回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_emissary_protocol::{
    Command,
    commands::{self, DecodedCommand, RejectionCode},
};
use serde_json::json;
use uuid::Uuid;

#[test]
fn only_explicit_empty_execution_envelope_can_ignore_unknown_notification() {
    let notification = Command {
        kind: "future_notice".into(),
        command_id: "notice-id".into(),
        payload_json: r#"{"value":1}"#.into(),
        ignorable_notification: true,
        ..Default::default()
    };
    assert!(matches!(
        commands::decode(&notification).unwrap(),
        DecodedCommand::IgnoredNotification
    ));
    let mut ordinary = notification.clone();
    ordinary.ignorable_notification = false;
    assert_eq!(
        commands::decode(&ordinary).unwrap_err().code,
        RejectionCode::UnsupportedKind
    );
    for execution in [
        Command {
            assignment: Some(Default::default()),
            ..notification.clone()
        },
        Command {
            attempt_id: "attempt".into(),
            ..notification.clone()
        },
        Command {
            lease_token: "token".into(),
            ..notification.clone()
        },
        Command {
            lease_seconds: 1,
            ..notification.clone()
        },
        Command {
            signing_session_id: "authority".into(),
            ..notification.clone()
        },
        Command {
            request_id: "anchor".into(),
            ..notification.clone()
        },
        Command {
            ack_sequence: 1,
            ..notification.clone()
        },
        Command {
            message: "lease_invalid".into(),
            ..notification.clone()
        },
        Command {
            kind: String::new(),
            ..notification.clone()
        },
        Command {
            kind: "future\nnotice".into(),
            ..notification.clone()
        },
        Command {
            payload_json: "x".repeat(commands::MAX_COMMAND_JSON_BYTES + 1),
            ..notification.clone()
        },
    ] {
        assert_eq!(
            commands::decode(&execution).unwrap_err().code,
            RejectionCode::InvalidPayload
        );
    }
    let known_invalid = Command {
        kind: "terminal_open".into(),
        payload_json: "{}".into(),
        ..notification.clone()
    };
    assert_eq!(
        commands::decode(&known_invalid).unwrap_err().code,
        RejectionCode::InvalidPayload
    );
    let known = Command {
        kind: "assign".into(),
        assignment: Some(Default::default()),
        ..notification
    };
    assert!(matches!(
        commands::decode(&known).unwrap(),
        DecodedCommand::Build { .. }
    ));
}

#[test]
fn terminal_semantics_are_rejected_before_executor_admission() {
    let valid = json!({
        "terminal_id":Uuid::new_v4(),"node_session":Uuid::new_v4(),
        "startup_id":Uuid::new_v4(),"expires_at_ms":15_000,
        "columns":80,"rows":24,"term":"xterm-256color","profile":"default"
    });
    for (field, value) in [
        ("columns", json!(0)),
        ("rows", json!(501)),
        ("terminal_id", json!(Uuid::nil())),
        ("node_session", json!(Uuid::nil())),
        ("startup_id", json!(Uuid::nil())),
        ("profile", json!("/bin/sh")),
        ("term", json!("xterm;invalid")),
    ] {
        let mut payload = valid.clone();
        payload[field] = value;
        let command = Command {
            kind: "terminal_open".into(),
            payload_json: payload.to_string(),
            ..Default::default()
        };
        assert_eq!(
            commands::decode(&command).unwrap_err().code,
            RejectionCode::InvalidPayload,
            "{field} must be rejected before the terminal supervisor"
        );
    }
    let command = Command {
        kind: "terminal_open".into(),
        payload_json: valid.to_string(),
        ..Default::default()
    };
    assert!(
        matches!(
            commands::decode(&command).unwrap(),
            DecodedCommand::TerminalOpen(_)
        ),
        "clock and local admission remain the executor's responsibility"
    );
}

#[test]
fn operation_schema_mismatch_is_distinct_from_invalid_input() {
    use artifactory_emissary_protocol::operations::*;
    let input =
        validate_input(OperationKind::CollectDiagnostics, 1, &json!({"probes":[]})).unwrap();
    let mut envelope = OperationEnvelope {
        operation_id: Uuid::new_v4(),
        tenant_id: Uuid::new_v4(),
        node_id: Uuid::new_v4(),
        startup_id: Uuid::new_v4(),
        control_session: Uuid::new_v4(),
        actor_id: Uuid::new_v4(),
        kind: input.kind(),
        schema: 2,
        input: input.value(),
        input_digest: input.digest(),
        issued_at_ms: 1,
        expires_at_ms: 30_001,
    };
    let command = |envelope: &OperationEnvelope| Command {
        kind: "node_operation".into(),
        payload_json: serde_json::to_string(envelope).unwrap(),
        ..Default::default()
    };
    assert_eq!(
        commands::decode(&command(&envelope)).unwrap_err().code,
        RejectionCode::SchemaMismatch
    );
    envelope.schema = 1;
    envelope.input = json!({"probes":["arbitrary_shell"]});
    assert_eq!(
        commands::decode(&command(&envelope)).unwrap_err().code,
        RejectionCode::InvalidPayload
    );
}

#[test]
fn known_commands_decode_without_changing_build_lifecycle() {
    let command = Command {
        kind: "assign".into(),
        assignment: Some(Default::default()),
        ..Default::default()
    };
    assert!(matches!(
        commands::decode(&command).unwrap(),
        DecodedCommand::Build { .. }
    ));
    let mut command = command;
    command.assignment = None;
    assert_eq!(
        commands::decode(&command).unwrap_err().code,
        RejectionCode::InvalidPayload
    );
    assert_eq!(
        commands::catalog()
            .iter()
            .filter(|entry| entry.kind == "assign")
            .count(),
        1
    );
}

#[test]
fn unknown_or_malformed_commands_have_machine_readable_rejections() {
    let command = Command {
        command_id: "request".into(),
        kind: "run_anything".into(),
        ..Default::default()
    };
    let rejected = commands::decode(&command).unwrap_err();
    assert_eq!(rejected.command_id, "request");
    assert_eq!(rejected.code, RejectionCode::UnsupportedKind);
    let command = Command {
        kind: "terminal_close".into(),
        payload_json: "{}".into(),
        ..Default::default()
    };
    assert_eq!(
        commands::decode(&command).unwrap_err().code,
        RejectionCode::InvalidPayload
    );
    let command = Command {
        kind: "capabilities_ack".into(),
        payload_json: r#"{"schema_version":2,"server_time_ms":10,"responses":[]}"#.into(),
        ..Default::default()
    };
    assert_eq!(
        commands::decode(&command).unwrap_err().code,
        RejectionCode::SchemaMismatch
    );
}

#[test]
fn capability_ack_requires_explicit_command_rejection_response() {
    let command = Command {
        kind: "capabilities_ack".into(),
        payload_json:
            r#"{"schema_version":1,"server_time_ms":10,"responses":["command_rejected"]}"#.into(),
        ..Default::default()
    };
    let DecodedCommand::CapabilitiesAck(ack) = commands::decode(&command).unwrap() else {
        panic!("typed capability ACK required")
    };
    assert!(ack.accepts_rejections());
    let command = Command {
        payload_json: r#"{"schema_version":1,"server_time_ms":10,"responses":[]}"#.into(),
        ..command
    };
    let DecodedCommand::CapabilitiesAck(ack) = commands::decode(&command).unwrap() else {
        panic!("typed capability ACK required")
    };
    assert!(!ack.accepts_rejections());
}

#[test]
fn typed_command_debug_does_not_expose_ota_tickets_or_build_environment() {
    let command = Command {
        kind: "emissary_update".into(),
        payload_json: r#"{"generation":1,"id":"id","action":"apply","manifest":"manifest","signature":"signature","url":"https://download.invalid/?token=private"}"#.into(),
        ..Default::default()
    };
    assert!(!format!("{:?}", commands::decode(&command).unwrap()).contains("private"));
    let command = Command {
        kind: "assign".into(),
        assignment: Some(artifactory_emissary_protocol::Assignment {
            environment: [("SECRET".into(), "private".into())].into(),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert!(!format!("{:?}", commands::decode(&command).unwrap()).contains("private"));
}

#[test]
fn node_operation_resume_requires_frozen_identity_and_never_executes_work() {
    let descriptor = commands::catalog()
        .iter()
        .find(|entry| entry.kind == "node_operation_resume")
        .unwrap();
    assert!(!descriptor.executes_work);
    let command = Command {
        kind: "node_operation_resume".into(),
        payload_json: "{}".into(),
        ..Default::default()
    };
    assert_eq!(
        commands::decode(&command).unwrap_err().code,
        RejectionCode::InvalidPayload
    );
}

#[test]
fn catalog_exposes_executor_authority_and_lifecycle_without_granting_work_to_replay() {
    use commands::{AuthorizationPolicy, CancellationPolicy, DeadlinePolicy, DeduplicationPolicy};
    let entry = |kind| {
        commands::catalog()
            .iter()
            .find(|entry| entry.kind == kind)
            .unwrap()
    };
    let operation = entry("node_operation");
    assert_eq!(operation.input_schema.name, "operation_envelope");
    assert_eq!(operation.result_schema.unwrap().name, "operation_update");
    assert_eq!(
        operation.authorization,
        AuthorizationPolicy::TenantAdminSession
    );
    assert_eq!(
        operation.deadline,
        DeadlinePolicy::OperationReceiveAndExecution
    );
    assert_eq!(
        operation.deduplication,
        DeduplicationPolicy::OperationIdAndInputDigest
    );
    assert_eq!(
        operation.cancellation,
        CancellationPolicy::OperationIdWithDurableTerminal
    );
    let resume = entry("node_operation_resume");
    assert!(!resume.executes_work);
    assert_eq!(resume.input_schema.name, "operation_resume");
    assert_eq!(resume.result_schema.unwrap().name, "operation_update");
    let terminal = entry("terminal_open");
    assert_eq!(
        terminal.authorization,
        AuthorizationPolicy::TenantAdminSessionAndLocalOptIn
    );
    assert_eq!(terminal.deadline, DeadlinePolicy::TerminalOpenAndLease);
    let update = entry("emissary_update");
    assert_eq!(
        update.authorization,
        AuthorizationPolicy::TenantAdminUpdateAndSignedAsset
    );
    assert_eq!(
        update.deduplication,
        DeduplicationPolicy::UpdateGenerationAndId
    );
    let build = entry("start");
    assert_eq!(
        build.authorization,
        AuthorizationPolicy::ProjectAttemptLease
    );
    assert_eq!(build.deadline, DeadlinePolicy::AttemptLease);
    assert_eq!(
        build.deduplication,
        DeduplicationPolicy::AttemptAndLeaseToken
    );
    assert!(entry("capabilities_ack").result_schema.is_none());
    for entry in commands::catalog() {
        assert_eq!(entry.input_schema.version, entry.schema_version);
        assert!(entry.result_schema.is_none_or(|schema| schema.version == 1));
    }
}

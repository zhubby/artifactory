//! [INPUT]: 真实协商信封、回执与有限拒收原因。
//! [OUTPUT]: 验证格式交集、预算和摘要确认规则。
//! [POS]: 遥测协议新旧接收端兼容的行为回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_emissary_protocol::{
    TelemetryAck, TelemetryNegotiateRequest,
    telemetry_negotiation::{self, AckRequirement, Negotiated, RejectionReason},
};

#[test]
fn negotiation_selects_supported_intersection_without_fabricating_schema() {
    let reply = telemetry_negotiation::negotiate(&telemetry_negotiation::offer(), 42).unwrap();
    assert_eq!(reply.batch_schemas, [1]);
    assert_eq!(reply.inventory_schemas, [1]);
    assert_eq!(reply.encodings, ["identity"]);
    assert_eq!(reply.features, ["digest_ack_v1"]);
    assert_eq!(reply.server_time_ms, 42);
    let selected = Negotiated::from_reply(&reply).unwrap();
    assert_eq!(selected.batch_schema, Some(1));
    assert_eq!(selected.inventory_schema, Some(1));
    assert!(selected.digest_ack);

    let request = TelemetryNegotiateRequest {
        batch_schemas: vec![7],
        inventory_schemas: vec![9],
        encodings: vec!["unknown".into()],
        ..Default::default()
    };
    let reply = telemetry_negotiation::negotiate(&request, 0).unwrap();
    let selected = Negotiated::from_reply(&reply).unwrap();
    assert_eq!(selected.batch_schema, None);
    assert_eq!(selected.inventory_schema, None);
}

#[test]
fn invalid_negotiation_does_not_downgrade_into_legacy() {
    let mut request = telemetry_negotiation::offer();
    request.batch_schemas = vec![1; 9];
    assert_eq!(
        telemetry_negotiation::negotiate(&request, 0)
            .unwrap_err()
            .code(),
        tonic::Code::InvalidArgument
    );
    request = telemetry_negotiation::offer();
    request.features = vec!["x".repeat(4096)];
    assert!(telemetry_negotiation::negotiate(&request, 0).is_err());
    let mut reply = telemetry_negotiation::negotiate(&telemetry_negotiation::offer(), 0).unwrap();
    reply.batch_schemas = vec![999];
    assert!(Negotiated::from_reply(&reply).is_err());
    assert_eq!(Negotiated::legacy().batch_schema, Some(1));
    assert_eq!(Negotiated::legacy().inventory_schema, None);
}

#[test]
fn digest_ack_cannot_downgrade_when_backend_changes() {
    let mut ack = TelemetryAck {
        batch_id: "batch".into(),
        ..Default::default()
    };
    let required = AckRequirement::Digest { schema_version: 1 };
    assert!(AckRequirement::LegacyId.matches_ack(&ack, "batch", b"{}"));
    assert!(!required.matches_ack(&ack, "batch", b"{}"));
    ack.schema_version = 1;
    ack.sha256 = "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a".into();
    assert!(required.matches_ack(&ack, "batch", b"{}"));
    assert!(!required.matches_ack(&ack, "other", b"{}"));
    assert!(!required.matches_ack(&ack, "batch", b"{ }"));
}

#[test]
fn rejection_details_use_a_finite_machine_vocabulary() {
    for reason in RejectionReason::ALL {
        let status = reason.status();
        assert_eq!(RejectionReason::from_status(&status), Some(*reason));
        assert_eq!(status.code(), reason.code());
        assert_eq!(status.message(), reason.as_str());
    }
    assert_eq!(
        RejectionReason::from_status(&tonic::Status::invalid_argument("bad json")),
        None
    );
    let unknown = tonic::Status::with_details(
        tonic::Code::InvalidArgument,
        "new reason",
        b"unknown".as_slice().into(),
    );
    assert_eq!(RejectionReason::from_status(&unknown), None);
}

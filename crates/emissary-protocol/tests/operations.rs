// [INPUT]: 依赖有限操作输入、身份信封与结果摘要契约。
// [OUTPUT]: 验证固定探测目录、幂等摘要及结果预算。
// [POS]: 有限操作跨 Control 与设备的安全边界回归。
// [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_emissary_protocol::operations::*;
use serde_json::json;
use uuid::Uuid;

#[test]
fn rejects_arbitrary_commands_and_unknown_fields() {
    let create = |input| OperationCreate {
        request_id: Uuid::new_v4(),
        kind: OperationKind::CollectDiagnostics,
        schema: 1,
        input,
    };
    assert!(
        create(json!({"probes":["git"],"shell":"echo secret"}))
            .validated()
            .is_err()
    );
    assert!(create(json!({"probes":["/bin/sh"]})).validated().is_err());
    assert!(create(json!({"probes":["git","git"]})).validated().is_err());
    assert!(PROBE_IDS.len() <= 32);
}
#[test]
fn normalized_probe_order_has_one_input_digest() {
    let create = |input| OperationCreate {
        request_id: Uuid::new_v4(),
        kind: OperationKind::CollectDiagnostics,
        schema: 1,
        input,
    };
    assert_eq!(
        create(json!({"probes":["git","cargo"]}))
            .validated()
            .unwrap()
            .digest(),
        create(json!({"probes":["cargo","git"]}))
            .validated()
            .unwrap()
            .digest()
    );
    let bad = OperationCreate {
        request_id: Uuid::new_v4(),
        kind: OperationKind::RefreshInventory,
        schema: 1,
        input: json!({"mode":"partial"}),
    };
    assert!(bad.validated().is_err());
}
#[test]
fn partial_probe_failure_cannot_claim_success() {
    let report = ProbeReport {
        status: ProbeStatus::Timeout,
        observed_at: chrono::Utc::now(),
        duration_ms: 3000,
        probe_version: 1,
        version: None,
        error_code: Some(ProbeErrorCode::DeadlineExceeded),
    };
    let mut result = OperationResult::default();
    result.probes.insert("git".into(), report);
    assert!(!result.successful());
    let original = result.digest().unwrap();
    result.probes.get_mut("git").unwrap().status = ProbeStatus::Unsupported;
    result.probes.get_mut("git").unwrap().error_code = None;
    assert!(result.successful());
    assert_ne!(original, result.digest().unwrap());
    result.probes.get_mut("git").unwrap().version = Some("x".repeat(MAX_RESULT_BYTES));
    assert!(result.validate().is_err());
}

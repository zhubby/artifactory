//! [INPUT]: 原始清单字节、固定观察身份和旧批次样本。
//! [OUTPUT]: 验证观察摘要、身份、大小与回执绑定。
//! [POS]: 清单独立交付契约的可重放行为回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_emissary_protocol::{
    InventoryAck, InventoryObservation,
    inventory::MAX_INVENTORY_BYTES,
    telemetry::{Batch, Inventory},
};
use chrono::{TimeZone, Utc};
use uuid::Uuid;

fn observation() -> InventoryObservation {
    InventoryObservation::new(
        Uuid::from_u128(2),
        0,
        Utc.timestamp_millis_opt(1_000).unwrap(),
        17,
        &Inventory {
            os: "linux".into(),
            arch: "arm64".into(),
            ..Default::default()
        },
    )
    .unwrap()
}

#[test]
fn observation_is_bound_to_original_bytes_and_identity() {
    let observation = observation();
    assert_eq!(observation.validate().unwrap().os, "linux");
    let mut changed = observation.clone();
    changed.revision = 1;
    assert!(changed.validate().is_err());
    changed = observation.clone();
    changed.inventory_json.push(b' ');
    assert!(changed.validate().is_err());
    let mut ack = InventoryAck {
        observation_id: observation.observation_id.clone(),
        delivery_digest: observation.delivery_digest.clone(),
        ..Default::default()
    };
    assert!(observation.matches_ack(&ack));
    ack.delivery_digest = "0".repeat(64);
    assert!(!observation.matches_ack(&ack));
}

#[test]
fn malformed_oversized_and_unknown_inventory_do_not_enter_storage() {
    let mut observation = observation();
    observation.inventory_json = vec![b' '; MAX_INVENTORY_BYTES + 1];
    observation.delivery_digest = observation.computed_digest();
    assert!(observation.validate().is_err());
    observation = self::observation();
    observation.source_startup_id = "not-a-uuid".into();
    observation.delivery_digest = observation.computed_digest();
    assert!(observation.validate().is_err());
    observation = self::observation();
    observation.inventory_schema = 2;
    observation.delivery_digest = observation.computed_digest();
    assert!(observation.validate().is_err());
    observation = self::observation();
    let mut value: serde_json::Value = serde_json::from_slice(&observation.inventory_json).unwrap();
    value["private_key"] = "secret".into();
    observation.inventory_json = serde_json::to_vec(&value).unwrap();
    observation.delivery_digest = observation.computed_digest();
    assert!(observation.validate().is_err());
}

#[test]
fn legacy_migration_preserves_time_and_uses_stable_source_identity() {
    let batch: Batch =
        serde_json::from_slice(include_bytes!("fixtures/telemetry-batch.json")).unwrap();
    let first = InventoryObservation::from_legacy_batch(&batch)
        .unwrap()
        .unwrap();
    let second = InventoryObservation::from_legacy_batch(&batch)
        .unwrap()
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(first.observed_at_ms, batch.sampled_at.timestamp_millis());
    assert_eq!(first.source_startup_id, batch.session_id.to_string());
    assert_eq!(first.source_batch_id, batch.id.to_string());
    assert_eq!(
        first.inventory_json,
        serde_json::to_vec(batch.inventory.as_ref().unwrap()).unwrap()
    );
    first.validate().unwrap();
}

#[test]
fn delivery_digest_uses_length_prefixed_fields_with_a_stable_domain() {
    let observation = InventoryObservation {
        observation_id: Uuid::from_u128(1).to_string(),
        source_startup_id: Uuid::from_u128(2).to_string(),
        observed_at_ms: 1000,
        monotonic_ms: 17,
        inventory_schema: 1,
        inventory_json: b"{}".to_vec(),
        ..Default::default()
    };
    assert_eq!(
        observation.computed_digest(),
        "a496e14bc552e64007d525621a91a2f61e4ebb8055792694ee6d8b36b23f3137"
    );
    let changed = InventoryObservation {
        source_batch_id: Uuid::from_u128(3).to_string(),
        ..observation.clone()
    };
    assert_ne!(changed.computed_digest(), observation.computed_digest());
}

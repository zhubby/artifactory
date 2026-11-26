//! [INPUT]: 依赖本轮新采样的主机批次和严格v1字段预算。
//! [OUTPUT]: 按UTF8字节和实际JSON长度裁剪主机设备，报告省略。
//! [POS]: 采集器至新鲜队列的编码边界；禁止改写旧磁盘记录。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use anyhow::{Result, ensure};
use artifactory_emissary_protocol::telemetry::Batch;

// 待发送与发送中各占一份，两份总额不超过1MiB。
const MAX_HOST_BYTES: usize = 512 << 10;

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct HostBounds {
    pub omitted_devices: u64,
    pub truncated_fields: u64,
}

/// 返回不越过UTF8字符边界的最长字节前缀。
pub(super) fn text_prefix(value: &str, max_bytes: usize) -> &str {
    let mut end = value.len().min(max_bytes);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

fn trim_text(value: &mut String, max_bytes: usize, bounds: &mut HostBounds) {
    let end = text_prefix(value, max_bytes).len();
    if end < value.len() {
        value.truncate(end);
        bounds.truncated_fields += 1;
    }
}

fn trim_devices<T>(devices: &mut Vec<T>, max: usize, bounds: &mut HostBounds) {
    bounds.omitted_devices += devices.len().saturating_sub(max) as u64;
    devices.truncate(max);
}

fn remove_last<T: serde::Serialize>(devices: &mut Vec<T>) -> Result<Option<usize>> {
    let Some(last) = devices.last() else {
        return Ok(None);
    };
    let removed = serde_json::to_vec(last)?.len() + usize::from(devices.len() > 1);
    devices.pop();
    Ok(Some(removed))
}

/// 只裁剪本轮主机批次，保留身份、时间、标量和设备顺序。
///
/// # Errors
/// 拒绝混合批次和无主机批次；返回编码错误或无设备可省略的越界。
pub(super) fn bounded_host(batch: &mut Batch) -> Result<HostBounds> {
    ensure!(
        batch.host.is_some() && batch.inventory.is_none() && batch.processes.is_empty(),
        "host-only batch required"
    );
    let host = batch.host.as_mut().expect("host checked above");
    let mut bounds = HostBounds::default();
    trim_devices(&mut host.disks, 256, &mut bounds);
    trim_devices(&mut host.networks, 256, &mut bounds);
    trim_devices(&mut host.accelerators, 64, &mut bounds);
    for disk in &mut host.disks {
        trim_text(&mut disk.device, 512, &mut bounds);
        trim_text(&mut disk.mount, 1024, &mut bounds);
    }
    for network in &mut host.networks {
        trim_text(&mut network.interface, 128, &mut bounds);
    }
    for accelerator in &mut host.accelerators {
        trim_text(&mut accelerator.id, 256, &mut bounds);
    }
    // 入队后会覆写耐久缺口计数，预留其合法最大十进制宽度。
    let counter_reserve = (i64::MAX as u64)
        .to_string()
        .len()
        .saturating_sub(batch.dropped_batches.to_string().len());
    let mut encoded_bytes = serde_json::to_vec(batch)?.len() + counter_reserve;
    let host = batch.host.as_mut().expect("host checked above");
    // 先省略磁盘末项，保留接口与加速器；再按相同规则省略其末项。
    while encoded_bytes > MAX_HOST_BYTES {
        let removed = if !host.disks.is_empty() {
            remove_last(&mut host.disks)?
        } else if !host.networks.is_empty() {
            remove_last(&mut host.networks)?
        } else {
            remove_last(&mut host.accelerators)?
        };
        let Some(removed) = removed else {
            anyhow::bail!("host scalar encoding exceeds budget");
        };
        encoded_bytes -= removed;
        bounds.omitted_devices += 1;
    }
    ensure!(
        serde_json::to_vec(batch)?.len() + counter_reserve <= MAX_HOST_BYTES,
        "host encoding exceeds budget"
    );
    Ok(bounds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use artifactory_emissary_protocol::telemetry::{AcceleratorSample, DiskSample, NetworkSample};

    fn host_batch() -> Batch {
        let mut batch: Batch = serde_json::from_slice(include_bytes!(
            "../../../emissary-protocol/tests/fixtures/telemetry-batch.json"
        ))
        .unwrap();
        batch.inventory = None;
        batch.processes.clear();
        batch
    }

    #[test]
    fn legal_escaped_strings_fit_fresh_slot_without_changing_identity_or_scalars() {
        let mut batch = host_batch();
        let scalar = batch.host.as_ref().unwrap().clone();
        let host = batch.host.as_mut().unwrap();
        host.disks = vec![
            DiskSample {
                device: "\u{1}".repeat(512),
                mount: "\u{1}".repeat(1024),
                total_bytes: 200,
                available_bytes: 100,
                read_bytes: Some(10),
                written_bytes: None
            };
            256
        ];
        host.networks = vec![
            NetworkSample {
                interface: "\u{1}".repeat(128),
                received_bytes: 1,
                transmitted_bytes: 2,
                receive_errors: 3,
                transmit_errors: 4
            };
            256
        ];
        host.accelerators = vec![
            AcceleratorSample {
                id: "\u{1}".repeat(256),
                utilization_percent: Some(75.0),
                memory_used_bytes: Some(5)
            };
            64
        ];
        batch.validate().unwrap();
        assert!(serde_json::to_vec(&batch).unwrap().len() > 512 << 10);
        let id = batch.id;
        let session = batch.session_id;
        let sampled = batch.sampled_at;
        let monotonic = batch.monotonic_ms;
        let result = bounded_host(&mut batch).unwrap();
        assert!(result.omitted_devices > 0);
        assert!(serde_json::to_vec(&batch).unwrap().len() <= 512 << 10);
        batch.validate().unwrap();
        assert_eq!(
            (
                batch.id,
                batch.session_id,
                batch.sampled_at,
                batch.monotonic_ms
            ),
            (id, session, sampled, monotonic)
        );
        let host = batch.host.as_ref().unwrap();
        assert_eq!(host.cpu_percent, scalar.cpu_percent);
        assert_eq!(host.available_memory_bytes, scalar.available_memory_bytes);
        assert_eq!(
            host.build_disk_available_bytes,
            scalar.build_disk_available_bytes
        );
        assert_eq!(host.active_attempts, scalar.active_attempts);
        let mut actual_scalars = serde_json::to_value(host).unwrap();
        let mut expected_scalars = serde_json::to_value(&scalar).unwrap();
        for key in ["disks", "networks", "accelerators"] {
            actual_scalars.as_object_mut().unwrap().remove(key);
            expected_scalars.as_object_mut().unwrap().remove(key);
        }
        assert_eq!(actual_scalars, expected_scalars);
        assert_eq!(host.networks.len(), 256);
        assert_eq!(host.accelerators.len(), 64);
        assert_eq!(result.omitted_devices, 256 - host.disks.len() as u64);
    }

    #[test]
    fn normal_host_remains_byte_identical() {
        let mut batch = host_batch();
        let original = serde_json::to_vec(&batch).unwrap();
        assert_eq!(bounded_host(&mut batch).unwrap(), HostBounds::default());
        assert_eq!(serde_json::to_vec(&batch).unwrap(), original);
    }

    #[test]
    fn utf8_limits_keep_complete_codepoints_and_match_strict_byte_bounds() {
        assert_eq!(text_prefix("甲乙a", 4), "甲");
        assert_eq!(text_prefix("甲乙a", 6), "甲乙");
        assert_eq!(text_prefix("甲乙a", 0), "");
        assert_eq!(text_prefix("abc", 10), "abc");
        let mut batch = host_batch();
        let host = batch.host.as_mut().unwrap();
        host.disks = vec![DiskSample {
            device: "盘".repeat(512),
            mount: "挂".repeat(1024),
            total_bytes: 10,
            available_bytes: 5,
            read_bytes: None,
            written_bytes: None,
        }];
        host.networks = vec![NetworkSample {
            interface: "网".repeat(128),
            received_bytes: 1,
            transmitted_bytes: 2,
            receive_errors: 0,
            transmit_errors: 0,
        }];
        host.accelerators = vec![AcceleratorSample {
            id: "卡".repeat(256),
            utilization_percent: None,
            memory_used_bytes: None,
        }];
        let result = bounded_host(&mut batch).unwrap();
        assert_eq!(result.omitted_devices, 0);
        assert_eq!(result.truncated_fields, 4);
        batch.validate().unwrap();
        let host = batch.host.unwrap();
        assert_eq!(host.disks[0].device.len(), 510);
        assert_eq!(host.disks[0].mount.len(), 1023);
        assert_eq!(host.networks[0].interface.len(), 126);
        assert_eq!(host.accelerators[0].id.len(), 255);
    }

    #[test]
    fn device_count_trimming_is_deterministic_and_counted_once() {
        let mut batch = host_batch();
        let host = batch.host.as_mut().unwrap();
        host.disks = (0..258)
            .map(|i| DiskSample {
                device: i.to_string(),
                mount: "/".into(),
                total_bytes: 10,
                available_bytes: 5,
                read_bytes: None,
                written_bytes: None,
            })
            .collect();
        host.networks = (0..257)
            .map(|i| NetworkSample {
                interface: i.to_string(),
                received_bytes: 1,
                transmitted_bytes: 2,
                receive_errors: 0,
                transmit_errors: 0,
            })
            .collect();
        host.accelerators = (0..65)
            .map(|i| AcceleratorSample {
                id: i.to_string(),
                utilization_percent: None,
                memory_used_bytes: None,
            })
            .collect();
        assert_eq!(bounded_host(&mut batch).unwrap().omitted_devices, 4);
        let host = batch.host.as_ref().unwrap();
        assert_eq!(
            (
                host.disks.len(),
                host.networks.len(),
                host.accelerators.len()
            ),
            (256, 256, 64)
        );
        assert_eq!(host.disks.last().unwrap().device, "255");
        assert_eq!(host.networks.last().unwrap().interface, "255");
        assert_eq!(host.accelerators.last().unwrap().id, "63");
        assert_eq!(bounded_host(&mut batch).unwrap(), HostBounds::default());
    }

    #[test]
    fn rejects_non_host_only_batches_before_mutation() {
        let mut batch = host_batch();
        batch.host = None;
        assert!(bounded_host(&mut batch).is_err());
        let mut batch = host_batch();
        batch.inventory = Some(Default::default());
        let bytes = serde_json::to_vec(&batch).unwrap();
        assert!(bounded_host(&mut batch).is_err());
        assert_eq!(serde_json::to_vec(&batch).unwrap(), bytes);
    }

    #[test]
    fn bounding_does_not_repair_invalid_numeric_resources() {
        let mut batch = host_batch();
        batch.host.as_mut().unwrap().cpu_percent = Some(150.0);
        let original = serde_json::to_vec(&batch).unwrap();
        assert_eq!(bounded_host(&mut batch).unwrap(), HostBounds::default());
        assert_eq!(serde_json::to_vec(&batch).unwrap(), original);
        assert!(batch.validate().is_err());
    }

    #[test]
    fn later_durable_gap_counter_growth_stays_within_fresh_slot() {
        let mut batch = host_batch();
        let host = batch.host.as_mut().unwrap();
        host.disks = vec![
            DiskSample {
                device: "\u{1}".repeat(512),
                mount: "\u{1}".repeat(1024),
                total_bytes: 200,
                available_bytes: 100,
                read_bytes: Some(10),
                written_bytes: None,
            };
            60
        ];
        // 只构造恰好触及编码边界的合法输入，不调用待测裁剪器。
        let mut excess = serde_json::to_vec(&batch).unwrap().len() - (512 << 10);
        for disk in batch.host.as_mut().unwrap().disks.iter_mut().rev() {
            for field in [&mut disk.mount, &mut disk.device] {
                let removed = field.len().min(excess.div_ceil(6));
                field.truncate(field.len() - removed);
                excess = excess.saturating_sub(removed * 6);
            }
        }
        let remaining = (512 << 10) - serde_json::to_vec(&batch).unwrap().len();
        batch
            .host
            .as_mut()
            .unwrap()
            .disks
            .iter_mut()
            .rev()
            .find(|disk| disk.mount.len() < 1018)
            .unwrap()
            .mount
            .push_str(&"x".repeat(remaining));
        batch.validate().unwrap();
        assert_eq!(serde_json::to_vec(&batch).unwrap().len(), 512 << 10);
        assert!(bounded_host(&mut batch).unwrap().omitted_devices > 0);
        batch.dropped_batches = i64::MAX as u64;
        assert!(serde_json::to_vec(&batch).unwrap().len() <= 512 << 10);
        batch.validate().unwrap();
    }
}

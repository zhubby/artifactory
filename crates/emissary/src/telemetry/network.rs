//! [INPUT]: 依赖 Collector 的本地接口地址、采样时间与共享 JSON 预算。
//! [OUTPUT]: 提供排序、过滤且限制字节数的网络诊断快照。
//! [POS]: 仅构造开放诊断，不改变严格遥测批次。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_emissary_protocol::nodes::MAX_NETWORK_INTERFACES_BYTES;
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    net::IpAddr,
};

const MAX_INTERFACES: usize = 16;
const MAX_ADDRESSES: usize = 8;
const MAX_NAME_BYTES: usize = 128;

pub(super) fn snapshot(
    sampled_at: DateTime<Utc>,
    interfaces: impl IntoIterator<Item = (String, Vec<IpAddr>)>,
) -> Value {
    let mut grouped = BTreeMap::<String, BTreeSet<IpAddr>>::new();
    let mut truncated = false;
    for (name, addresses) in interfaces {
        if name.len() > MAX_NAME_BYTES {
            truncated = true;
            continue;
        }
        grouped.entry(name).or_default().extend(
            addresses
                .into_iter()
                .filter(|ip| !ip.is_loopback() && !ip.is_unspecified()),
        );
    }
    let mut entries = Vec::new();
    for (name, addresses) in grouped {
        if addresses.is_empty() {
            continue;
        }
        if entries.len() == MAX_INTERFACES {
            truncated = true;
            break;
        }
        truncated |= addresses.len() > MAX_ADDRESSES;
        entries.push(json!({
            "name":name,
            "addresses":addresses.into_iter().take(MAX_ADDRESSES).map(|ip| ip.to_string()).collect::<Vec<_>>()
        }));
    }
    let mut snapshot = json!({
        "sampled_at":sampled_at.to_rfc3339_opts(SecondsFormat::Millis, true),
        "interfaces":entries,
        "truncated":truncated
    });
    while serde_json::to_vec(&snapshot)
        .expect("network snapshot is JSON")
        .len()
        > MAX_NETWORK_INTERFACES_BYTES
    {
        snapshot["interfaces"]
            .as_array_mut()
            .expect("interfaces is an array")
            .pop();
        snapshot["truncated"] = json!(true);
    }
    snapshot
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};
    use serde_json::json;
    use std::net::IpAddr;

    fn sampled_at() -> DateTime<Utc> {
        "2026-11-20T01:02:03Z".parse().unwrap()
    }
    fn interface(name: &str, addresses: &[&str]) -> (String, Vec<IpAddr>) {
        (
            name.into(),
            addresses.iter().map(|ip| ip.parse().unwrap()).collect(),
        )
    }

    #[test]
    fn sorts_deduplicates_and_filters_local_addresses_without_losing_vpn_or_ipv6() {
        let snapshot = snapshot(
            sampled_at(),
            [
                interface("vpn0", &["100.64.0.2", "fd00::2", "100.64.0.2"]),
                interface(
                    "eth0",
                    &["192.168.1.9", "0.0.0.0", "127.0.0.1", "::", "::1"],
                ),
                interface("eth0", &["10.0.0.2", "fe80::1", "192.168.1.9"]),
                interface("lo", &["::1", "127.0.0.1"]),
            ],
        );
        assert_eq!(
            snapshot,
            json!({
                "sampled_at":"2026-11-20T01:02:03.000Z",
                "interfaces":[
                    {"name":"eth0","addresses":["10.0.0.2","192.168.1.9","fe80::1"]},
                    {"name":"vpn0","addresses":["100.64.0.2","fd00::2"]}
                ],
                "truncated":false
            })
        );
    }

    #[test]
    fn interface_changes_replace_previous_addresses() {
        let first = snapshot(sampled_at(), [interface("eth0", &["10.0.0.1"])]);
        let next = snapshot(sampled_at(), [interface("vpn0", &["100.64.0.1"])]);
        assert_eq!(first["interfaces"][0]["addresses"], json!(["10.0.0.1"]));
        assert_eq!(
            next["interfaces"],
            json!([{"name":"vpn0","addresses":["100.64.0.1"]}])
        );
    }

    #[test]
    fn limits_interfaces_and_addresses_after_sorting_and_deduplication() {
        let interfaces = (0..20).rev().map(|index| {
            (
                format!("net{index:02}"),
                (1..=10)
                    .rev()
                    .map(|last| IpAddr::from([10, 0, 0, last]))
                    .collect(),
            )
        });
        let value = snapshot(sampled_at(), interfaces);
        let interfaces = value["interfaces"].as_array().unwrap();
        assert_eq!(interfaces.len(), 16);
        assert_eq!(interfaces[0]["name"], "net00");
        assert_eq!(interfaces[15]["name"], "net15");
        assert_eq!(
            interfaces[0]["addresses"],
            json!([
                "10.0.0.1", "10.0.0.2", "10.0.0.3", "10.0.0.4", "10.0.0.5", "10.0.0.6", "10.0.0.7",
                "10.0.0.8"
            ])
        );
        assert_eq!(value["truncated"], true);
    }

    #[test]
    fn skips_names_above_utf8_byte_limit_and_keeps_exact_boundary() {
        let accepted = "网".repeat(42) + "ab";
        let rejected = "网".repeat(43);
        let value = snapshot(
            sampled_at(),
            [
                interface(&rejected, &["10.0.0.1"]),
                interface(&accepted, &["10.0.0.2"]),
            ],
        );
        assert_eq!(
            value["interfaces"],
            json!([{"name":accepted,"addresses":["10.0.0.2"]}])
        );
        assert_eq!(value["truncated"], true);
    }

    #[test]
    fn duplicate_addresses_do_not_consume_the_address_budget() {
        let addresses = (1..=8)
            .flat_map(|last| [IpAddr::from([10, 0, 0, last]); 2])
            .collect();
        let value = snapshot(sampled_at(), [("eth0".into(), addresses)]);
        assert_eq!(
            value["interfaces"][0]["addresses"]
                .as_array()
                .unwrap()
                .len(),
            8
        );
        assert_eq!(value["interfaces"][0]["addresses"][7], "10.0.0.8");
        assert_eq!(value["truncated"], false);
    }

    #[test]
    fn enforces_serialized_json_limit_including_escaped_interface_names() {
        let value = snapshot(
            sampled_at(),
            (0..16).map(|index| {
                let name = format!("{index:02}{}", "\u{0001}".repeat(126));
                let addresses = (0..8)
                    .map(|last| {
                        format!("ffff:ffff:ffff:ffff:ffff:ffff:ffff:{last:x}")
                            .parse()
                            .unwrap()
                    })
                    .collect();
                (name, addresses)
            }),
        );
        assert!(serde_json::to_vec(&value).unwrap().len() <= 8192);
        assert!(value["interfaces"].as_array().unwrap().len() < 16);
        assert_eq!(value["truncated"], true);
    }
}

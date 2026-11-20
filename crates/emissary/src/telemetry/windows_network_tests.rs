//! [INPUT]: 依赖 Windows ABI 结构、原生查询入口和网络诊断整形。
//! [OUTPUT]: 验证无 MAC VPN、地址移除、缓冲区重试与安全遍历。
//! [POS]: Windows 地址来源的跨平台缓冲区夹具与目标机采集回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use serde_json::json;
use windows_sys::Win32::Networking::WinSock::{IN_ADDR, IN_ADDR_0, IN6_ADDR, IN6_ADDR_0};

#[repr(C)]
#[derive(Default)]
struct Fixture {
    adapters: [IP_ADAPTER_ADDRESSES_LH; 2],
    unicast: [IP_ADAPTER_UNICAST_ADDRESS_LH; 3],
    ipv4: [SOCKADDR_IN; 2],
    ipv6: SOCKADDR_IN6,
    names: [[u16; 32]; 2],
}

// 夹具直接写入被测查询分配的缓冲区；所有原生指针都指向该分配。
unsafe fn write_fixture(pointer: *mut IP_ADAPTER_ADDRESSES_LH, removed: bool) {
    let fixture = unsafe { &mut *pointer.cast::<Fixture>() };
    *fixture = Fixture::default();
    for (index, name) in ["Ethernet", "Tailscale 隧道🛜"].into_iter().enumerate() {
        let units: Vec<_> = name.encode_utf16().collect();
        fixture.names[index][..units.len()].copy_from_slice(&units);
        fixture.adapters[index].Anonymous1.Anonymous.Length =
            size_of::<IP_ADAPTER_ADDRESSES_LH>() as u32;
        fixture.adapters[index].FriendlyName = fixture.names[index].as_mut_ptr();
    }
    fixture.adapters[0].Next = &mut fixture.adapters[1];
    fixture.adapters[0].PhysicalAddressLength = 6;
    fixture.adapters[0].TransmitLinkSpeed = 1_000_000_000;
    fixture.adapters[0].ReceiveLinkSpeed = 1_000_000_000;
    // VPN 保留零 MAC 长度与零双向链路速率，复现 sysinfo 的过滤边界。
    if removed {
        return;
    }
    for entry in &mut fixture.unicast {
        entry.Anonymous.Anonymous.Length = size_of::<IP_ADAPTER_UNICAST_ADDRESS_LH>() as u32;
    }
    fixture.adapters[0].FirstUnicastAddress = &mut fixture.unicast[0];
    fixture.adapters[1].FirstUnicastAddress = &mut fixture.unicast[1];
    fixture.unicast[1].Next = &mut fixture.unicast[2];
    for (index, octets) in [[192, 168, 1, 9], [100, 64, 0, 2]].into_iter().enumerate() {
        fixture.ipv4[index].sin_family = AF_INET;
        fixture.ipv4[index].sin_addr = IN_ADDR {
            S_un: IN_ADDR_0 {
                S_addr: u32::from_ne_bytes(octets),
            },
        };
        fixture.unicast[index].Address = SOCKET_ADDRESS {
            lpSockaddr: (&mut fixture.ipv4[index] as *mut SOCKADDR_IN).cast(),
            iSockaddrLength: size_of::<SOCKADDR_IN>() as i32,
        };
    }
    fixture.ipv6.sin6_family = AF_INET6;
    fixture.ipv6.sin6_addr = IN6_ADDR {
        u: IN6_ADDR_0 {
            Byte: "fd00::2".parse::<std::net::Ipv6Addr>().unwrap().octets(),
        },
    };
    fixture.unicast[2].Address = SOCKET_ADDRESS {
        lpSockaddr: (&mut fixture.ipv6 as *mut SOCKADDR_IN6).cast(),
        iSockaddrLength: size_of::<SOCKADDR_IN6>() as i32,
    };
}

fn enumerate_fixture(removed: bool) -> Vec<(String, Vec<IpAddr>)> {
    collect_with(|family, flags, pointer, length| {
        assert_eq!(family, AF_UNSPEC as u32);
        assert_eq!(
            flags & GAA_FLAG_INCLUDE_ALL_INTERFACES,
            GAA_FLAG_INCLUDE_ALL_INTERFACES
        );
        assert_eq!(pointer as usize % align_of::<IP_ADAPTER_ADDRESSES_LH>(), 0);
        assert!(unsafe { *length } as usize >= size_of::<Fixture>());
        unsafe {
            write_fixture(pointer, removed);
            *length = size_of::<Fixture>() as u32;
        }
        NO_ERROR
    })
    .expect("原生夹具必须成功枚举")
}

#[test]
fn no_mac_zero_speed_vpn_keeps_real_ipv4_ipv6_and_clears_removed_addresses() {
    let sampled_at = "2026-11-20T01:02:03Z".parse().unwrap();
    let snapshot = super::super::network::snapshot(sampled_at, enumerate_fixture(false));
    assert_eq!(
        snapshot["interfaces"],
        json!([
            {"name":"Ethernet","addresses":["192.168.1.9"]},
            {"name":"Tailscale 隧道🛜","addresses":["100.64.0.2","fd00::2"]}
        ])
    );
    assert_eq!(snapshot["truncated"], false);
    assert!(!snapshot.to_string().contains("PhysicalAddress"));
    let next = super::super::network::snapshot(sampled_at, enumerate_fixture(true));
    assert_eq!(next["interfaces"], json!([]));
}

#[test]
fn buffer_growth_retries_then_reads_the_fresh_allocation() {
    let mut calls = 0;
    let entries = collect_with(|_, _, pointer, length| {
        calls += 1;
        if calls == 1 {
            unsafe {
                *length = 32 * 1024;
            }
            return ERROR_BUFFER_OVERFLOW;
        }
        assert_eq!(unsafe { *length }, 32 * 1024);
        unsafe {
            write_fixture(pointer, false);
            *length = size_of::<Fixture>() as u32;
        }
        NO_ERROR
    })
    .unwrap();
    assert_eq!(calls, 2);
    assert_eq!(entries[1].1.len(), 2);
}

#[test]
fn failed_or_unbounded_native_queries_do_not_publish_a_false_empty_snapshot() {
    assert!(collect_with(|_, _, _, _| 5).is_none());
    assert_eq!(collect_with(|_, _, _, _| ERROR_NO_DATA), Some(vec![]));
    let mut calls = 0;
    assert!(
        collect_with(|_, _, _, length| {
            calls += 1;
            unsafe {
                *length = (*length).saturating_mul(2);
            }
            ERROR_BUFFER_OVERFLOW
        })
        .is_none()
    );
    assert_eq!(calls, 3);
    assert!(
        collect_with(|_, _, _, length| {
            unsafe {
                *length = u32::MAX;
            }
            ERROR_BUFFER_OVERFLOW
        })
        .is_none()
    );
}

#[test]
fn malformed_native_links_lengths_and_utf16_are_bounded() {
    for case in 0..6 {
        let result = collect_with(|_, _, pointer, length| {
            unsafe {
                write_fixture(pointer, false);
                let fixture = &mut *pointer.cast::<Fixture>();
                match case {
                    0 => fixture.adapters[1].Next = pointer,
                    1 => fixture.unicast[2].Next = &mut fixture.unicast[1],
                    2 => fixture.unicast[0].Address.iSockaddrLength = 1,
                    3 => fixture.adapters[0].Anonymous1.Anonymous.Length = 1,
                    4 => fixture.adapters[0].Next = (pointer as usize + 15000) as *mut _,
                    _ => {
                        *length = 0;
                        return NO_ERROR;
                    }
                }
                *length = size_of::<Fixture>() as u32;
            }
            NO_ERROR
        });
        assert!(result.is_none(), "损坏的原生链必须拒绝：{case}");
    }
    let result = collect_with(|_, _, pointer, length| {
        unsafe {
            write_fixture(pointer, false);
            let fixture = &mut *pointer.cast::<Fixture>();
            fixture.names[0][0] = 0xd800;
            *length = size_of::<Fixture>() as u32;
        }
        NO_ERROR
    })
    .unwrap();
    assert_eq!(result.len(), 1, "非法 UTF-16 名称不生成伪造接口");
    assert_eq!(result[0].0, "Tailscale 隧道🛜");
}

#[cfg(windows)]
#[test]
fn native_get_adapters_addresses_feeds_diagnostics() {
    let interfaces = collect().expect("目标机 GetAdaptersAddresses 必须成功");
    let snapshot = super::super::network::snapshot(chrono::Utc::now(), interfaces);
    assert!(snapshot["interfaces"].is_array());
    assert!(serde_json::to_vec(&snapshot).unwrap().len() <= 8192);
    eprintln!("Windows 原生接口诊断：{snapshot}");
}

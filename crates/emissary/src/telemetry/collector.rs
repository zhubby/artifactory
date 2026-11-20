//! [INPUT]: 依赖系统资源、Windows 完整单播地址、受管任务身份与节点遥测契约。
//! [OUTPUT]: 生成严格遥测批次，每轮重建本地接口诊断缓存。
//! [POS]: 节点遥测循环的采样器，不建立网络连接。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::managed::{self, Registry};
use crate::config::Config;
use artifactory_emissary_protocol::telemetry::{
    Batch, DiskSample, HostSample, Inventory, MAX_PROCESSES, NetworkSample, ProcessSample,
};
use chrono::Utc;
use serde_json::json;
use std::{collections::HashMap, time::Instant};
use sysinfo::{Disks, Networks, Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use uuid::Uuid;

pub(super) struct Collector {
    system: System,
    disks: Disks,
    networks: Networks,
    session: Uuid,
    started: Instant,
    previous: HashMap<(u32, u64), Uuid>,
    cpu_ready: bool,
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[test]
    fn native_collector_uses_complete_adapter_addresses() {
        let root = tempfile::tempdir().unwrap();
        let cfg = Config {
            root: root.path().into(),
            ..Default::default()
        };
        let mut collector = Collector::new(Uuid::new_v4());
        let (_, observed) = collector.sample(&cfg, &Registry::default(), true, None);
        let observed = observed.network_interfaces.expect("原生接口采集必须成功");
        let interfaces =
            super::super::windows_network::collect().expect("独立 IP Helper 枚举必须成功");
        let expected = super::super::network::snapshot(Utc::now(), interfaces);
        assert_eq!(observed["interfaces"], expected["interfaces"]);
        assert_eq!(observed["truncated"], expected["truncated"]);
    }
}
impl Collector {
    pub fn new(session: Uuid) -> Self {
        let mut system = System::new();
        system.refresh_cpu_all();
        system.refresh_memory();
        Self {
            system,
            disks: Disks::new_with_refreshed_list(),
            networks: Networks::new(),
            session,
            started: Instant::now(),
            previous: HashMap::new(),
            cpu_ready: false,
        }
    }
    pub fn sample(
        &mut self,
        cfg: &Config,
        registry: &Registry,
        host: bool,
        inventory: Option<Inventory>,
    ) -> (Batch, super::Snapshot) {
        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        // sysinfo 保留仍存在接口的旧 IP；每轮从空集合枚举地址。
        self.networks = Networks::new_with_refreshed_list();
        let sampled_at = Utc::now();
        #[cfg(windows)]
        let network_interfaces = super::windows_network::collect()
            .map(|interfaces| super::network::snapshot(sampled_at, interfaces));
        #[cfg(not(windows))]
        let network_interfaces = Some(super::network::snapshot(
            sampled_at,
            self.networks.iter().map(|(name, network)| {
                (
                    name.clone(),
                    network.ip_networks().iter().map(|ip| ip.addr).collect(),
                )
            }),
        ));
        let roots = registry.roots();
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().without_tasks(),
        );
        let owners = managed::owners(&self.system, &roots, &self.previous);
        let mut pids: Vec<_> = owners.keys().map(|(pid, _)| Pid::from_u32(*pid)).collect();
        pids.push(Pid::from_u32(std::process::id()));
        // Enumerate ancestry locally, then read resource counters only for managed processes.
        // Never request command lines, environment, cwd, executable paths or user IDs.
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&pids),
            false,
            ProcessRefreshKind::nothing()
                .without_tasks()
                .with_cpu()
                .with_memory()
                .with_disk_usage(),
        );
        let mut identities: Vec<_> = owners.iter().collect();
        identities.sort_by_key(|(identity, _)| **identity);
        let omitted = identities
            .len()
            .saturating_sub(MAX_PROCESSES)
            .min(u32::MAX as usize) as u32;
        let processes = identities
            .into_iter()
            .take(MAX_PROCESSES)
            .filter_map(|(&(pid, started), &attempt)| {
                let p = self.system.process(Pid::from_u32(pid))?;
                let disk = p.disk_usage();
                Some(ProcessSample {
                    attempt_id: attempt,
                    pid,
                    started_at: started,
                    parent_pid: p.parent().map(|p| p.as_u32()),
                    step: roots.get(&attempt).and_then(|r| r.step),
                    cpu_percent: self
                        .previous
                        .contains_key(&(pid, started))
                        .then_some(f64::from(p.cpu_usage())),
                    rss_bytes: Some(p.memory()),
                    read_bytes: if cfg!(any(target_os = "linux", target_os = "macos", windows)) {
                        Some(disk.total_read_bytes)
                    } else {
                        None
                    },
                    written_bytes: if cfg!(any(target_os = "linux", target_os = "macos", windows)) {
                        Some(disk.total_written_bytes)
                    } else {
                        None
                    },
                    status: p.status().to_string(),
                })
            })
            .collect();
        self.previous = owners;
        let h = host.then(|| {
            self.disks.refresh(true);
            HostSample {
                cpu_percent: self
                    .cpu_ready
                    .then_some(f64::from(self.system.global_cpu_usage())),
                memory_total_bytes: Some(self.system.total_memory()).filter(|n| *n > 0),
                memory_used_bytes: Some(self.system.used_memory())
                    .filter(|_| self.system.total_memory() > 0),
                available_memory_bytes: super::inventory::available_memory(&self.system),
                build_disk_available_bytes: fs2::available_space(&cfg.root).ok(),
                swap_total_bytes: Some(self.system.total_swap()),
                swap_used_bytes: Some(self.system.used_swap()),
                load_one: if cfg!(unix) {
                    Some(System::load_average().one)
                } else {
                    None
                },
                disks: self
                    .disks
                    .list()
                    .iter()
                    .filter(|d| d.total_space() > 0)
                    .take(256)
                    .map(|d| {
                        let usage = d.usage();
                        DiskSample {
                            device: d.name().to_string_lossy().chars().take(512).collect(),
                            mount: d
                                .mount_point()
                                .to_string_lossy()
                                .chars()
                                .take(1024)
                                .collect(),
                            total_bytes: d.total_space(),
                            available_bytes: d.available_space(),
                            read_bytes: if cfg!(any(target_os = "linux", windows)) {
                                Some(usage.total_read_bytes)
                            } else {
                                None
                            },
                            written_bytes: if cfg!(any(target_os = "linux", windows)) {
                                Some(usage.total_written_bytes)
                            } else {
                                None
                            },
                        }
                    })
                    .collect(),
                networks: self
                    .networks
                    .iter()
                    .take(256)
                    .map(|(name, n)| NetworkSample {
                        interface: name.chars().take(128).collect(),
                        received_bytes: n.total_received(),
                        transmitted_bytes: n.total_transmitted(),
                        receive_errors: n.total_errors_on_received(),
                        transmit_errors: n.total_errors_on_transmitted(),
                    })
                    .collect(),
                active_attempts: roots.len() as u32,
                ..HostSample::default()
            }
        });
        self.cpu_ready = true;
        let metrics = json!({"logical_cpus":self.system.cpus().len(),"emissary_rss_bytes":self.system.process(Pid::from_u32(std::process::id())).map(|p|p.memory()),"managed_free_bytes":fs2::available_space(&cfg.root).ok()});
        (
            Batch {
                id: Uuid::new_v4(),
                session_id: self.session,
                sampled_at,
                monotonic_ms: self.started.elapsed().as_millis().min(i64::MAX as u128) as u64,
                dropped_batches: 0,
                omitted_processes: omitted,
                host: h,
                processes,
                inventory,
            },
            super::Snapshot {
                metrics,
                network_interfaces,
            },
        )
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn removed_last_address_does_not_survive_the_next_collection() {
        const MARKER_ENV: &str = "ARTIFACTORY_TEST_REMOVED_INTERFACE_ADDRESSES";
        if let Some(marker) = std::env::var_os(MARKER_ENV) {
            let root = tempfile::tempdir().unwrap();
            let cfg = Config {
                root: root.path().into(),
                ..Default::default()
            };
            let mut collector = Collector::new(Uuid::new_v4());
            let registry = Registry::default();
            let (first_batch, first) = collector.sample(&cfg, &registry, true, None);
            let first = first.network_interfaces.unwrap();
            let name = first["interfaces"][0]["name"]
                .as_str()
                .expect("首轮必须采到接口地址")
                .to_owned();
            assert!(
                first_batch
                    .host
                    .unwrap()
                    .networks
                    .iter()
                    .any(|network| network.interface == name)
            );
            std::fs::write(marker, b"remove addresses").unwrap();
            let (next_batch, next) = collector.sample(&cfg, &registry, true, None);
            assert!(
                next_batch
                    .host
                    .unwrap()
                    .networks
                    .iter()
                    .any(|network| network.interface == name),
                "接口本身必须继续存在"
            );
            assert_eq!(
                next.network_interfaces.unwrap()["interfaces"],
                json!([]),
                "本轮无 IP 时必须清除上轮地址"
            );
            return;
        }

        // 只替换子进程的地址枚举；接口计数仍由真实 sysinfo 读取。
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("interface-addresses.c");
        let library = directory.path().join("interface-addresses.dylib");
        std::fs::write(
            &source,
            r#"
#include <arpa/inet.h>
#include <ifaddrs.h>
#include <stdlib.h>
#include <unistd.h>

static int sampled_getifaddrs(struct ifaddrs **result) {
    int status = getifaddrs(result);
    if (status != 0) return status;
    const char *marker = getenv("ARTIFACTORY_TEST_REMOVED_INTERFACE_ADDRESSES");
    int removed = marker && access(marker, F_OK) == 0;
    for (struct ifaddrs *item = *result; item; item = item->ifa_next) {
        if (!item->ifa_addr) continue;
        int family = item->ifa_addr->sa_family;
        if (family != AF_INET && family != AF_INET6) continue;
        if (removed) {
            item->ifa_addr->sa_family = AF_UNSPEC;
        } else if (family == AF_INET) {
            inet_pton(AF_INET, "10.0.0.2", &((struct sockaddr_in *)item->ifa_addr)->sin_addr);
        } else {
            inet_pton(AF_INET6, "fd00::2", &((struct sockaddr_in6 *)item->ifa_addr)->sin6_addr);
        }
    }
    return status;
}
__attribute__((used)) static struct {
    const void *replacement;
    const void *original;
} interpose_getifaddrs __attribute__((section("__DATA,__interpose"))) = {
    (const void *)sampled_getifaddrs, (const void *)getifaddrs
};
"#,
        )
        .unwrap();
        let compiled = Command::new("cc")
            .arg("-dynamiclib")
            .arg(&source)
            .arg("-o")
            .arg(&library)
            .output()
            .unwrap();
        assert!(
            compiled.status.success(),
            "{}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        let result = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "telemetry::collector::tests::removed_last_address_does_not_survive_the_next_collection", "--nocapture", "--test-threads=1"])
            .env("DYLD_INSERT_LIBRARIES", &library)
            .env(MARKER_ENV, directory.path().join("addresses-removed"))
            .output().unwrap();
        assert!(
            result.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
}

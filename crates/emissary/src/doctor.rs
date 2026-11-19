//! [INPUT]: 依赖节点工具配置和八路并发的有界探测。
//! [OUTPUT]: 提供诊断和通过实际探测的执行能力声明。
//! [POS]: 首次连接前固定能力快照，探测完成顺序不影响输出。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{COMMIT, VERSION, config::Config, platform, process};
use futures::{StreamExt, stream};
use serde_json::{Value, json};
use std::{collections::BTreeSet, time::Duration};
use tokio::process::Command;

// 八路探测限制启动进程数，默认工具不再逐个累积超时。
const TOOL_PROBE_CONCURRENCY: usize = 8;
const TOOL_PROBE_TIMEOUT: Duration = Duration::from_secs(3);

pub async fn doctor(cfg: &Config) -> (Value, Vec<String>) {
    let mut names: BTreeSet<String> = [
        "git",
        "git-lfs",
        "bash",
        "sh",
        "powershell",
        "pwsh",
        "cc",
        "cargo",
        "go",
        "node",
        "npm",
        "xcodebuild",
        "java",
        "gradle",
        "cmake",
        "ninja",
        "dotnet",
        "msbuild",
        "signtool",
        "codesign",
        "security",
        "apksigner",
        "jarsigner",
        "keytool",
        "xcrun",
        "ditto",
        "hdiutil",
        "docker",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    names.extend(cfg.tools.keys().cloned());
    probe_tools(cfg, names).await
}
async fn probe_tools(cfg: &Config, names: BTreeSet<String>) -> (Value, Vec<String>) {
    let mut tools = serde_json::Map::new();
    let mut capabilities = vec!["external-build-plan-v1".into(), "managed-signing-v1".into()];
    let mut probes = stream::iter(names)
        .map(|name| probe_tool(cfg, name))
        .buffered(TOOL_PROBE_CONCURRENCY);
    while let Some((name, diagnostic, working)) = probes.next().await {
        tools.insert(name.clone(), diagnostic);
        if working {
            capabilities.push(name);
        }
    }
    capabilities.sort();
    let (os, arch) = platform();
    (
        json!({"emissary_version":VERSION,"emissary_commit":COMMIT,"protocol_major":1,"hostname":sysinfo::System::host_name().unwrap_or_default(),"os":os,"arch":arch,"tools":tools,"configured_signing_profiles":cfg.signing_profiles.keys().collect::<Vec<_>>()}),
        capabilities,
    )
}

async fn probe_tool(cfg: &Config, name: String) -> (String, Value, bool) {
    let binary = cfg.tool(&name);
    let Ok(Ok(path)) = tokio::task::spawn_blocking(move || which::which(binary)).await else {
        return (name, json!({"available":false}), false);
    };
    let args = match name.as_str() {
        // POSIX sh 无需支持 --version，例如 Debian dash。
        "sh" => vec!["-c", "exit 0"],
        "go" | "apksigner" => vec!["version"],
        "xcodebuild" | "java" | "msbuild" => vec!["-version"],
        "codesign" => vec!["--help"],
        "jarsigner" | "keytool" => vec!["-help"],
        "hdiutil" => vec!["help"],
        "ditto" => vec!["-h"],
        "security" => vec!["help"],
        "signtool" => vec!["/?"],
        "powershell" | "pwsh" => vec![
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$PSVersionTable.PSVersion.ToString()",
        ],
        _ => vec!["--version"],
    };
    let mut command = Command::new(&path);
    command.args(&args);
    // 版本输出使用两个有界管道；配置路径不经过 shell 拼接。
    let result = process::probe(command, TOOL_PROBE_TIMEOUT).await;
    let (success, code, output) = match result {
        Ok((status, output)) => (status.success(), status.code(), output),
        Err(_) => (false, None, Vec::new()),
    };
    let output = String::from_utf8_lossy(&output);
    let version = output.trim();
    let version = version[..version.floor_char_boundary(version.len().min(512))].to_owned();
    let working = success
        || (name == "codesign" && code == Some(2) && version.starts_with("Usage: codesign"))
        || (name == "ditto" && version.contains("ditto"));
    let diagnostic = json!({"available":working,"path":path,"version":version,"version_check_succeeded":success,"probe_succeeded":working,"probe_arguments":args});
    (name, diagnostic, working)
}
pub fn metrics(cfg: &Config) -> Value {
    let mut system = sysinfo::System::new();
    let pid = sysinfo::Pid::from_u32(std::process::id());
    system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
    json!({"logical_cpus":std::thread::available_parallelism().ok().map(|n| n.get()),"emissary_rss_bytes":system.process(pid).map(|p|p.memory()),"managed_free_bytes":fs2::available_space(&cfg.root).ok(),"managed_root":cfg.root})
}
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn probes_tools_concurrently_with_a_limit_and_stable_output() {
        let directory = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        let names: BTreeSet<String> = (0..10).map(|index| format!("tool-{index:02}")).collect();
        for name in &names {
            let path = directory.path().join(name);
            crate::config::private_write(
                &path,
                b"#!/bin/sh\nprintf started > \"$0.started\"\nwhile [ ! -f \"${0%/*}/release\" ]; do sleep 0.01; done\nprintf 'v1\\n'\n",
                true,
                true,
            )
            .unwrap();
            cfg.tools
                .insert(name.clone(), path.to_string_lossy().into());
        }
        let expected = names.clone();
        let probing = tokio::spawn(async move { probe_tools(&cfg, names).await });
        let started = || {
            std::fs::read_dir(directory.path())
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "started"))
                .count()
        };
        let first_wave = tokio::time::timeout(Duration::from_secs(2), async {
            while started() < 8 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        let active_before_release = started();
        std::fs::write(directory.path().join("release"), "release").unwrap();
        let (diagnostics, capabilities) = probing.await.unwrap();
        assert!(
            first_wave.is_ok(),
            "independent tools must start concurrently"
        );
        assert_eq!(
            active_before_release, 8,
            "tool concurrency must remain bounded"
        );
        assert_eq!(
            diagnostics["tools"]
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
            expected.iter().cloned().collect::<Vec<_>>(),
            "completion order must not change the diagnostic order"
        );
        assert!(expected.iter().all(|name| capabilities.contains(name)));
    }

    #[tokio::test]
    async fn times_out_hung_tools_in_one_concurrent_wave() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("hung");
        crate::config::private_write(&path, b"#!/bin/sh\nexec sleep 30\n", true, true).unwrap();
        let mut cfg = Config::default();
        for index in 0..8 {
            cfg.tools
                .insert(format!("hung-{index}"), path.to_string_lossy().into());
        }
        let result = tokio::time::timeout(
            Duration::from_secs(6),
            probe_tools(&cfg, cfg.tools.keys().cloned().collect()),
        )
        .await;
        let (diagnostics, capabilities) =
            result.expect("hung tool deadlines must share one bounded concurrent wave");
        assert_eq!(diagnostics["tools"].as_object().unwrap().len(), 8);
        for name in cfg.tools.keys() {
            assert_eq!(diagnostics["tools"][name]["available"], false);
            assert!(!capabilities.contains(name));
        }
    }

    #[tokio::test]
    async fn advertises_posix_sh_without_a_version_option() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("posix-sh");
        crate::config::private_write(
            &path,
            b"#!/bin/sh\nif [ \"$1\" = --version ]; then exit 2; fi\nexec /bin/sh \"$@\"\n",
            true,
            true,
        )
        .unwrap();
        let mut cfg = Config::default();
        cfg.tools.insert("sh".into(), path.to_string_lossy().into());

        let (diagnostics, capabilities) =
            probe_tools(&cfg, cfg.tools.keys().cloned().collect()).await;

        assert!(capabilities.iter().any(|name| name == "sh"));
        assert_eq!(diagnostics["tools"]["sh"]["available"], true);
    }

    #[tokio::test]
    async fn only_advertises_working_toolchains() {
        let directory = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        for (name, contents) in [
            ("working", "#!/bin/sh\necho v1\n"),
            ("broken", "#!/bin/sh\nexit 1\n"),
            ("hung", "#!/bin/sh\nsleep 30\n"),
        ] {
            let path = directory.path().join(name);
            crate::config::private_write(&path, contents.as_bytes(), true, true).unwrap();
            cfg.tools.insert(name.into(), path.to_string_lossy().into());
        }
        let (diagnostics, capabilities) =
            probe_tools(&cfg, cfg.tools.keys().cloned().collect()).await;
        assert!(
            capabilities.iter().any(|s| s == "working"),
            "working tool probe: {}",
            diagnostics["tools"]["working"]
        );
        assert!(
            !capabilities.iter().any(|s| s == "broken" || s == "hung"),
            "rejected tool probes: {}",
            diagnostics["tools"]
        );
        assert_eq!(diagnostics["tools"].as_object().unwrap().len(), 3);
    }
}

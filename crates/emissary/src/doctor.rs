//! [INPUT]: 依赖节点工具配置和八路并发的有界探测。
//! [OUTPUT]: 提供类型化探测、旧清单投影和实际通过的能力声明。
//! [POS]: 首次连接前固定能力快照，探测完成顺序不影响输出。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{COMMIT, VERSION, config::Config, platform, process};
use artifactory_emissary_protocol::operations::{
    PROBE_IDS, ProbeErrorCode, ProbeReport, ProbeStatus,
};
use futures::{StreamExt, stream};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

// 八路探测限制启动进程数，默认工具不再逐个累积超时。
const TOOL_PROBE_CONCURRENCY: usize = 8;
const TOOL_PROBE_TIMEOUT: Duration = Duration::from_secs(3);

pub async fn doctor(cfg: &Config) -> (Value, Vec<String>) {
    let mut names: BTreeSet<String> = PROBE_IDS.iter().map(|id| (*id).to_owned()).collect();
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
    let report = probe_report(cfg, &name, &CancellationToken::new(), TOOL_PROBE_TIMEOUT).await;
    let working = report.status == ProbeStatus::Available;
    let diagnostic = json!({"available":working,"version":report.version});
    (name, diagnostic, working)
}

/// 远程调用只传固定目录 ID；配置路径不进入结果。
pub async fn probe_report(
    cfg: &Config,
    name: &str,
    cancel: &CancellationToken,
    timeout: Duration,
) -> ProbeReport {
    let started = Instant::now();
    let observed_at = chrono::Utc::now();
    let report = |status, version, error_code| ProbeReport {
        status,
        observed_at,
        duration_ms: u64::try_from(started.elapsed().as_millis())
            .unwrap_or(u64::MAX)
            .min(30_000),
        probe_version: 1,
        version,
        error_code,
    };
    if cancel.is_cancelled() {
        return report(ProbeStatus::Error, None, Some(ProbeErrorCode::Canceled));
    }
    let binary = cfg.tool(name);
    let lookup = tokio::task::spawn_blocking(move || {
        let configured_exists = std::path::Path::new(&binary).is_file();
        (configured_exists, which::which(binary))
    });
    let resolved = tokio::select! {
        _=cancel.cancelled()=>return report(ProbeStatus::Error,None,Some(ProbeErrorCode::Canceled)),
        resolved=tokio::time::timeout(timeout.min(TOOL_PROBE_TIMEOUT),lookup)=>resolved,
    };
    let path = match resolved {
        Ok(Ok((_, Ok(path)))) => path,
        Ok(Ok((true, Err(_)))) => {
            return report(
                ProbeStatus::PermissionDenied,
                None,
                Some(ProbeErrorCode::PermissionDenied),
            );
        }
        Ok(Ok((false, Err(_)))) => return report(ProbeStatus::Unsupported, None, None),
        Err(_) => {
            return report(
                ProbeStatus::Timeout,
                None,
                Some(ProbeErrorCode::DeadlineExceeded),
            );
        }
        Ok(Err(_)) => return report(ProbeStatus::Error, None, Some(ProbeErrorCode::SpawnFailed)),
    };
    let args = probe_arguments(name);
    let mut command = Command::new(&path);
    command.args(&args);
    let remaining = timeout
        .min(TOOL_PROBE_TIMEOUT)
        .saturating_sub(started.elapsed());
    if remaining.is_zero() {
        return report(
            ProbeStatus::Timeout,
            None,
            Some(ProbeErrorCode::DeadlineExceeded),
        );
    }
    let result = cancellable_probe(command, cancel, remaining).await;
    let (status, bytes) = match result {
        Ok(result) => result,
        Err(error) => {
            let permission = error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::PermissionDenied);
            let (status, code) = if cancel.is_cancelled() {
                (
                    ProbeStatus::Error,
                    if error.to_string() == "execution canceled" {
                        ProbeErrorCode::Canceled
                    } else {
                        ProbeErrorCode::CleanupFailed
                    },
                )
            } else if started.elapsed() >= timeout || error.to_string() == "execution timed out" {
                (ProbeStatus::Timeout, ProbeErrorCode::DeadlineExceeded)
            } else if permission {
                (
                    ProbeStatus::PermissionDenied,
                    ProbeErrorCode::PermissionDenied,
                )
            } else {
                (ProbeStatus::Error, ProbeErrorCode::SpawnFailed)
            };
            return report(status, None, Some(code));
        }
    };
    let output = String::from_utf8_lossy(&bytes);
    let working = status.success()
        || (name == "codesign"
            && status.code() == Some(2)
            && output.trim().starts_with("Usage: codesign"))
        || (name == "ditto" && output.contains("ditto"));
    if working {
        report(ProbeStatus::Available, normalized_version(&output), None)
    } else {
        report(ProbeStatus::Error, None, Some(ProbeErrorCode::ExitFailure))
    }
}
fn probe_arguments(name: &str) -> Vec<&'static str> {
    match name {
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
    }
}
fn normalized_version(output: &str) -> Option<String> {
    // 只输出数字版本，不回传任意工具输出、路径或凭据。
    output.split_whitespace().take(64).find_map(|word| {
        let word = word
            .trim_matches(['\"', '\'', '(', ')', ','])
            .trim_start_matches('v');
        (word.len() <= 128
            && word.contains('.')
            && word.chars().all(|c| c.is_ascii_digit() || c == '.')
            && word.chars().any(|c| c.is_ascii_digit()))
        .then(|| word.to_owned())
    })
}
pub(crate) async fn cancellable_probe(
    mut command: Command,
    cancel: &CancellationToken,
    timeout: Duration,
) -> anyhow::Result<(std::process::ExitStatus, Vec<u8>)> {
    use std::process::Stdio;
    use tokio::io::AsyncReadExt;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = process::spawn(command)?;
    let stdout = child.stdout().take();
    let stderr = child.stderr().take();
    let out = tokio::spawn(async move {
        let mut bytes = Vec::new();
        if let Some(pipe) = stdout {
            pipe.take(4096).read_to_end(&mut bytes).await?;
        }
        Ok::<_, std::io::Error>(bytes)
    });
    let err = tokio::spawn(async move {
        let mut bytes = Vec::new();
        if let Some(pipe) = stderr {
            pipe.take(4096).read_to_end(&mut bytes).await?;
        }
        Ok::<_, std::io::Error>(bytes)
    });
    let status = process::wait(&mut *child, cancel, timeout).await;
    // wait 完成取消清扫后再返回，终态不能早于子进程回收。
    let mut bytes = Vec::new();
    for mut reader in [out, err] {
        match tokio::time::timeout(Duration::from_secs(2), &mut reader).await {
            Ok(Ok(Ok(part))) => bytes.extend(part),
            _ => {
                reader.abort();
                let _ = reader.await;
            }
        }
    }
    Ok((status?, bytes))
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
    async fn typed_reports_separate_absence_error_permission_and_timeout() {
        let directory = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        for (id, contents, executable) in [
            ("git", "#!/bin/sh\necho token-secret 2.40.1\n", true),
            ("cargo", "#!/bin/sh\nexit 9\n", true),
            ("node", "#!/bin/sh\nexit 0\n", false),
            ("go", "#!/bin/sh\nexec sleep 30\n", true),
        ] {
            let path = directory.path().join(id);
            crate::config::private_write(&path, contents.as_bytes(), executable, true).unwrap();
            cfg.tools
                .insert(id.into(), path.to_string_lossy().into_owned());
        }
        cfg.tools.insert(
            "npm".into(),
            directory
                .path()
                .join("missing")
                .to_string_lossy()
                .into_owned(),
        );
        let token = CancellationToken::new();
        let available = probe_report(&cfg, "git", &token, Duration::from_secs(3)).await;
        assert_eq!(available.status, ProbeStatus::Available);
        assert_eq!(available.version.as_deref(), Some("2.40.1"));
        assert!(
            !serde_json::to_string(&available)
                .unwrap()
                .contains("token-secret")
        );
        assert_eq!(
            probe_report(&cfg, "cargo", &token, Duration::from_secs(3))
                .await
                .status,
            ProbeStatus::Error
        );
        assert_eq!(
            probe_report(&cfg, "node", &token, Duration::from_secs(3))
                .await
                .status,
            ProbeStatus::PermissionDenied
        );
        assert_eq!(
            probe_report(&cfg, "npm", &token, Duration::from_secs(3))
                .await
                .status,
            ProbeStatus::Unsupported
        );
        assert_eq!(
            probe_report(&cfg, "go", &token, Duration::from_millis(50))
                .await
                .status,
            ProbeStatus::Timeout
        );
    }

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

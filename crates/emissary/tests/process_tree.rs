//! [INPUT]: 真实 Emissary 子进程、工作目录和取消信号。
//! [OUTPUT]: 验证 EOF、后代进程清理与服务启动失败。
//! [POS]: Emissary 进程树的跨平台集成验收；PID 标记原子发布。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_emissary::process;
use std::{fs, time::Duration};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn tools_receive_eof_without_reading_parent_input() {
    const CHILD: &str = "ARTIFACTORY_TEST_TOOL_STDIN";
    if std::env::var(CHILD).as_deref() == Ok("1") {
        let command = || {
            #[cfg(unix)]
            let mut command = Command::new("sh");
            #[cfg(unix)]
            command.args(["-c", "cat; printf tool-eof"]);
            #[cfg(windows)]
            let mut command = Command::new("pwsh");
            #[cfg(windows)]
            command.args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "[Console]::In.ReadToEnd(); Write-Output tool-eof",
            ]);
            command
        };
        let timeout = Duration::from_secs(3);
        let (status, bytes) = process::output(command(), &CancellationToken::new(), timeout)
            .await
            .expect("tool must see EOF while the parent input pipe remains open");
        assert!(status.success());
        assert!(String::from_utf8_lossy(&bytes).contains("tool-eof"));
        let (status, bytes) = process::probe(command(), timeout)
            .await
            .expect("probe must see EOF while the parent input pipe remains open");
        assert!(status.success());
        assert!(String::from_utf8_lossy(&bytes).contains("tool-eof"));
        return;
    }
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "tools_receive_eof_without_reading_parent_input",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    // Keep the parent pipe open so inheriting it makes the tool block.
    let _input = child.stdin.take().unwrap();
    let output = tokio::time::timeout(Duration::from_secs(15), child.wait_with_output())
        .await
        .expect("tool input isolation test hung")
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn process_tree_terminates_descendants() {
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join("descendant.pid");
    #[cfg(unix)]
    let mut command = {
        let mut command = Command::new("sh");
        command.args([
            "-c",
            "sh -c 'sleep 120 & wait' & pid=$!; printf '%s\\n' \"$pid\" > \"$MARKER.tmp\" && mv \"$MARKER.tmp\" \"$MARKER\"; wait",
        ]);
        command
    };
    #[cfg(windows)]
    let mut command = {
        let mut command = Command::new("pwsh");
        command.args(["-NoProfile","-Command","$p=Start-Process pwsh -ArgumentList '-NoProfile','-Command','Start-Sleep 120' -PassThru; [IO.File]::WriteAllText($env:MARKER+'.tmp',$p.Id.ToString()); [IO.File]::Move($env:MARKER+'.tmp',$env:MARKER); Wait-Process -Id $p.Id"]);
        command
    };
    command.env("MARKER", &marker);
    let mut child = process::spawn(command).unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        while !marker.exists() {
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await
    .unwrap();
    let pid = fs::read_to_string(&marker)
        .unwrap()
        .trim()
        .parse::<u32>()
        .unwrap();
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(
        process::wait(&mut *child, &cancel, Duration::from_secs(1))
            .await
            .is_err()
    );
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let mut system = sysinfo::System::new();
            system.refresh_processes(
                sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(pid)]),
                true,
            );
            if system
                .process(sysinfo::Pid::from_u32(pid))
                .is_none_or(|p| p.status() == sysinfo::ProcessStatus::Zombie)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
}

#[test]
fn service_run_exits_when_emissary_cannot_start() {
    let directory = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_artifactory-emissary"))
        .args(["run", "--config"])
        .arg(directory.path().join("missing.json"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    #[cfg(unix)]
    {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_artifactory-emissary"))
            .args(["service-run", "--config"])
            .arg(directory.path().join("missing.json"))
            .output()
            .unwrap();
        assert!(!output.status.success());
    }
}

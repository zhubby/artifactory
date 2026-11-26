//! [INPUT]: 隔离宿主、原生服务、私有ext4卷及有界命令夹具。
//! [OUTPUT]: 验证维护、持久补报、可选真实满盘及清理读回。
//! [POS]: 原生服务生命周期验收，拒绝覆盖现有服务和账户。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
#[path = "native_service/cleanup.rs"]
mod cleanup;
#[path = "native_service/command.rs"]
mod command;
#[path = "native_service/disk.rs"]
mod disk;
#[path = "native_service/durable.rs"]
mod durable;
#[path = "support/durable_receiver.rs"]
mod durable_receiver;
#[path = "native_service/maintenance.rs"]
mod maintenance;
use maintenance::queues;
mod support;
#[path = "support/telemetry_gate.rs"]
mod telemetry_gate;
#[path = "support/telemetry_sources.rs"]
mod telemetry_sources;
use artifactory_emissary::{ota, platform, protocol as pb, service};
use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::Duration,
};

const USER: &str = "artifactory-ci";
struct Machine {
    directory: PathBuf,
    binary: PathBuf,
    config: PathBuf,
    account: bool,
    installed: bool,
    password: String,
}
impl Machine {
    fn command(&self, admin: bool, program: impl AsRef<OsStr>, args: &[&OsStr]) -> Output {
        self.try_command(admin, program, args)
            .expect("native test command failed")
    }
    fn try_command(
        &self,
        admin: bool,
        program: impl AsRef<OsStr>,
        args: &[&OsStr],
    ) -> std::io::Result<Output> {
        self.try_command_limited(admin, program, args, 1 << 20)
    }
    fn try_command_limited(
        &self,
        admin: bool,
        program: impl AsRef<OsStr>,
        args: &[&OsStr],
        limit: usize,
    ) -> std::io::Result<Output> {
        let mut command = if admin && cfg!(unix) {
            let mut cmd = Command::new("sudo");
            cmd.args(["-n", "--"]).arg(program);
            cmd
        } else {
            Command::new(program)
        };
        command
            .args(args)
            .env("ARTIFACTORY_SERVICE_PASSWORD", &self.password)
            .env("ARTIFACTORY_TEST_SERVICE_PASSWORD", &self.password)
            .env("EMISSARY_SERVICE_DIRECTORY", &self.directory);
        command::run_with_limit(command, Duration::from_secs(120), limit)
    }
    fn require(&self, program: &str, args: &[&str]) -> String {
        let args = args.iter().map(OsStr::new).collect::<Vec<_>>();
        let output = self.command(true, program, &args);
        assert!(
            output.status.success(),
            "{program} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }
    fn powershell(&self, script: &str) -> String {
        self.require(
            "pwsh",
            &["-NoProfile", "-NonInteractive", "-Command", script],
        )
    }
    fn action(&self, action: &str) -> Output {
        self.try_action(action)
            .expect("native test service command failed")
    }
    fn try_action(&self, action: &str) -> std::io::Result<Output> {
        let mut args = vec![
            OsStr::new("service"),
            OsStr::new("--action"),
            OsStr::new(action),
            OsStr::new("--config"),
            self.config.as_os_str(),
        ];
        let user = if cfg!(windows) {
            format!(".\\{USER}")
        } else {
            USER.into()
        };
        if action == "install" {
            args.extend([OsStr::new("--user"), OsStr::new(&user)])
        }
        self.try_command(true, &self.binary, &args)
    }
    fn act(&self, action: &str) {
        let output = self.action(action);
        assert!(
            output.status.success(),
            "service {action} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    fn state(&self) -> (bool, bool, u32, String) {
        if cfg!(target_os = "linux") {
            let output = self.command(
                false,
                "systemctl",
                &[
                    OsStr::new("show"),
                    OsStr::new(service::NAME),
                    OsStr::new("--property=LoadState,UnitFileState,MainPID,User"),
                ],
            );
            let text = String::from_utf8(output.stdout).unwrap();
            let fields = text
                .lines()
                .filter_map(|line| line.split_once('='))
                .collect::<std::collections::HashMap<_, _>>();
            (
                fields.get("LoadState").is_some_and(|v| *v != "not-found"),
                fields.get("UnitFileState") == Some(&"enabled"),
                fields.get("MainPID").unwrap_or(&"0").parse().unwrap(),
                fields.get("User").unwrap_or(&"").to_string(),
            )
        } else if cfg!(target_os = "macos") {
            let output = self.command(
                false,
                "sudo",
                &[
                    OsStr::new("-n"),
                    OsStr::new("launchctl"),
                    OsStr::new("print"),
                    OsStr::new("system/artifactory-emissary"),
                ],
            );
            let text = String::from_utf8(output.stdout).unwrap();
            let pid = text
                .lines()
                .find_map(|line| {
                    line.trim()
                        .strip_prefix("pid = ")
                        .and_then(|v| v.parse().ok())
                })
                .unwrap_or(0);
            (
                Path::new("/Library/LaunchDaemons/artifactory-emissary.plist").exists(),
                output.status.success(),
                pid,
                if pid > 0 {
                    self.require("ps", &["-o", "user=", "-p", &pid.to_string()])
                        .trim()
                        .into()
                } else {
                    String::new()
                },
            )
        } else {
            let text=self.powershell("$s=Get-CimInstance Win32_Service -Filter \"Name='artifactory-emissary'\"; if(!$s){'{\"installed\":false}'}else{@{installed=$true;enabled=($s.StartMode -eq 'Auto');pid=[int]$s.ProcessId;user=$s.StartName}|ConvertTo-Json -Compress}");
            let state: serde_json::Value = serde_json::from_str(&text).unwrap();
            (
                state["installed"] == true,
                state["enabled"] == true,
                state["pid"].as_u64().unwrap_or(0) as u32,
                state["user"]
                    .as_str()
                    .unwrap_or("")
                    .trim_start_matches(".\\")
                    .into(),
            )
        }
    }
    async fn running(&self) -> u32 {
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let (installed, enabled, pid, user) = self.state();
                if pid > 0 {
                    assert!(installed && enabled);
                    assert_eq!(user, USER);
                    return pid;
                }
                tokio::time::sleep(Duration::from_millis(100)).await
            }
        })
        .await
        .expect("native service did not become ready")
    }
}
impl Drop for Machine {
    fn drop(&mut self) {
        let cleanup = |label: &str, result: std::io::Result<Output>| {
            let succeeded = result.is_ok_and(|output| output.status.success());
            if !succeeded {
                eprintln!("native test cleanup failed: {label}");
            }
            succeeded
        };
        if self.installed {
            let stopped = cleanup("stop", self.try_action("stop"));
            let uninstalled = cleanup("uninstall", self.try_action("uninstall"));
            if !stopped || !uninstalled {
                eprintln!(
                    "native test recovery files retained: {}",
                    self.directory.display()
                );
                return;
            }
        }
        if self.account {
            let removed = if cfg!(target_os = "linux") {
                cleanup(
                    "account removal",
                    self.try_command(true, "userdel", &[OsStr::new(USER)]),
                )
            } else if cfg!(target_os = "macos") {
                cleanup(
                    "account removal",
                    self.try_command(
                        true,
                        "dscl",
                        &[
                            OsStr::new("."),
                            OsStr::new("-delete"),
                            OsStr::new(&format!("/Users/{USER}")),
                        ],
                    ),
                )
            } else {
                let script = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../scripts/native-service-account.ps1");
                cleanup(
                    "account removal",
                    self.try_command(
                        true,
                        "pwsh",
                        &[
                            OsStr::new("-NoProfile"),
                            OsStr::new("-File"),
                            script.as_os_str(),
                            OsStr::new("-Action"),
                            OsStr::new("remove"),
                            OsStr::new("-Directory"),
                            self.directory.as_os_str(),
                        ],
                    ),
                )
            };
            if !removed {
                eprintln!(
                    "native test recovery files retained: {}",
                    self.directory.display()
                );
                return;
            }
        }
        if cfg!(unix) {
            cleanup(
                "directory removal",
                self.try_command(true, "rm", &[OsStr::new("-rf"), self.directory.as_os_str()]),
            );
        } else {
            cleanup(
                "directory removal",
                self.try_command(true, "pwsh", &[
                    OsStr::new("-NoProfile"), OsStr::new("-NonInteractive"),
                    OsStr::new("-Command"),
                    OsStr::new("Remove-Item -LiteralPath $env:EMISSARY_SERVICE_DIRECTORY -Recurse -Force -ErrorAction Stop"),
                ]),
            );
        }
    }
}

// Service stop/restart commands block until the daemon exits. Keep the loopback
// peer running so it can acknowledge logs and terminal events during shutdown.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "changes services and accounts; requires an explicitly enabled disposable runner or dedicated test host"]
async fn native_service_lifecycle() {
    lifecycle(None, false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "changes services/accounts and requires an explicit isolated local TimescaleDB on a dedicated host"]
async fn native_service_durable_recovery() {
    durable::write_evidence(&serde_json::json!({"passed":false,"execution_status":"incomplete"}))
        .expect("native durable initial evidence failed");
    let proof = durable_receiver::isolated(Duration::from_secs(900), |url| async move {
        Ok(lifecycle(Some(&url), false).await)
    })
    .await
    .expect("native durable recovery failed");
    durable::write_evidence(&proof).expect("native durable success evidence failed");
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires a dedicated Linux host, an owned ext4 mount and an isolated local TimescaleDB"]
async fn native_service_disk_recovery() {
    durable::write_evidence(&serde_json::json!({"passed":false,"execution_status":"incomplete"}))
        .expect("native disk initial evidence failed");
    let name = std::env::var("ARTIFACTORY_NATIVE_DISK_DATABASE")
        .expect("parent-reserved database required");
    let proof =
        durable_receiver::isolated_named(Duration::from_secs(900), &name, |url| async move {
            Ok(lifecycle(Some(&url), true).await)
        })
        .await
        .expect("native disk recovery failed");
    durable::write_evidence(&proof).expect("native disk success evidence failed");
}

async fn lifecycle(database: Option<&str>, disk_fault: bool) -> serde_json::Value {
    static EXCLUSIVE_HOST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _exclusive = EXCLUSIVE_HOST.lock().await;
    assert_eq!(
        std::env::var("ARTIFACTORY_NATIVE_SERVICE_TESTS").as_deref(),
        Ok("1")
    );
    let dedicated =
        std::env::var("ARTIFACTORY_NATIVE_SERVICE_SCOPE").as_deref() == Ok("dedicated-host");
    if !dedicated {
        assert_eq!(std::env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
        assert_eq!(
            std::env::var("RUNNER_ENVIRONMENT").as_deref(),
            Ok("github-hosted")
        );
    }
    let (os, arch) = platform();
    assert_eq!(
        std::env::var("ARTIFACTORY_NATIVE_EXPECTED_PLATFORM").unwrap(),
        format!("{os}/{arch}")
    );
    if !dedicated {
        assert_eq!(
            std::env::var("RUNNER_ARCH").unwrap(),
            if arch == "amd64" { "X64" } else { "ARM64" }
        );
        assert_eq!(
            std::env::var("RUNNER_OS").unwrap(),
            match os {
                "linux" => "Linux",
                "darwin" => "macOS",
                _ => "Windows",
            }
        );
    }
    let base = if disk_fault {
        disk::mount().expect("native disk requires its private ext4 mount")
    } else if cfg!(windows) {
        PathBuf::from(std::env::var_os("ProgramData").expect("ProgramData required"))
    } else {
        PathBuf::from("/tmp")
    };
    let directory = tempfile::Builder::new()
        .prefix("emissary-service-")
        .tempdir_in(base)
        .unwrap()
        .keep();
    // Resolve macOS /tmp before changing ownership. Git for Windows rejects
    // the device namespace prefix that Windows canonicalize would introduce.
    #[cfg(unix)]
    let directory = directory.canonicalize().unwrap();
    let private = directory.join("private data %n $HOME");
    let bin = directory.join("emissary bin %i $HOME");
    fs::create_dir(&private).unwrap();
    fs::create_dir(&bin).unwrap();
    let mut machine = Machine {
        binary: bin.join(ota::binary_name()),
        config: private.join("emissary config.json"),
        directory,
        account: false,
        installed: false,
        password: format!("Emissary-{}!", uuid::Uuid::new_v4()),
    };
    assert!(
        !machine.state().0,
        "refusing to replace an existing service"
    );
    fs::copy(env!("CARGO_BIN_EXE_artifactory-emissary"), &machine.binary).unwrap();
    let mut fixture = support::Fixture::start(&private, &[&rustls::version::TLS13]).await;
    // Service managers have a different PATH. Use the installed test toolchain,
    // including on Rosetta hosts whose Apple Git shim lacks an Intel libxcrun.
    for name in if cfg!(windows) {
        &["git", "pwsh"][..]
    } else {
        &["git"][..]
    } {
        fixture.cfg.tools.insert(
            (*name).into(),
            which::which(name)
                .expect("native acceptance tool is not installed")
                .to_string_lossy()
                .into(),
        );
    }
    let mut durable = if let Some(url) = database {
        Some(durable::Prepared::prepare(url, &mut fixture).await.unwrap())
    } else {
        None
    };
    fixture.cfg.save(&machine.config).unwrap();
    let maintenance = if let Some(durable) = &durable {
        maintenance::Prepared::prepare_with_queues(&machine, &fixture.cfg, durable.queues())
    } else {
        maintenance::Prepared::prepare(&machine, &fixture.cfg).await
    };
    // === 上游边界 ===
    // HTTP 源码服务器属于测试进程，仓库保留测试账户所有权。
    let repo = machine.directory.join("upstream repository %n $HOME");
    fs::create_dir(&repo).unwrap();
    let git = |args: &[&str]| {
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(&repo)
            .args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@localhost",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args);
        let output = command::run(command, Duration::from_secs(30)).unwrap();
        assert!(
            output.status.success(),
            "fixture Git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };
    git(&["init"]);
    fs::write(repo.join("README"), "fixture").unwrap();
    git(&["add", "."]);
    git(&["commit", "-m", "fixture"]);
    let commit = git(&["rev-parse", "HEAD"]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&machine.directory, fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(&machine.binary, fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(&private, fs::Permissions::from_mode(0o700)).unwrap();
        for name in ["ca.pem", "client.pem", "client.key"] {
            fs::set_permissions(private.join(name), fs::Permissions::from_mode(0o600)).unwrap();
        }
        assert!(
            !machine
                .command(false, "id", &[OsStr::new(USER)])
                .status
                .success(),
            "refusing existing test account"
        );
        if os == "linux" {
            machine.require(
                "useradd",
                &[
                    "--system",
                    "--home-dir",
                    private.to_str().unwrap(),
                    "--shell",
                    "/usr/sbin/nologin",
                    USER,
                ],
            );
            machine.account = true;
        } else {
            let users = machine.require("dscl", &[".", "-list", "/Users", "UniqueID"]);
            let mut uid = 6001;
            while users.split_whitespace().any(|word| word == uid.to_string()) {
                uid += 1;
            }
            let record = format!("/Users/{USER}");
            machine.require("dscl", &[".", "-create", &record]);
            machine.account = true;
            for (key, value) in [
                ("UniqueID", uid.to_string()),
                ("PrimaryGroupID", "20".into()),
                ("NFSHomeDirectory", private.to_str().unwrap().into()),
                ("UserShell", "/usr/bin/false".into()),
                ("IsHidden", "1".into()),
            ] {
                machine.require("dscl", &[".", "-create", &record, key, &value]);
            }
        }
        machine.require("chown", &["-R", USER, private.to_str().unwrap()]);
    }
    if cfg!(windows) {
        let script =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/native-service-account.ps1");
        machine.account = true;
        machine.require(
            "pwsh",
            &[
                "-NoProfile",
                "-File",
                script.to_str().unwrap(),
                "-Action",
                "create",
                "-Directory",
                machine.directory.to_str().unwrap(),
            ],
        );
    }
    assert_eq!(git(&["rev-parse", "HEAD"]), commit);
    machine.installed = true;
    machine.act("install");
    machine.act("start");
    let mut first = fixture.connection().await;
    assert!(
        first.hello.emissary_id == fixture.cfg.emissary_id,
        "native service identity changed"
    );
    first = maintenance.exercise(&machine, &mut fixture, first).await;
    if let Some(durable) = &mut durable {
        let disk = if disk_fault {
            Some(
                durable
                    .disk_fault(&machine, &fixture.cfg, &mut first)
                    .await
                    .unwrap(),
            )
        } else {
            None
        };
        let mut proof = durable.deliver(&machine, &fixture.cfg).await.unwrap();
        if let Some(disk) = disk {
            proof["native_disk_fault"] = disk;
        }
        machine.act("stop");
        first.closed().await;
        machine.act("uninstall");
        machine.installed = false;
        assert!(
            !machine.state().0,
            "native durable service remains installed"
        );
        let directory = machine.directory.clone();
        drop(machine);
        cleanup::verify(&directory).unwrap();
        proof["service_account_directory_removed"] = serde_json::json!(true);
        return proof;
    }
    let first_pid = machine.running().await;
    let long = if cfg!(windows) {
        "$p=Start-Process pwsh -ArgumentList '-NoProfile','-Command','Start-Sleep 120' -PassThru; [IO.File]::WriteAllText('child.pid',$p.Id.ToString()); Write-Output tree-started; Wait-Process -Id $p.Id"
    } else {
        "sh -c 'sleep 120 & wait' & echo $! > child.pid; echo tree-started; wait"
    };
    for action in ["restart", "stop"] {
        let attempt = uuid::Uuid::new_v4().to_string();
        first
            .commands
            .send(pb::Command {
                kind: "assign".into(),
                attempt_id: attempt.clone(),
                lease_token: "native-fixture".into(),
                lease_seconds: 90,
                assignment: Some(pb::Assignment {
                    source_manifest_json: fixture.source_manifest(&repo, &commit).await,
                    timeout_seconds: 120,
                    steps: vec![pb::Step {
                        name: "tree".into(),
                        shell: if cfg!(windows) { "pwsh" } else { "sh" }.into(),
                        run: long.into(),
                        timeout_seconds: 120,
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            })
            .await
            .unwrap();
        let marker = fixture
            .cfg
            .root
            .join("work")
            .join(&attempt)
            .join("source/child.pid");
        let pid = tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let contents = if cfg!(unix) {
                    let output = machine.command(true, "cat", &[marker.as_os_str()]);
                    output.status.success().then_some(output.stdout)
                } else {
                    // Win32 accepts mixed separators; cmd's type command does not.
                    fs::read(&marker).ok()
                };
                if let Some(contents) = contents
                    && let Ok(pid) = String::from_utf8_lossy(&contents).trim().parse::<u32>()
                {
                    break pid;
                }
                while let Ok(event) = first.events.try_recv() {
                    assert_ne!(
                        event.kind, "finished",
                        "native Worker finished before creating its child: {}",
                        event.payload_json
                    );
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .expect("native Worker never started");
        machine.act(action);
        first.event("finished").await;
        first.closed().await;
        let output = if cfg!(unix) {
            machine.command(
                true,
                "ps",
                &[
                    OsStr::new("-o"),
                    OsStr::new("stat="),
                    OsStr::new("-p"),
                    OsStr::new(&pid.to_string()),
                ],
            )
        } else {
            machine.command(
                true,
                "pwsh",
                &[
                    OsStr::new("-Command"),
                    OsStr::new(&format!(
                        "if(Get-Process -Id {pid} -ErrorAction SilentlyContinue){{exit 1}}"
                    )),
                ],
            )
        };
        if cfg!(unix) {
            assert!(
                !output.status.success()
                    || String::from_utf8_lossy(&output.stdout)
                        .trim()
                        .starts_with('Z'),
                "descendant survived service {action}"
            );
        } else {
            assert!(
                output.status.success(),
                "descendant survived service {action}"
            );
        }
        #[cfg(unix)]
        assert!(String::from_utf8_lossy(&fixture.logs(&attempt)).contains("tree-started"));
        if action == "stop" {
            assert_eq!(machine.state().2, 0);
            machine.act("start");
        }
        first = fixture.connection().await;
        assert_ne!(machine.running().await, first_pid);
        let task = uuid::Uuid::new_v4().to_string();
        first
            .commands
            .send(pb::Command {
                kind: "assign".into(),
                attempt_id: task,
                lease_token: "followup".into(),
                lease_seconds: 90,
                assignment: Some(pb::Assignment {
                    source_manifest_json: fixture.source_manifest(&repo, &commit).await,
                    timeout_seconds: 60,
                    steps: vec![pb::Step {
                        name: "followup".into(),
                        shell: if cfg!(windows) { "pwsh" } else { "sh" }.into(),
                        run: "echo followup".into(),
                        timeout_seconds: 30,
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            })
            .await
            .unwrap();
        let finished = first.event("finished").await;
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&finished.payload_json).unwrap()["status"],
            "succeeded"
        );
    }
    machine.act("stop");
    first.closed().await;
    machine.act("uninstall");
    machine.installed = false;
    assert!(!machine.state().0);
    let directory = machine.directory.clone();
    drop(machine);
    cleanup::verify(&directory).expect("native lifecycle cleanup not confirmed");
    serde_json::Value::Null
}

//! [INPUT]: 本机执行程序、mTLS 平台夹具和 OTA 签名组件。
//! [OUTPUT]: 验证 TLS 约束、运行时停机、OTA 升级和回滚。
//! [POS]: 使用真实子进程；首连前暴露监督任务退出原因。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
mod support;
#[cfg(unix)]
use artifactory_emissary::config;
use artifactory_emissary::{client, ota, platform, protocol as pb};
use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signer, SigningKey};
use std::{fs, path::Path, time::Duration};
#[cfg(unix)]
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

#[cfg(unix)]
#[tokio::test]
async fn log_batches_reuse_one_mtls_transport_and_retry_from_spool() {
    use std::sync::atomic::Ordering;
    let directory = tempfile::tempdir().unwrap();
    let mut fixture = support::Fixture::start(directory.path(), &[&rustls::version::TLS13]).await;
    fixture.metrics.fail_next_log.store(true, Ordering::SeqCst);
    let config = directory.path().join("emissary.json");
    fixture.cfg.save(&config).unwrap();
    let repo = directory.path().join("repository");
    fs::create_dir(&repo).unwrap();
    for args in [
        vec!["init"],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@localhost",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--allow-empty",
            "-m",
            "fixture",
        ],
    ] {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    let commit = std::process::Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    let mut daemon = Command::new(env!("CARGO_BIN_EXE_artifactory-emissary"))
        .args(["run", "--config"])
        .arg(&config)
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut connection = fixture.connection().await;
    let attempt = uuid::Uuid::new_v4().to_string();
    connection
        .commands
        .send(pb::Command {
            kind: "assign".into(),
            attempt_id: attempt.clone(),
            lease_token: "log-transport-fixture".into(),
            lease_seconds: 90,
            assignment: Some(pb::Assignment {
                source_manifest_json: fixture
                    .source_manifest(&repo, String::from_utf8(commit.stdout).unwrap().trim())
                    .await,
                timeout_seconds: 30,
                steps: vec![pb::Step {
                    name: "compile".into(),
                    shell: "sh".into(),
                    run:
                        "for i in 1 2 3 4 5 6 7 8; do printf 'chunk-%s\\n' \"$i\"; sleep 0.4; done"
                            .into(),
                    timeout_seconds: 15,
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        })
        .await
        .unwrap();
    let finished = connection.event("finished").await;
    assert!(finished.payload_json.contains("succeeded"));
    let output = String::from_utf8(fixture.logs(&attempt)).unwrap();
    for index in 1..=8 {
        assert_eq!(
            output.matches(&format!("chunk-{index}\n")).count(),
            1,
            "missing or repeated chunk {index}"
        );
    }
    assert!(fixture.metrics.log_batches.load(Ordering::SeqCst) >= 3);
    assert_eq!(
        fixture.metrics.accepted.load(Ordering::SeqCst),
        3,
        "control, source material, and log upload use three TLS connections"
    );
    assert!(
        std::process::Command::new("kill")
            .args(["-TERM", &daemon.id().unwrap().to_string()])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        tokio::time::timeout(Duration::from_secs(15), daemon.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
}

#[tokio::test]
async fn pinned_mtls_requires_tls13() {
    for (version, accepted) in [
        (&rustls::version::TLS13, true),
        (&rustls::version::TLS12, false),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut fixture = support::Fixture::start(directory.path(), &[version]).await;
        let result = client::control(&fixture.cfg).await;
        if !accepted {
            assert!(result.is_err(), "TLS 1.2 was accepted");
            continue;
        }
        let mut client = result.unwrap();
        let (tx, rx) = tokio::sync::mpsc::channel(2);
        tx.send(pb::EmissaryEvent {
            kind: "hello".into(),
            ..Default::default()
        })
        .await
        .unwrap();
        let stream = client
            .connect(tokio_stream::wrappers::ReceiverStream::new(rx))
            .await
            .unwrap();
        let mut connection = fixture.connection().await;
        assert_eq!(connection.event("hello").await.kind, "hello");
        drop(stream);
        drop(tx);
        let other = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        fs::write(&fixture.cfg.ca_file, other.cert.pem()).unwrap();
        assert!(
            client::dial(&fixture.cfg, false).await.is_err(),
            "untrusted server CA was accepted"
        );
        fixture.cfg.endpoint = "http://127.0.0.1:1".into();
        assert!(
            client::dial(&fixture.cfg, true).await.is_err(),
            "plaintext endpoint was accepted"
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn slow_acknowledgements_do_not_starve_terminal_result() {
    let directory = tempfile::tempdir().unwrap();
    let mut fixture = support::Fixture::with_ack_delay(
        directory.path(),
        &[&rustls::version::TLS13],
        Duration::from_millis(50),
    )
    .await;
    let config = directory.path().join("emissary.json");
    fixture.cfg.save(&config).unwrap();
    let repo = directory.path().join("repository");
    fs::create_dir(&repo).unwrap();
    for args in [
        vec!["init"],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@localhost",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--allow-empty",
            "-m",
            "fixture",
        ],
    ] {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    let commit = std::process::Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    let mut daemon = Command::new(env!("CARGO_BIN_EXE_artifactory-emissary"))
        .args(["run", "--config"])
        .arg(&config)
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut connection = fixture.connection().await;
    let attempt = uuid::Uuid::new_v4().to_string();
    connection
        .commands
        .send(pb::Command {
            kind: "assign".into(),
            attempt_id: attempt.clone(),
            lease_token: "delayed-ack".into(),
            lease_seconds: 90,
            assignment: Some(pb::Assignment {
                source_manifest_json: fixture
                    .source_manifest(&repo, String::from_utf8(commit.stdout).unwrap().trim())
                    .await,
                timeout_seconds: 90,
                steps: (0..20)
                    .map(|index| pb::Step {
                        name: format!("step-{index}"),
                        shell: "sh".into(),
                        run: "true".into(),
                        timeout_seconds: 10,
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            }),
            ..Default::default()
        })
        .await
        .unwrap();
    let finished = connection.event("finished").await;
    assert_eq!(finished.attempt_id, attempt);
    assert!(finished.payload_json.contains("succeeded"));
    assert!(
        std::process::Command::new("kill")
            .args(["-TERM", &daemon.id().unwrap().to_string()])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        tokio::time::timeout(Duration::from_secs(15), daemon.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
}

async fn phase(directory: &Path, wanted: &str) -> ota::State {
    tokio::time::timeout(Duration::from_secs(75), async {
        loop {
            if let Ok(state) = ota::snapshot(directory)
                && state.phase == wanted
            {
                return state;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("OTA phase {wanted} not reached"))
}
async fn apply(
    connection: &support::Connection,
    key: &SigningKey,
    url: &str,
    bytes: &[u8],
    generation: i64,
    version: &str,
) {
    let (os, arch) = platform();
    let manifest = serde_json::to_vec(&ota::Manifest {
        version: version.into(),
        commit: "a".repeat(40),
        protocol_min: 1,
        protocol_max: 1,
        assets: vec![ota::Asset::from_bytes(os, arch, bytes)],
    })
    .unwrap();
    let request = ota::Request {
        id: format!("update-{generation}"),
        generation,
        action: "apply".into(),
        manifest: String::from_utf8(manifest.clone()).unwrap(),
        signature: STANDARD.encode(key.sign(&manifest).to_bytes()),
        url: url.into(),
    };
    connection
        .commands
        .send(pb::Command {
            kind: "emissary_update".into(),
            payload_json: serde_json::to_string(&request).unwrap(),
            ..Default::default()
        })
        .await
        .unwrap();
}
#[tokio::test]
async fn ota_supervisor_mtls_upgrade_and_rollback() {
    let directory = tempfile::tempdir().unwrap();
    // Linux keeps DWARF in the executable: a debug build can exceed the release
    // size limit. Exercise the signed runtime image with its debug data removed.
    let image = directory.path().join(ota::binary_name());
    fs::copy(env!("CARGO_BIN_EXE_artifactory-emissary"), &image).unwrap();
    #[cfg(target_os = "linux")]
    {
        let output = std::process::Command::new("strip")
            .arg("--strip-debug")
            .arg(&image)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "could not prepare native OTA image: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert!(fs::metadata(&image).unwrap().len() <= ota::MAX_BINARY_SIZE);
    let mut fixture = support::Fixture::start(directory.path(), &[&rustls::version::TLS13]).await;
    let path = directory.path().join("emissary.json");
    fixture.cfg.save(&path).unwrap();
    let ota_dir = fixture.cfg.root.join("ota");
    let key = SigningKey::from_bytes(&[17; 32]);
    ota::initialize(
        &path,
        &ota_dir,
        &image,
        "bootstrap",
        vec![STANDARD.encode(key.verifying_key().to_bytes())],
        true,
    )
    .unwrap();
    let body = fs::read(&image).unwrap();
    let good = body.clone();
    let bad = b"invalid native executable\n";
    let server = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", server.local_addr().unwrap());
    let http = tokio::spawn(async move {
        axum::serve(
            server,
            axum::Router::new()
                .route(
                    "/good",
                    axum::routing::get(move || {
                        let body = good.clone();
                        async move { body }
                    }),
                )
                .route("/bad", axum::routing::get(|| async { bad.to_vec() })),
        )
        .await
        .unwrap()
    });
    let cancel = CancellationToken::new();
    let stop = cancel.clone();
    let mut supervisor =
        tokio::spawn(async move { ota::supervise(&path, "test-launcher", stop).await });
    let mut first = tokio::select! {
        connection = fixture.connection() => connection,
        result = &mut supervisor => panic!("OTA supervisor exited before initial mTLS connection: {result:?}"),
    };
    assert!(first.hello.payload_json.contains("emissary-ota-v1"));
    apply(
        &first,
        &key,
        &format!("{url}/good"),
        &body,
        1,
        "replacement",
    )
    .await;
    let state = phase(&ota_dir, "succeeded").await;
    assert_eq!(state.previous.version, "bootstrap");
    assert_eq!(state.current.version, "replacement");
    first.closed().await;
    let mut candidate = fixture.connection().await;
    apply(
        &candidate,
        &key,
        &format!("{url}/bad"),
        bad,
        2,
        "bad-candidate",
    )
    .await;
    let state = phase(&ota_dir, "rolled_back").await;
    assert_eq!(state.current.version, "replacement");
    assert!(!state.error.is_empty());
    candidate.closed().await;
    let restored = fixture.connection().await;
    apply(
        &restored,
        &key,
        &format!("{url}/good"),
        &body,
        1,
        "replacement",
    )
    .await;
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(ota::snapshot(&ota_dir).unwrap().generation, 2);
    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(15), supervisor)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    http.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn foreground_and_ota_shutdown_stop_authenticated_worker_tree() {
    for supervised in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let mut fixture =
            support::Fixture::start(directory.path(), &[&rustls::version::TLS13]).await;
        let path = directory.path().join("emissary.json");
        fixture.cfg.save(&path).unwrap();
        if supervised {
            ota::initialize(
                &path,
                &fixture.cfg.root.join("ota"),
                Path::new(env!("CARGO_BIN_EXE_artifactory-emissary")),
                "bootstrap",
                vec![STANDARD.encode(SigningKey::from_bytes(&[19; 32]).verifying_key().to_bytes())],
                true,
            )
            .unwrap();
        }
        let repo = directory.path().join("repository");
        fs::create_dir(&repo).unwrap();
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
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
                .args(args)
                .output()
                .unwrap();
            assert!(output.status.success());
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        };
        git(&["init"]);
        fs::write(repo.join("README"), "fixture").unwrap();
        git(&["add", "."]);
        git(&["commit", "-m", "fixture"]);
        let commit = git(&["rev-parse", "HEAD"]);
        let mut daemon = Command::new(env!("CARGO_BIN_EXE_artifactory-emissary"))
            .args([if supervised { "service-run" } else { "run" }, "--config"])
            .arg(&path)
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut connection = fixture.connection().await;
        let attempt = uuid::Uuid::new_v4().to_string();
        connection.commands.send(pb::Command{kind:"assign".into(),attempt_id:attempt.clone(),lease_token:"fixture-lease".into(),lease_seconds:90,assignment:Some(pb::Assignment{source_manifest_json:fixture.source_manifest(&repo,&commit).await,timeout_seconds:120,steps:vec![pb::Step{name:"tree".into(),shell:"sh".into(),run:"sh -c 'sleep 120 & wait' & pid=$!; printf '%s\\n' \"$pid\" > child.pid.tmp && mv child.pid.tmp child.pid; echo tree-started; wait".into(),timeout_seconds:120,..Default::default()}],..Default::default()}),..Default::default()}).await.unwrap();
        let marker = fixture
            .cfg
            .root
            .join("work")
            .join(&attempt)
            .join("source/child.pid");
        tokio::time::timeout(Duration::from_secs(15), async {
            while !marker.exists() {
                tokio::time::sleep(Duration::from_millis(30)).await
            }
        })
        .await
        .unwrap();
        let pid = fs::read_to_string(&marker)
            .unwrap()
            .trim()
            .parse::<u32>()
            .unwrap();
        assert!(
            std::process::Command::new("kill")
                .args(["-TERM", &daemon.id().unwrap().to_string()])
                .status()
                .unwrap()
                .success()
        );
        let finished = connection.event("finished").await;
        assert_eq!(finished.attempt_id, attempt);
        assert!(finished.payload_json.contains("failed"));
        assert!(
            tokio::time::timeout(Duration::from_secs(15), daemon.wait())
                .await
                .unwrap()
                .unwrap()
                .success()
        );
        connection.closed().await;
        let mut system = sysinfo::System::new();
        system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        assert!(
            system
                .process(sysinfo::Pid::from_u32(pid))
                .is_none_or(|p| p.status() == sysinfo::ProcessStatus::Zombie),
            "worker descendant survived shutdown, supervised={supervised}"
        );
        assert!(!fixture.cfg.root.join("work").join(&attempt).exists());
        assert!(
            String::from_utf8_lossy(&fixture.logs(&attempt)).contains("tree-started"),
            "shutdown lost unacknowledged logs"
        );
        let spool = fixture.cfg.root.join("spool").join(&attempt);
        assert!(!spool.exists() || fs::read_dir(spool).unwrap().next().is_none());
        config::Config::load(&path).unwrap();
    }
}

#[cfg(unix)]
#[tokio::test]
async fn worker_kills_process_tree_on_parent_death_and_lease_expiry() {
    use artifactory_emissary::{
        config::Config,
        protocol::{Assignment, Step},
        worker::{Event, Input},
    };
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    for cause in ["parent-death", "lease-expiry", "sigterm"] {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("repo");
        fs::create_dir(&repo).unwrap();
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args([
                    "-c",
                    "user.name=Test",
                    "-c",
                    "user.email=test@localhost",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .output()
                .unwrap();
            assert!(output.status.success());
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        };
        git(&["init"]);
        fs::write(repo.join("README"), "fixture").unwrap();
        git(&["add", "."]);
        git(&["commit", "-m", "fixture"]);
        let commit = git(&["rev-parse", "HEAD"]);
        let id = uuid::Uuid::new_v4().to_string();
        let root = directory.path().join("managed");
        let fixture = support::Fixture::start(directory.path(), &[&rustls::version::TLS13]).await;
        let input = Input {
            kind: "start".into(),
            config: Config {
                root: root.clone(),
                ..fixture.cfg.clone()
            },
            attempt_id: id.clone(),
            lease_token: "test".into(),
            lease_seconds: if cause == "lease-expiry" { 2 } else { 30 },
            assignment: Some(Assignment {
                source_manifest_json: fixture.source_manifest(&repo, &commit).await,
                timeout_seconds: 60,
                steps: vec![Step {
                    name: "long".into(),
                    shell: "sh".into(),
                    run: "sh -c 'sleep 120 & wait' & pid=$!; printf '%s\\n' \"$pid\" > child.pid.tmp && mv child.pid.tmp child.pid; wait".into(),
                    timeout_seconds: 60,
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut child = Command::new(env!("CARGO_BIN_EXE_artifactory-emissary"))
            .arg("worker")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let mut line = serde_json::to_vec(&input).unwrap();
        line.push(b'\n');
        stdin.write_all(&line).await.unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let drain = tokio::spawn(async move {
            let mut events = Vec::new();
            while let Some(line) = lines.next_line().await.unwrap() {
                events.push(serde_json::from_str::<Event>(&line).unwrap());
            }
            events
        });
        let marker = root.join("work").join(&id).join("source/child.pid");
        tokio::time::timeout(Duration::from_secs(10), async {
            while !marker.exists() {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .unwrap();
        let pid = fs::read_to_string(marker)
            .unwrap()
            .trim()
            .parse::<u32>()
            .unwrap();
        if cause == "parent-death" {
            drop(stdin);
        } else if cause == "sigterm" {
            assert!(
                std::process::Command::new("kill")
                    .args(["-TERM", &child.id().unwrap().to_string()])
                    .status()
                    .unwrap()
                    .success()
            );
        }
        tokio::time::timeout(Duration::from_secs(10), child.wait())
            .await
            .unwrap()
            .unwrap();
        let events = drain.await.unwrap();
        assert!(
            events
                .iter()
                .any(|e| e.kind == "finished" && e.payload["status"] == "failed"),
            "{cause}"
        );
        assert!(
            !root.join("work").join(&id).exists(),
            "workspace remains: {cause}"
        );
        let mut system = sysinfo::System::new();
        system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        assert!(
            system
                .process(sysinfo::Pid::from_u32(pid))
                .is_none_or(|p| p.status() == sysinfo::ProcessStatus::Zombie),
            "descendant remains: {cause}"
        );
    }
}

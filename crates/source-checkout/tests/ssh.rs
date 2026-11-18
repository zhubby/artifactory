//! [INPUT]: 依赖显式启用的独立 sshd、临时密钥和真实 Git。
//! [OUTPUT]: 验证 SSH 固定提交、密钥认证及主机信任拒绝。
//! [POS]: SSH 传输验收，不扫描宿主密钥或修改系统配置。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
#![cfg(unix)]

use artifactory_source_checkout::{
    Checkout, GitCredential, Options, SourceCredential, SourceManifest, SourceRepository,
};
use process_wrap::tokio::{CommandWrap, KillOnDrop, ProcessGroup};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

fn command(program: &str, args: &[&str]) -> String {
    let output = std::process::Command::new(program)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{program}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}
fn git(path: &Path, args: &[&str]) -> String {
    let mut all = vec![
        "-C",
        path.to_str().unwrap(),
        "-c",
        "user.name=Source SSH test",
        "-c",
        "user.email=test@localhost",
        "-c",
        "commit.gpgsign=false",
    ];
    all.extend_from_slice(args);
    command("git", &all)
}
fn key(path: &Path) {
    command(
        "ssh-keygen",
        &[
            "-q",
            "-t",
            "ed25519",
            "-N",
            "",
            "-f",
            path.to_str().unwrap(),
        ],
    );
}
fn host_trust(key: &Path, port: u16) -> String {
    let public = fs::read_to_string(key.with_extension("pub")).unwrap();
    let mut fields = public.split_whitespace();
    format!(
        "[127.0.0.1]:{port} {} {}\n",
        fields.next().unwrap(),
        fields.next().unwrap()
    )
}
fn shell_quote(value: &Path) -> String {
    format!("'{}'", value.to_str().unwrap().replace('\'', "'\\''"))
}
struct Container(String);
impl Drop for Container {
    fn drop(&mut self) {
        let _ = std::process::Command::new("docker")
            .args(["rm", "-f", &self.0])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

#[tokio::test]
#[ignore = "set ARTIFACTORY_TEST_NATIVE_SSHD=1 or ARTIFACTORY_TEST_SSHD_IMAGE to start isolated SSH"]
async fn ssh_checkout_requires_bound_key_and_host_trust_for_fixed_commit() {
    let image = std::env::var("ARTIFACTORY_TEST_SSHD_IMAGE").ok();
    if image.is_none() && std::env::var("ARTIFACTORY_TEST_NATIVE_SSHD").as_deref() != Ok("1") {
        eprintln!("SSH fixture disabled; explicitly select native sshd or a local Docker image");
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let host = root.path().join("host");
    let client = root.path().join("client");
    let wrong = root.path().join("wrong");
    for path in [&host, &client, &wrong] {
        key(path);
    }
    let repository = root.path().join("repo.git");
    fs::create_dir(&repository).unwrap();
    git(&repository, &["init", "-b", "source"]);
    fs::write(repository.join("main.c"), "pinned source\n").unwrap();
    git(&repository, &["add", "."]);
    git(&repository, &["commit", "-m", "pinned"]);
    let fixed = git(&repository, &["rev-parse", "HEAD"]);
    fs::write(repository.join("main.c"), "moving branch\n").unwrap();
    git(&repository, &["commit", "-am", "moved"]);
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let username = if image.is_some() {
        "root".to_owned()
    } else {
        command("id", &["-un"])
    };
    let authorized = root.path().join("authorized_keys");
    fs::copy(client.with_extension("pub"), &authorized).unwrap();
    let config = root.path().join("sshd_config");
    let server_path = |path: &Path| {
        if image.is_some() {
            PathBuf::from("/fixture").join(path.strip_prefix(root.path()).unwrap())
        } else {
            path.to_owned()
        }
    };
    let server_port = if image.is_some() { 2222 } else { port };
    let listen = if image.is_some() {
        "0.0.0.0"
    } else {
        "127.0.0.1"
    };
    let git_binary = if image.is_some() {
        "/usr/bin/git".to_owned()
    } else {
        command("which", &["git"])
    };
    let server_repository = server_path(&repository);
    fs::write(&config, format!("HostKey {}\nPort {server_port}\nListenAddress {listen}\nPidFile /tmp/artifactory-test-sshd.pid\nUsePAM no\nUseDNS no\nStrictModes {}\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nPubkeyAuthentication yes\nAuthenticationMethods publickey\nPermitRootLogin prohibit-password\nAllowUsers {username}\nAllowAgentForwarding no\nAllowTcpForwarding no\nX11Forwarding no\nAuthorizedKeysFile {}\nForceCommand {} -c safe.directory={} upload-pack {}\n",server_path(&host).display(),if image.is_some(){"no"}else{"yes"},server_path(&authorized).display(),shell_quote(Path::new(&git_binary)),shell_quote(&server_repository),shell_quote(&server_repository))).unwrap();
    let diagnostics = root.path().join("sshd.log");
    let container = image.as_ref().map(|_| {
        Container(format!(
            "source-checkout-sshd-{}",
            uuid::Uuid::new_v4().simple()
        ))
    });
    let mut daemon = if let (Some(image), Some(container)) = (&image, &container) {
        let mut daemon = Command::new("docker");
        daemon.args([
            "run",
            "--rm",
            "--pull=never",
            "--platform",
            if cfg!(target_arch = "aarch64") {
                "linux/arm64"
            } else {
                "linux/amd64"
            },
            "--name",
            &container.0,
            "-p",
            &format!("127.0.0.1:{port}:{server_port}"),
            "--mount",
            &format!(
                "type=bind,src={},dst=/fixture,readonly",
                root.path().canonicalize().unwrap().display()
            ),
            image,
        ]);
        daemon
    } else {
        Command::new("/usr/sbin/sshd")
    };
    daemon
        .args(["-D", "-e", "-f"])
        .arg(server_path(&config))
        .stdout(Stdio::null())
        .stderr(fs::File::create(&diagnostics).unwrap());
    let mut wrap = CommandWrap::from(daemon);
    wrap.wrap(KillOnDrop).wrap(ProcessGroup::leader());
    let mut daemon = wrap.spawn().unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            assert!(
                daemon.try_wait().unwrap().is_none(),
                "isolated sshd stopped: {}",
                fs::read_to_string(&diagnostics).unwrap()
            );
            if tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let url = format!("ssh://{username}@127.0.0.1:{port}/repo.git");
    let manifest = SourceManifest::new(
        uuid::Uuid::new_v4(),
        vec![SourceRepository {
            path: String::new(),
            upstream_url: url.clone(),
            commit_sha: fixed.clone(),
            tags: Default::default(),
            lfs_objects: Vec::new(),
        }],
    )
    .unwrap();
    let auth = root.path().join("auth");
    let checkout = Checkout::new(Options {
        auth_root: auth.clone(),
        timeout: Duration::from_secs(10),
        ..Default::default()
    });
    let cancel = CancellationToken::new();
    for (name, identity, known_hosts) in [
        ("wrong-host", &client, host_trust(&wrong, port)),
        ("wrong-client", &wrong, host_trust(&host, port)),
        ("trusted", &client, host_trust(&host, port)),
    ] {
        let credentials = [SourceCredential {
            upstream_url: url.clone(),
            credential: GitCredential {
                kind: "ssh".into(),
                username: username.clone(),
                secret: fs::read_to_string(identity).unwrap(),
                known_hosts,
            },
        }];
        let workspace = root.path().join(name);
        let result = checkout
            .checkout(&manifest, &credentials, &workspace, &cancel)
            .await;
        if name == "trusted" {
            result.unwrap_or_else(|error| {
                panic!(
                    "SSH checkout failed: {error:#}; daemon: {}",
                    fs::read_to_string(&diagnostics).unwrap()
                )
            });
            assert_eq!(git(&workspace, &["rev-parse", "HEAD"]), fixed);
            assert_eq!(
                fs::read_to_string(workspace.join("main.c")).unwrap(),
                "pinned source\n"
            );
        } else {
            assert!(result.is_err(), "{name} bypassed SSH trust");
        }
        assert_eq!(
            fs::read_dir(&auth).unwrap().count(),
            0,
            "SSH authentication files remain after {name}"
        );
    }
    if let Some(container) = &container {
        command("docker", &["rm", "-f", &container.0]);
    }
    let _ = daemon.start_kill();
    daemon.wait().await.unwrap();
}

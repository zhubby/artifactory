//! [INPUT]: 依赖原生任务目录隔离和遗留目录清理。
//! [OUTPUT]: 验证任务缓存、系统路径、活动目录保护与 Windows 缓存链接清理。
//! [POS]: 原生源码生命周期回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_emissary::{config::Config, source};
#[test]
fn task_environment_contains_all_writable_caches() {
    let temp = tempfile::tempdir().unwrap();
    let mut env = source::build_environment();
    source::isolate_environment(temp.path(), &mut env).unwrap();
    for key in [
        "HOME",
        "TMPDIR",
        "CARGO_HOME",
        "GOCACHE",
        "GOMODCACHE",
        "XDG_CACHE_HOME",
        "npm_config_cache",
        "GRADLE_USER_HOME",
        "NUGET_PACKAGES",
    ] {
        let path = std::path::Path::new(env.get(std::ffi::OsStr::new(key)).unwrap());
        assert!(
            path.starts_with(temp.path()),
            "{key} escapes attempt directory"
        );
    }
    for key in [
        "ANTHROPIC_API_KEY",
        "ARTIFACTORY_MASTER_KEY",
        "ARTIFACTORY_ENROLLMENT_TOKEN",
    ] {
        assert!(!env.contains_key(std::ffi::OsStr::new(key)));
    }
}

#[cfg(windows)]
#[test]
fn isolated_windows_dotnet_finds_common_application_data() {
    let root = tempfile::tempdir().unwrap();
    let mut environment = source::build_environment();
    source::isolate_environment(root.path(), &mut environment).unwrap();
    let output = std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$path = [Environment]::GetFolderPath([Environment+SpecialFolder]::CommonApplicationData); if ([string]::IsNullOrEmpty($path) -or -not [IO.Path]::IsPathRooted($path)) { exit 1 }; [Console]::WriteLine($path)",
        ])
        .env_clear()
        .envs(&environment)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "isolated .NET CommonApplicationData failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn java_and_maven_keep_jvm_state_in_attempt() {
    use std::{ffi::OsStr, process::Command};
    if !Command::new("java")
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        eprintln!("JVM source isolation test requires an installed JDK");
        return;
    }
    let root = tempfile::Builder::new()
        .prefix("source isolation ")
        .tempdir()
        .unwrap();
    let mut environment = source::build_environment();
    for name in [
        "JAVA_TOOL_OPTIONS",
        "JDK_JAVA_OPTIONS",
        "_JAVA_OPTIONS",
        "MAVEN_OPTS",
    ] {
        environment.insert(
            name.into(),
            "-Duser.home=/outside-task -Dmaven.repo.local=/outside-task".into(),
        );
    }
    let original_java_home = environment.get(OsStr::new("JAVA_HOME")).cloned();
    source::isolate_environment(root.path(), &mut environment).unwrap();
    assert_eq!(
        environment.get(OsStr::new("JAVA_HOME")),
        original_java_home.as_ref()
    );
    let java = Command::new("java")
        .args(["-XshowSettings:properties", "-version"])
        .env_clear()
        .envs(&environment)
        .output()
        .unwrap();
    let settings = String::from_utf8_lossy(&java.stderr);
    assert!(java.status.success(), "{settings}");
    for (name, expected) in [
        ("user.home", "home"),
        ("java.io.tmpdir", "tmp"),
        ("maven.repo.local", "cache/maven/repository"),
    ] {
        let actual = settings
            .lines()
            .find_map(|line| line.trim().strip_prefix(&format!("{name} = ")))
            .unwrap_or_else(|| panic!("missing JVM property {name}: {settings}"));
        assert_eq!(
            std::path::Path::new(actual).canonicalize().unwrap(),
            root.path().join(expected).canonicalize().unwrap()
        );
    }
    let maven = if cfg!(windows) { "mvn.cmd" } else { "mvn" };
    if Command::new(maven).arg("-version").output().is_err() {
        eprintln!("Maven source isolation test requires an installed Maven");
        return;
    }
    let pom = root.path().join("pom.xml");
    std::fs::write(&pom,"<project><modelVersion>4.0.0</modelVersion><groupId>test</groupId><artifactId>ephemeral-source</artifactId><version>1</version><packaging>pom</packaging></project>").unwrap();
    let output = Command::new(maven)
        .args(["-o", "-X", "-Dstyle.color=never", "-f"])
        .arg(&pom)
        .arg("validate")
        .env_clear()
        .envs(&environment)
        .output()
        .unwrap();
    let logs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "{logs}");
    let repository = logs
        .lines()
        .find_map(|line| {
            line.split_once("Using local repository at ")
                .map(|(_, path)| path.trim())
        })
        .unwrap_or_else(|| panic!("Maven did not report its local repository: {logs}"));
    assert_eq!(
        std::path::Path::new(repository).canonicalize().unwrap(),
        root.path()
            .join("cache/maven/repository")
            .canonicalize()
            .unwrap()
    );
}
#[tokio::test]
async fn startup_sweep_refuses_active_attempt_and_deletes_abandoned_source() {
    let temp = tempfile::tempdir().unwrap();
    let cfg = Config {
        root: temp.path().to_owned(),
        ..Default::default()
    };
    let id = uuid::Uuid::new_v4().to_string();
    let work = temp.path().join("work").join(&id);
    std::fs::create_dir_all(&work).unwrap();
    std::fs::write(work.join("source.txt"), "private source").unwrap();
    let lock = source::attempt_lock(&cfg, &id).unwrap();
    assert!(source::sweep(&cfg).await.is_err());
    assert!(work.exists());
    fs2::FileExt::unlock(&lock).unwrap();
    drop(lock);
    source::sweep(&cfg).await.unwrap();
    assert!(!work.exists());
}

#[cfg(windows)]
#[tokio::test]
async fn startup_sweep_removes_denied_cache_junction_without_touching_target() {
    use std::{fs, path::PathBuf, process::Command};

    // 清理失败也先删除链接，避免测试夹具留下受限目录。
    struct Junction(PathBuf);
    impl Drop for Junction {
        fn drop(&mut self) {
            let _ = fs::remove_dir(&self.0);
        }
    }

    let temp = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let cfg = Config {
        root: temp.path().join("data"),
        ..Default::default()
    };
    let id = uuid::Uuid::new_v4().to_string();
    let workspace = cfg.root.join("work").join(&id);
    let cache = workspace.join(r"home\AppData\Local\Microsoft\Windows\INetCache");
    fs::create_dir_all(&cache).unwrap();
    let readonly = workspace.join("readonly.idx");
    fs::write(&readonly, "git index").unwrap();
    let mut permissions = fs::metadata(&readonly).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&readonly, permissions).unwrap();
    fs::write(outside.path().join("keep.txt"), "outside workspace").unwrap();
    let junction = Junction(cache.join("Content.IE5"));
    let output = Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(&junction.0)
        .arg(outside.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let output = Command::new("icacls.exe")
        .arg(&junction.0)
        .args(["/deny", "*S-1-1-0:(S,RD)", "/L"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    fs::write(cfg.root.join("source-cleanup-blocked"), "previous failure").unwrap();

    let result = source::sweep(&cfg).await;
    assert!(result.is_ok(), "受限缓存链接阻止启动清理: {result:?}");
    assert!(!workspace.exists());
    assert!(!cfg.root.join("source-cleanup-blocked").exists());
    assert_eq!(
        fs::read_to_string(outside.path().join("keep.txt")).unwrap(),
        "outside workspace",
        "清理不得进入链接目标"
    );
}

#[cfg(windows)]
#[tokio::test]
async fn startup_sweep_keeps_real_access_denial_blocked_until_recovery() {
    use std::{fs, path::PathBuf, process::Command};

    struct DeniedDirectory(PathBuf);
    impl Drop for DeniedDirectory {
        fn drop(&mut self) {
            let _ = Command::new("icacls.exe")
                .arg(&self.0)
                .args(["/remove:d", "*S-1-1-0"])
                .output();
        }
    }

    let temp = tempfile::tempdir().unwrap();
    let cfg = Config {
        root: temp.path().join("data"),
        ..Default::default()
    };
    let workspace = cfg.root.join("work").join(uuid::Uuid::new_v4().to_string());
    let denied = DeniedDirectory(workspace.join("private-directory"));
    fs::create_dir_all(&denied.0).unwrap();
    fs::write(denied.0.join("source.txt"), "retained source").unwrap();
    let output = Command::new("icacls.exe")
        .arg(&denied.0)
        .args(["/deny", "*S-1-1-0:(RD)"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let acl_before = Command::new("icacls.exe").arg(&denied.0).output().unwrap();
    assert!(acl_before.status.success(), "{acl_before:?}");

    assert!(source::sweep(&cfg).await.is_err());
    assert!(cfg.root.join("source-cleanup-blocked").exists());
    assert!(workspace.exists());
    let acl_after = Command::new("icacls.exe").arg(&denied.0).output().unwrap();
    assert!(acl_after.status.success(), "{acl_after:?}");
    assert_eq!(
        acl_before.stdout, acl_after.stdout,
        "清理不得放宽真实目录的访问权限"
    );
    drop(denied);
    source::sweep(&cfg).await.unwrap();
    assert!(!workspace.exists());
    assert!(!cfg.root.join("source-cleanup-blocked").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn startup_sweep_kills_orphaned_worker_sessions_before_deleting_source() {
    use artifactory_emissary::process;
    use std::{process::Stdio, time::Duration};
    use tokio::process::Command;
    if let Some(marker) = std::env::var_os("ARTIFACTORY_SESSION_TEST_MARKER") {
        let mut command = Command::new("sh");
        command
            .args(["-c", "sleep 120"])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = process::spawn(command).unwrap();
        std::fs::write(marker, child.id().unwrap().to_string()).unwrap();
        let _ = child.wait().await;
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let cfg = Config {
        root: root.path().to_owned(),
        ..Default::default()
    };
    let id = uuid::Uuid::new_v4().to_string();
    let workspace = root.path().join("work").join(&id);
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::write(workspace.join("source.txt"), "task source").unwrap();
    let marker = root.path().join("descendant.pid");
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "startup_sweep_kills_orphaned_worker_sessions_before_deleting_source",
            "--nocapture",
        ])
        .env("ARTIFACTORY_SESSION_TEST_MARKER", &marker)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut worker = process::spawn_worker(command).unwrap();
    let pid = worker.id().unwrap();
    source::register_session(&cfg, &id, pid).unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        while !marker.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let descendant = std::fs::read_to_string(&marker)
        .unwrap()
        .parse::<u32>()
        .unwrap();
    assert!(
        std::process::Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .status()
            .unwrap()
            .success()
    );
    worker.wait().await.unwrap();
    source::sweep(&cfg).await.unwrap();
    assert!(!workspace.exists());
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    assert!(
        system
            .process(sysinfo::Pid::from_u32(descendant))
            .is_none_or(|p| p.status() == sysinfo::ProcessStatus::Zombie),
        "orphaned build process survives source cleanup"
    );
}

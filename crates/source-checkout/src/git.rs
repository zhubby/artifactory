//! [INPUT]: 依赖短期凭据、系统路径、标准代理环境和私有临时目录。
//! [OUTPUT]: 隔离环境与进程；有界读取 stderr，仅返回安全分类。
//! [POS]: 检出引擎的唯一 Git 进程入口。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::diagnostics::{GitFailure, GitFailureKind, classify_stderr};
use crate::{GitCredential, SourceCredential, same_origin, validate_repository};
use anyhow::{Context, Result, bail, ensure};
use process_wrap::tokio::{CommandWrap, KillOnDrop};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{ExitStatus, Stdio},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};
use tokio_util::sync::CancellationToken;

pub const NULL: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };
const OUTPUT_LIMIT: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct Options {
    pub git: String,
    pub ca_file: PathBuf,
    pub auth_root: PathBuf,
    pub timeout: Duration,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            git: "git".into(),
            ca_file: PathBuf::new(),
            auth_root: std::env::temp_dir(),
            timeout: Duration::from_secs(120),
        }
    }
}

pub fn build_environment() -> BTreeMap<OsString, OsString> {
    [
        "PATH",
        "HOME",
        "USER",
        "LOGNAME",
        "SHELL",
        "LANG",
        "LC_ALL",
        "TZ",
        "TMPDIR",
        "TMP",
        "TEMP",
        "SystemRoot",
        "SYSTEMROOT",
        "SystemDrive",
        "WINDIR",
        "COMSPEC",
        "PATHEXT",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "JAVA_HOME",
        "ANDROID_HOME",
        "ANDROID_SDK_ROOT",
        "DEVELOPER_DIR",
        "SDKROOT",
        "CARGO_HOME",
        "RUSTUP_HOME",
        "GOPATH",
        "GOCACHE",
        "CC",
        "CXX",
        "PKG_CONFIG_PATH",
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
        "NO_PROXY",
        "no_proxy",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
        "NODE_EXTRA_CA_CERTS",
    ]
    .into_iter()
    .filter_map(|key| std::env::var_os(key).map(|value| (key.into(), value)))
    .collect()
}
pub fn private_dir(path: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    Ok(())
}
fn private_write(path: &Path, data: &[u8]) -> Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(data)?;
    Ok(())
}
pub fn askpass() -> bool {
    if std::env::var("ARTIFACTORY_GIT_ASKPASS").as_deref() != Ok("1") {
        return false;
    }
    let prompt = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    let expected = std::env::var("ARTIFACTORY_GIT_AUTH_ORIGIN").ok();
    let actual = prompt
        .split('\'')
        .nth(1)
        .and_then(|s| url::Url::parse(s).ok())
        .map(|u| u.origin().ascii_serialization());
    if expected.is_none() || expected != actual {
        println!();
        return true;
    }
    println!(
        "{}",
        std::env::var(if prompt.to_ascii_lowercase().contains("username") {
            "ARTIFACTORY_GIT_USERNAME"
        } else {
            "ARTIFACTORY_GIT_PASSWORD"
        })
        .unwrap_or_default()
    );
    true
}
fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub(crate) struct Git {
    directory: tempfile::TempDir,
    options: Options,
    environment: BTreeMap<OsString, OsString>,
}
impl Git {
    pub(crate) fn isolate_index(&mut self) {
        self.environment.insert(
            "GIT_INDEX_FILE".into(),
            self.directory.path().join("index").into_os_string(),
        );
        let config = [
            ("core.sparseCheckout", OsString::from("false")),
            ("index.sparse", OsString::from("false")),
            ("core.splitIndex", OsString::from("false")),
            ("core.fsmonitor", OsString::from("false")),
            (
                "lfs.storage",
                self.directory.path().join("lfs").into_os_string(),
            ),
        ];
        self.environment
            .insert("GIT_CONFIG_COUNT".into(), config.len().to_string().into());
        for (index, (name, value)) in config.into_iter().enumerate() {
            self.environment
                .insert(format!("GIT_CONFIG_KEY_{index}").into(), name.into());
            self.environment
                .insert(format!("GIT_CONFIG_VALUE_{index}").into(), value);
        }
    }
    pub(crate) fn new(
        options: &Options,
        repository: &str,
        credentials: &[SourceCredential],
        inherited: Option<(&str, &GitCredential)>,
    ) -> Result<Self> {
        validate_repository(repository)?;
        private_dir(&options.auth_root)?;
        let directory = tempfile::Builder::new()
            .prefix("git-auth-")
            .tempdir_in(&options.auth_root)?;
        let mut environment = build_environment();
        for key in [
            "HOME",
            "USERPROFILE",
            "XDG_CONFIG_HOME",
            "TMPDIR",
            "TMP",
            "TEMP",
            "APPDATA",
            "LOCALAPPDATA",
        ] {
            environment.insert(key.into(), directory.path().as_os_str().to_owned());
        }
        for (key, value) in [
            ("GIT_TERMINAL_PROMPT", "0"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_CONFIG_GLOBAL", NULL),
            ("GIT_LFS_SKIP_SMUDGE", "1"),
            ("GIT_ATTR_NOSYSTEM", "1"),
        ] {
            environment.insert(key.into(), value.into());
        }
        environment.insert("GIT_SSH_COMMAND".into(),format!("ssh -F {} -o BatchMode=yes -o IdentitiesOnly=yes -o IdentityFile=none -o IdentityAgent=none -o StrictHostKeyChecking=yes -o UserKnownHostsFile={} -o GlobalKnownHostsFile={}",quote(NULL),quote(NULL),quote(NULL)).into());
        if !options.ca_file.as_os_str().is_empty() {
            environment.insert(
                "GIT_SSL_CAINFO".into(),
                options.ca_file.as_os_str().to_owned(),
            );
        }
        let credential = credentials
            .iter()
            .find(|c| c.upstream_url == repository)
            .map(|c| &c.credential)
            .or_else(|| {
                inherited
                    .filter(|(parent, _)| same_origin(parent, repository))
                    .map(|(_, c)| c)
            });
        if let Some(credential) = credential {
            match credential.kind.as_str() {
                "https" => {
                    ensure!(
                        repository.starts_with("http://") || repository.starts_with("https://"),
                        "HTTPS credential does not match repository transport"
                    );
                    environment.insert(
                        "ARTIFACTORY_GIT_AUTH_ORIGIN".into(),
                        url::Url::parse(repository)?
                            .origin()
                            .ascii_serialization()
                            .into(),
                    );
                    environment.insert(
                        "GIT_ASKPASS".into(),
                        std::env::current_exe()?.into_os_string(),
                    );
                    environment.insert("ARTIFACTORY_GIT_ASKPASS".into(), "1".into());
                    environment.insert(
                        "ARTIFACTORY_GIT_USERNAME".into(),
                        credential.username.clone().into(),
                    );
                    environment.insert(
                        "ARTIFACTORY_GIT_PASSWORD".into(),
                        credential.secret.clone().into(),
                    );
                }
                "ssh" => {
                    ensure!(
                        !repository.starts_with("http://") && !repository.starts_with("https://"),
                        "SSH credential does not match repository transport"
                    );
                    ensure!(
                        !credential.known_hosts.trim().is_empty(),
                        "SSH host trust is required"
                    );
                    let key = directory.path().join("identity");
                    let hosts = directory.path().join("known_hosts");
                    private_write(&key, credential.secret.as_bytes())?;
                    private_write(&hosts, credential.known_hosts.as_bytes())?;
                    let trusted = format!(
                        "UserKnownHostsFile=\"{}\"",
                        hosts
                            .to_string_lossy()
                            .replace('\\', "/")
                            .replace('"', "\\\"")
                            .replace('%', "%%")
                    );
                    environment.insert("GIT_SSH_COMMAND".into(),format!("ssh -F {} -i {} -o BatchMode=yes -o IdentitiesOnly=yes -o IdentityAgent=none -o StrictHostKeyChecking=yes -o GlobalKnownHostsFile={} -o {}",quote(NULL),quote(&key.to_string_lossy()),quote(NULL),quote(&trusted)).into());
                }
                _ => bail!("unsupported Git credential"),
            }
        }
        Ok(Self {
            directory,
            options: options.clone(),
            environment,
        })
    }
    pub(crate) async fn output(
        &self,
        directory: &Path,
        args: &[&str],
        cancel: &CancellationToken,
    ) -> Result<(ExitStatus, Vec<u8>)> {
        self.output_with_input(directory, args, None, cancel).await
    }
    pub(crate) async fn output_with_input(
        &self,
        directory: &Path,
        args: &[&str],
        input: Option<&[u8]>,
        cancel: &CancellationToken,
    ) -> Result<(ExitStatus, Vec<u8>)> {
        let (status, bytes, _) = self
            .diagnosed_output(directory, args, input, cancel)
            .await?;
        Ok((status, bytes))
    }
    async fn diagnosed_output(
        &self,
        directory: &Path,
        args: &[&str],
        input: Option<&[u8]>,
        cancel: &CancellationToken,
    ) -> Result<(ExitStatus, Vec<u8>, GitFailureKind)> {
        let mut command = Command::new(&self.options.git);
        command
            .current_dir(directory)
            .env_clear()
            .envs(&self.environment)
            .arg("-c")
            .arg(format!("core.hooksPath={NULL}"))
            .args([
                "-c",
                "credential.helper=",
                "-c",
                "protocol.file.allow=never",
                "-c",
                "protocol.ext.allow=never",
                "-c",
                "http.followRedirects=false",
            ])
            .args(args)
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut wrap = CommandWrap::from(command);
        wrap.wrap(KillOnDrop);
        #[cfg(unix)]
        wrap.wrap(process_wrap::tokio::ProcessGroup::leader());
        #[cfg(windows)]
        wrap.wrap(process_wrap::tokio::JobObject);
        let mut child = wrap.spawn().context("start Git process")?;
        let stdout = child.stdout().take().context("Git stdout missing")?;
        let mut stderr = child.stderr().take().context("Git stderr missing")?;
        let stdin = child.stdin().take();
        let read = async {
            let mut bytes = Vec::new();
            stdout
                .take(OUTPUT_LIMIT + 1)
                .read_to_end(&mut bytes)
                .await?;
            ensure!(
                bytes.len() as u64 <= OUTPUT_LIMIT,
                "Git output exceeds limit"
            );
            Ok::<_, anyhow::Error>(bytes)
        };
        let write = async {
            if let (Some(mut stdin), Some(input)) = (stdin, input) {
                stdin.write_all(input).await?;
                stdin.shutdown().await?;
            }
            Ok::<_, anyhow::Error>(())
        };
        let diagnose = async {
            let mut retained = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let count = stderr.read(&mut chunk).await?;
                if count == 0 {
                    break;
                }
                let keep = count.min((64 * 1024usize).saturating_sub(retained.len()));
                retained.extend_from_slice(&chunk[..keep]);
            }
            Ok::<_, anyhow::Error>(classify_stderr(&retained))
        };
        let operation = async {
            let (status, bytes, (), diagnostic) = tokio::try_join!(
                async { Ok::<_, anyhow::Error>(child.wait().await?) },
                read,
                write,
                diagnose
            )?;
            Ok::<_, anyhow::Error>((status, bytes, diagnostic))
        };
        let result = tokio::select! {
            result = operation => result,
            _ = cancel.cancelled() => Err(GitFailure(GitFailureKind::Canceled).into()),
            _ = tokio::time::sleep(self.options.timeout) => Err(GitFailure(GitFailureKind::Timeout).into()),
        };
        if result.is_err() {
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
        result
    }
    pub(crate) async fn run(
        &self,
        directory: &Path,
        args: &[&str],
        cancel: &CancellationToken,
    ) -> Result<Vec<u8>> {
        let (status, bytes, diagnostic) =
            self.diagnosed_output(directory, args, None, cancel).await?;
        if !status.success() {
            return Err(GitFailure(diagnostic).into());
        }
        Ok(bytes)
    }
    pub(crate) fn directory(&self) -> &Path {
        self.directory.path()
    }
    pub(crate) fn close(self) -> Result<()> {
        self.directory
            .close()
            .context("remove Git authentication directory")
    }
}

#[cfg(test)]
mod tests {
    use super::build_environment;
    use std::{ffi::OsStr, process::Command};

    #[test]
    fn build_environment_keeps_system_drive_and_excludes_secrets() {
        const MARKER: &str = "ARTIFACTORY_ENVIRONMENT_TEST_CHILD";
        if std::env::var_os(MARKER).is_some() {
            let environment = build_environment();
            assert_eq!(
                environment.get(OsStr::new("SystemDrive")),
                Some(&"C:".into())
            );
            assert!(!environment.contains_key(OsStr::new("ARTIFACTORY_MASTER_KEY")));
            return;
        }
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "git::tests::build_environment_keeps_system_drive_and_excludes_secrets",
                "--nocapture",
            ])
            .env(MARKER, "1")
            .env("SystemDrive", "C:")
            .env("ARTIFACTORY_MASTER_KEY", "must-not-reach-build")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

//! [INPUT]: 依赖设备本地配置、系统账户查询与公开终端能力契约。
//! [OUTPUT]: 提供默认关闭的终端设置和固定 Shell profile 校验。
//! [POS]: 设备本地准入边界；浏览器不能选择任意可执行路径。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use anyhow::{Context, Result, ensure};
use artifactory_emissary_protocol::terminal::{TerminalCapabilities, TerminalProfile};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf, time::Duration};

/// 只由设备本地配置指定的 Shell 和参数。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShellProfile {
    pub label: String,
    pub shell: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
}

/// 设备必须显式开启；不从远端命令接受可执行路径或环境。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TerminalSettings {
    pub enabled: bool,
    pub profiles: BTreeMap<String, ShellProfile>,
    pub cwd: Option<PathBuf>,
    pub max_sessions: usize,
}

impl Default for TerminalSettings {
    fn default() -> Self {
        #[cfg(unix)]
        let profile = ShellProfile {
            label: "Shell".into(),
            shell: "/bin/sh".into(),
            args: vec!["-i".into()],
        };
        #[cfg(windows)]
        let profile = ShellProfile {
            label: "PowerShell".into(),
            shell: std::env::var_os("SystemRoot")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
                .join(r"System32\WindowsPowerShell\v1.0\powershell.exe"),
            args: vec!["-NoLogo".into(), "-NoProfile".into()],
        };
        Self {
            enabled: false,
            profiles: BTreeMap::from([("default".into(), profile)]),
            cwd: None,
            max_sessions: 2,
        }
    }
}

impl TerminalSettings {
    /// 在设备接收远程终端前验证其本地配置。
    ///
    /// # Errors
    /// 拒绝非法 profile、非绝对或缺失的 Shell、超额参数及目录。
    pub fn validate(&self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        ensure!(
            (1..=2).contains(&self.max_sessions),
            "terminal max_sessions must be 1 or 2"
        );
        ensure!(
            !self.profiles.is_empty() && self.profiles.len() <= 8,
            "terminal requires 1 to 8 profiles"
        );
        for (id, profile) in &self.profiles {
            ensure!(
                !id.is_empty()
                    && id.len() <= 64
                    && id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b)),
                "invalid terminal profile identifier"
            );
            ensure!(
                !profile.label.is_empty()
                    && profile.label.len() <= 128
                    && !profile.label.chars().any(char::is_control),
                "invalid terminal profile label"
            );
            ensure!(
                profile.shell.is_absolute() && profile.shell.is_file(),
                "terminal profile requires an existing absolute executable"
            );
            ensure!(
                profile.args.len() <= 16
                    && profile
                        .args
                        .iter()
                        .all(|a| a.len() <= 1024 && !a.contains('\0')),
                "invalid terminal arguments"
            );
        }
        if let Some(cwd) = &self.cwd {
            ensure!(
                cwd.is_absolute() && cwd.is_dir(),
                "terminal cwd must be an existing absolute directory"
            );
        }
        Ok(())
    }
}

pub(crate) async fn capabilities(settings: &TerminalSettings) -> Result<TerminalCapabilities> {
    settings.validate()?;
    if !settings.enabled {
        return Ok(TerminalCapabilities::default());
    }
    #[cfg(unix)]
    let mut command = tokio::process::Command::new("/usr/bin/id");
    #[cfg(unix)]
    command.arg("-un");
    #[cfg(windows)]
    let mut command = tokio::process::Command::new(
        std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
            .join(r"System32\whoami.exe"),
    );
    let (status, bytes) = crate::process::probe(command, Duration::from_secs(3)).await?;
    ensure!(status.success(), "cannot identify terminal service account");
    let account = String::from_utf8(bytes)
        .context("invalid service account encoding")?
        .trim()
        .to_owned();
    ensure!(
        !account.is_empty() && account.len() <= 256 && !account.chars().any(char::is_control),
        "invalid terminal service account"
    );
    Ok(TerminalCapabilities {
        enabled: true,
        profiles: settings
            .profiles
            .iter()
            .map(|(id, profile)| TerminalProfile {
                id: id.clone(),
                label: profile.label.clone(),
            })
            .collect(),
        account,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_requires_explicit_device_enable() {
        let settings: TerminalSettings = serde_json::from_str("{}").unwrap();
        assert!(!settings.enabled);
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn enabled_terminal_rejects_relative_profile_and_unknown_configuration() {
        let mut settings = TerminalSettings {
            enabled: true,
            ..Default::default()
        };
        settings.profiles.get_mut("default").unwrap().shell = "sh".into();
        assert!(settings.validate().is_err());
        assert!(
            serde_json::from_str::<TerminalSettings>(
                r#"{"enabled":true,"arbitrary_command":"sh"}"#
            )
            .is_err()
        );
    }
}

//! [INPUT]: 仓库平台安装器、签名候选和当前隔离服务路径。
//! [OUTPUT]: 固定安装器副本，构造不含注册参数的维护调用。
//! [POS]: 原生维护成功路径的入口适配；Unix提权由安装器执行。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use sha2::Digest;
use std::{ffi::OsString, fs, path::Path};

const UNIX: &[u8] = include_bytes!("../../../control-plane/src/installation/installer.sh");
const WINDOWS: &[u8] = include_bytes!("../../../control-plane/src/installation/installer.ps1");

pub(super) struct Invocation {
    pub program: &'static str,
    pub args: Vec<OsString>,
    pub admin: bool,
}

pub(super) struct Files<'a> {
    pub installed: &'a Path,
    pub candidate: &'a Path,
    pub manifest: &'a Path,
    pub signature: &'a Path,
    pub config: &'a Path,
}

pub(super) fn stage(windows: bool, directory: &Path, files: Files<'_>) -> Invocation {
    let path = directory.join(if windows {
        "maintenance-installer.ps1"
    } else {
        "maintenance-installer.sh"
    });
    let source = if windows { WINDOWS } else { UNIX };
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    use std::io::Write;
    let mut file = options.open(&path).unwrap();
    file.write_all(source).unwrap();
    file.sync_all().unwrap();
    let digest = hex::encode(sha2::Sha256::digest(fs::read(files.candidate).unwrap()));
    let mut args: Vec<OsString> = if windows {
        ["-NoProfile", "-NonInteractive", "-File"]
            .map(OsString::from)
            .into()
    } else {
        vec![]
    };
    args.push(path.into_os_string());
    args.push(
        if windows {
            "-MaintainLauncher"
        } else {
            "--maintain-launcher"
        }
        .into(),
    );
    for (unix, windows_name, value) in [
        (
            "--installed-binary",
            "-InstalledBinary",
            files.installed.as_os_str(),
        ),
        ("--candidate", "-Candidate", files.candidate.as_os_str()),
        (
            "--candidate-sha256",
            "-CandidateSha256",
            std::ffi::OsStr::new(&digest),
        ),
        ("--manifest", "-Manifest", files.manifest.as_os_str()),
        ("--signature", "-Signature", files.signature.as_os_str()),
        ("--config", "-Config", files.config.as_os_str()),
    ] {
        args.push(if windows { windows_name } else { unix }.into());
        args.push(value.to_owned());
    }
    Invocation {
        program: if windows { "pwsh" } else { "sh" },
        args,
        admin: windows,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeMap, ffi::OsStr, path::PathBuf};

    fn check(windows: bool) {
        let directory = tempfile::Builder::new()
            .prefix("installer %n $HOME ")
            .tempdir()
            .unwrap();
        let installed = directory.path().join("installed binary");
        let candidate = directory.path().join("signed candidate");
        let manifest = directory.path().join("manifest.json");
        let signature = directory.path().join("manifest.sig");
        let config = directory.path().join("private config.json");
        fs::write(&candidate, b"signed candidate bytes").unwrap();
        let invocation = stage(
            windows,
            directory.path(),
            Files {
                installed: &installed,
                candidate: &candidate,
                manifest: &manifest,
                signature: &signature,
                config: &config,
            },
        );
        assert_eq!(
            invocation.admin, windows,
            "Unix installer must perform its own sudo"
        );
        assert_eq!(invocation.program, if windows { "pwsh" } else { "sh" });
        let prefix = if windows { 3 } else { 0 };
        if windows {
            assert_eq!(
                invocation.args[..prefix],
                ["-NoProfile", "-NonInteractive", "-File"]
            );
        }
        let staged = PathBuf::from(&invocation.args[prefix]);
        assert_eq!(
            fs::read(&staged).unwrap(),
            if windows { WINDOWS } else { UNIX }
        );
        assert_eq!(
            invocation.args[prefix + 1],
            if windows {
                "-MaintainLauncher"
            } else {
                "--maintain-launcher"
            }
        );
        let parameters: BTreeMap<_, _> = invocation.args[prefix + 2..]
            .chunks_exact(2)
            .map(|pair| (pair[0].to_string_lossy().into_owned(), pair[1].as_os_str()))
            .collect();
        let source = std::str::from_utf8(if windows { WINDOWS } else { UNIX }).unwrap();
        for (name, value) in &parameters {
            let declared = if windows {
                format!("${}", name.trim_start_matches('-'))
            } else {
                format!("{name})")
            };
            assert!(
                source.contains(&declared),
                "argument not declared by real installer: {name}"
            );
            assert!(!value.is_empty());
        }
        let names = if windows {
            [
                "-InstalledBinary",
                "-Candidate",
                "-Manifest",
                "-Signature",
                "-Config",
                "-CandidateSha256",
            ]
        } else {
            [
                "--installed-binary",
                "--candidate",
                "--manifest",
                "--signature",
                "--config",
                "--candidate-sha256",
            ]
        };
        for (name, path) in names[..5]
            .iter()
            .zip([&installed, &candidate, &manifest, &signature, &config])
        {
            assert_eq!(
                parameters[*name],
                path.as_os_str(),
                "special path must stay one literal argument"
            );
        }
        assert_eq!(
            parameters[names[5]],
            OsStr::new("487b00f0556de9d5d75f5dbeffdd74457f2a4d281f82a96fbf3a982b850044b1")
        );
        assert_eq!(
            parameters.len(),
            6,
            "maintenance must not require enrollment or service account arguments"
        );
    }

    #[test]
    fn unix_staging_matches_real_installer_interface() {
        check(false);
    }

    #[test]
    fn windows_staging_matches_real_installer_interface() {
        check(true);
    }
}

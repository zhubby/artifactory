//! [INPUT]: 依赖最终制品副本、固定证书元数据和原生验签工具。
//! [OUTPUT]: 提供绑定 SHA-256 的证据并向控制平面登记。
//! [POS]: 上传前校验应用、DMG 装订票据和摘要，并回收挂载。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::{Assignment, native};
use crate::{
    artifacts, client,
    config::{Config, private_dir},
    process, protocol as pb,
    worker::Input,
};
use anyhow::{Context, Result, ensure};
use pb::signing::{Binding, Evidence, Output};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    io::{Read, Seek},
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

/// Upload uses these same copies. Subsequent source mutations cannot change evidence.
pub async fn verify_outputs(
    input: &Input,
    assignment: &Assignment,
    source: &Path,
    cancel: &CancellationToken,
) -> Result<tempfile::TempDir> {
    let directory = tempfile::Builder::new()
        .prefix("verified-")
        .tempdir_in(&input.config.root)?;
    let binding = &assignment.bindings[0];
    let mut control = client::control(&input.config).await?;
    for output in &assignment.outputs {
        ensure!(
            artifacts::safe_relative(&output.path),
            "unsafe signed artifact path"
        );
        let path = directory.path().join(&output.path);
        private_dir(path.parent().context("missing artifact parent")?)?;
        let mut original = artifacts::open(source, &output.path)?;
        let mut copy = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        std::io::copy(&mut original, &mut copy)?;
        copy.sync_all()?;
        drop(copy);
        if binding.platform == "android" {
            android(&input.config, &path, binding, cancel).await?;
        } else {
            macos(&input.config, &path, output, binding, cancel).await?;
        }
        let mut file = artifacts::open(directory.path(), &output.path)?;
        let mut hash = Sha256::new();
        let mut buffer = [0; 65536];
        loop {
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
        let evidence = Evidence {
            path: output.path.clone(),
            sha256: hex::encode(hash.finalize()),
            profile: binding.profile.clone(),
            version_id: binding.version_id.clone(),
            fingerprint_sha256: binding.metadata.fingerprint_sha256.clone(),
            notarized: binding.notarize,
        };
        let mut request = pb::SigningEvidenceRequest {
            attempt_id: input.attempt_id.clone(),
            lease_token: input.lease_token.clone(),
            session_id: assignment.session_id(),
            evidence_json: serde_json::to_string(&evidence)?,
        };
        let mut retries = 0;
        let reply = loop {
            request.session_id = assignment.session_id();
            let reply = tokio::select! {_=cancel.cancelled()=>anyhow::bail!("signing verification canceled"),reply=tokio::time::timeout(Duration::from_secs(10),control.record_signing_evidence(request.clone()))=>reply.context("signing evidence registration timed out")?};
            match reply {
                Ok(reply) => break reply,
                Err(error)
                    if error.code() == tonic::Code::FailedPrecondition
                        && error.message()
                            == "all build steps must finish before final signing verification"
                        && retries < 100 =>
                {
                    retries += 1;
                    tokio::select! {_=cancel.cancelled()=>anyhow::bail!("signing verification canceled"),_=tokio::time::sleep(Duration::from_millis(100))=>{}}
                }
                Err(_) => anyhow::bail!("signing evidence registration rejected"),
            }
        };
        ensure!(reply.into_inner().accepted, "signing evidence rejected");
    }
    Ok(directory)
}
async fn android(
    cfg: &Config,
    path: &Path,
    binding: &Binding,
    cancel: &CancellationToken,
) -> Result<()> {
    let file = path.to_str().context("invalid Android artifact path")?;
    if file.ends_with(".apk") {
        let output = native(
            cfg,
            "apksigner",
            &["verify", "--verbose", "--print-certs", file],
            cancel,
        )
        .await?;
        let output = String::from_utf8_lossy(&output);
        let fingerprints: Vec<_> = output
            .lines()
            .filter_map(|line| {
                line.split_once("certificate SHA-256 digest:")
                    .map(|(_, value)| value.trim().replace(':', "").to_lowercase())
            })
            .collect();
        ensure!(
            fingerprints.len() == 1 && fingerprints[0] == binding.metadata.fingerprint_sha256,
            "APK signing certificate mismatch"
        );
    } else {
        ensure!(file.ends_with(".aab"), "unsupported Android signing output");
        let mut command = Command::new(cfg.tool("jarsigner"));
        command
            .args(["-verify", "-strict", file])
            .env("LC_ALL", "C")
            .env("JAVA_TOOL_OPTIONS", "-Duser.language=en -Duser.country=US");
        let (status, output) = process::output(command, cancel, Duration::from_secs(120)).await?;
        // JDK exit bit 4 covers self-signed/untrusted Android certificates. All
        // other strict failures (including unsigned entries) are rejected.
        ensure!(
            matches!(status.code(), Some(0 | 4))
                && String::from_utf8_lossy(&output).contains("jar verified"),
            "AAB is unsigned or JAR verification failed"
        );
        let certificate = native(
            cfg,
            "keytool",
            &["-printcert", "-jarfile", file, "-rfc"],
            cancel,
        )
        .await?;
        let certificates = rustls_pemfile::certs(&mut certificate.as_slice())
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ensure!(
            !certificates.is_empty()
                && hex::encode(Sha256::digest(certificates[0].as_ref()))
                    == binding.metadata.fingerprint_sha256,
            "AAB signing certificate mismatch"
        );
        // More than one signer would make certificate selection ambiguous.
        let archive = zip::ZipArchive::new(fs::File::open(path)?)?;
        let signers = archive
            .file_names()
            .filter(|name| {
                name.starts_with("META-INF/")
                    && [".RSA", ".DSA", ".EC"]
                        .iter()
                        .any(|ext| name.ends_with(ext))
            })
            .count();
        ensure!(signers == 1, "AAB requires exactly one signer");
    }
    Ok(())
}
fn archive_safe<R: Read + Seek>(archive: &mut zip::ZipArchive<R>) -> Result<()> {
    let mut links = HashSet::new();
    let mut paths = Vec::new();
    let mut size = 0u64;
    ensure!(archive.len() <= 100_000, "ZIP has too many entries");
    for index in 0..archive.len() {
        let mut file = archive.by_index(index)?;
        let name = file.name().trim_end_matches('/').to_owned();
        ensure!(artifacts::safe_relative(&name), "unsafe ZIP entry");
        size = size.checked_add(file.size()).context("ZIP size overflow")?;
        ensure!(size <= 20 << 30, "expanded ZIP exceeds limit");
        if file
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            let mut target = String::new();
            file.by_ref().take(4096).read_to_string(&mut target)?;
            ensure!(
                !target.starts_with('/') && !target.contains(['\\', '\0', ':']),
                "unsafe ZIP symbolic link"
            );
            let parent = Path::new(&name).parent().context("missing link parent")?;
            let mut depth = parent.components().count() as i64;
            for part in target.split('/') {
                if part == ".." {
                    depth -= 1;
                } else if part != "." && !part.is_empty() {
                    depth += 1;
                }
                ensure!(depth >= 0, "ZIP symbolic link escapes archive");
            }
            links.insert(name.clone());
        }
        paths.push(name);
    }
    for path in paths {
        let mut parent = Path::new(&path).parent();
        while let Some(p) = parent {
            ensure!(
                !links.contains(p.to_str().unwrap_or("")),
                "ZIP entry traverses a symbolic link"
            );
            parent = p.parent();
        }
    }
    Ok(())
}
async fn check_bundle(
    cfg: &Config,
    root: &Path,
    output: &Output,
    binding: &Binding,
    cancel: &CancellationToken,
) -> Result<()> {
    let relative = output
        .bundle
        .as_deref()
        .context("missing macOS bundle path")?;
    ensure!(
        artifacts::safe_relative(relative) && relative.ends_with(".app"),
        "invalid macOS bundle path"
    );
    let bundle = root.join(relative).canonicalize()?;
    ensure!(
        bundle.starts_with(root.canonicalize()?),
        "bundle escapes archive"
    );
    let bundle = bundle.to_str().context("invalid bundle path")?;
    native(
        cfg,
        "codesign",
        &[
            "--verify",
            "--deep",
            "--strict",
            "--all-architectures",
            bundle,
        ],
        cancel,
    )
    .await?;
    let certs = tempfile::tempdir_in(&cfg.root)?;
    let prefix = certs.path().join("cert");
    let prefix = prefix.to_str().context("invalid certificate path")?;
    native(
        cfg,
        "codesign",
        &["--display", "--extract-certificates", prefix, bundle],
        cancel,
    )
    .await?;
    let mut trust_args = vec!["verify-cert".to_owned(), "-p".into(), "codesigning".into()];
    for index in 0..16 {
        let path = format!("{prefix}{index}");
        if !Path::new(&path).exists() {
            break;
        }
        trust_args.extend(["-c".into(), path]);
    }
    native(
        cfg,
        "security",
        &trust_args.iter().map(String::as_str).collect::<Vec<_>>(),
        cancel,
    )
    .await?;
    let cert = fs::read(format!("{prefix}0"))?;
    ensure!(
        hex::encode(Sha256::digest(&cert)) == binding.metadata.fingerprint_sha256,
        "macOS signing certificate mismatch"
    );
    let (_, parsed) = x509_parser::parse_x509_certificate(&cert)
        .map_err(|_| anyhow::anyhow!("invalid bundle certificate"))?;
    let team = parsed
        .subject()
        .iter_organizational_unit()
        .next()
        .and_then(|value| value.as_str().ok());
    ensure!(
        team == binding.metadata.team_id.as_deref(),
        "macOS Team ID mismatch"
    );
    if binding.notarize {
        native(cfg, "xcrun", &["stapler", "validate", bundle], cancel).await?;
    }
    Ok(())
}
async fn macos(
    cfg: &Config,
    path: &Path,
    output: &Output,
    binding: &Binding,
    cancel: &CancellationToken,
) -> Result<()> {
    ensure!(
        cfg!(target_os = "macos"),
        "macOS verification requires macOS"
    );
    if output.path.ends_with(".zip") {
        let mut archive = zip::ZipArchive::new(fs::File::open(path)?)?;
        archive_safe(&mut archive)?;
        drop(archive);
        let root = tempfile::tempdir_in(&cfg.root)?;
        native(
            cfg,
            "ditto",
            &[
                "-x",
                "-k",
                path.to_str().context("invalid ZIP path")?,
                root.path().to_str().context("invalid extraction path")?,
            ],
            cancel,
        )
        .await?;
        check_bundle(cfg, root.path(), output, binding, cancel).await
    } else {
        ensure!(output.path.ends_with(".dmg"), "unsupported macOS output");
        if binding.notarize {
            native(
                cfg,
                "xcrun",
                &[
                    "stapler",
                    "validate",
                    path.to_str().context("invalid DMG path")?,
                ],
                cancel,
            )
            .await?;
        }
        let mount = tempfile::tempdir_in(&cfg.root)?;
        let mount_path = mount.path().join("volume");
        fs::create_dir(&mount_path)?;
        let name = mount_path.to_str().context("invalid mount path")?;
        let mut guard = Mount {
            path: mount_path.clone(),
            tool: cfg.tool("hdiutil"),
            armed: true,
        };
        native(
            cfg,
            "hdiutil",
            &[
                "attach",
                "-readonly",
                "-nobrowse",
                "-noautoopen",
                "-mountpoint",
                name,
                path.to_str().context("invalid DMG path")?,
            ],
            cancel,
        )
        .await?;
        let result = check_bundle(cfg, &mount_path, output, binding, cancel).await;
        let detached = native(cfg, "hdiutil", &["detach", name], &CancellationToken::new()).await;
        guard.armed = detached.is_err();
        drop(guard);
        detached?;
        result
    }
}
struct Mount {
    path: PathBuf,
    tool: String,
    armed: bool,
}
impl Drop for Mount {
    fn drop(&mut self) {
        if self.armed && self.path.exists() {
            let _ = std::process::Command::new(&self.tool)
                .args(["detach", "-force"])
                .arg(&self.path)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "macos")]
    fn macos_tools(directory: &Path) -> Config {
        let tool = directory.join("hdiutil");
        crate::config::private_write(
            &tool,
            b"#!/bin/sh\ncd -- \"$(dirname -- \"$0\")\"\nprintf '%s\\n' \"$1\" >> mount-events\n[ \"$1\" != attach ]\n",
            true,
            true,
        ).unwrap();
        let stapler = directory.join("xcrun");
        crate::config::private_write(&stapler, b"#!/bin/sh\nexit 65\n", true, true).unwrap();
        Config {
            root: directory.into(),
            tools: [
                ("hdiutil".into(), tool.to_str().unwrap().into()),
                ("xcrun".into(), stapler.to_str().unwrap().into()),
            ]
            .into(),
            ..Default::default()
        }
    }
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn required_dmg_ticket_is_checked_before_mounting() {
        let directory = tempfile::tempdir().unwrap();
        let cfg = macos_tools(directory.path());
        let mut binding = test_binding("0".repeat(64));
        binding.platform = "macos".into();
        binding.notarize = true;
        let output = Output {
            path: "Client.dmg".into(),
            bundle: Some("Client.app".into()),
        };
        let error = macos(
            &cfg,
            &directory.path().join(&output.path),
            &output,
            &binding,
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("native signing tool failed: xcrun"),
            "{error}"
        );
        assert!(!directory.path().join("mount-events").exists());
    }
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn partial_dmg_attachment_is_detached_after_tool_failure() {
        let directory = tempfile::tempdir().unwrap();
        let cfg = macos_tools(directory.path());
        let output = Output {
            path: "Client.dmg".into(),
            bundle: Some("Client.app".into()),
        };
        let error = macos(
            &cfg,
            &directory.path().join(&output.path),
            &output,
            &test_binding("0".repeat(64)),
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("native signing tool failed: hdiutil")
        );
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if fs::read_to_string(directory.path().join("mount-events"))
                    .unwrap()
                    .contains("detach")
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("partially mounted DMG was not detached");
    }
    #[test]
    fn rejects_archive_traversal_before_extraction() {
        let mut output = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut output);
            zip.start_file("../escape", zip::write::SimpleFileOptions::default())
                .unwrap();
            std::io::Write::write_all(&mut zip, b"bad").unwrap();
            zip.finish().unwrap();
        }
        output.set_position(0);
        assert!(archive_safe(&mut zip::ZipArchive::new(output).unwrap()).is_err());
    }
    #[tokio::test]
    #[ignore = "requires native JDK keytool and jarsigner"]
    async fn native_aab_verification_rejects_unsigned_entries_and_wrong_identity() {
        let directory = tempfile::tempdir().unwrap();
        let cfg = Config {
            root: directory.path().into(),
            ..Default::default()
        };
        let cancel = CancellationToken::new();
        let store = directory.path().join("upload.p12");
        let store = store.to_str().unwrap();
        native(
            &cfg,
            "keytool",
            &[
                "-genkeypair",
                "-alias",
                "release",
                "-keyalg",
                "RSA",
                "-keysize",
                "2048",
                "-validity",
                "365",
                "-dname",
                "CN=Signing Test",
                "-storetype",
                "PKCS12",
                "-keystore",
                store,
                "-storepass",
                "signing-test",
                "-keypass",
                "signing-test",
            ],
            &cancel,
        )
        .await
        .unwrap();
        let path = directory.path().join("app.aab");
        {
            let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
            zip.start_file(
                "base/assets/test.txt",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            std::io::Write::write_all(&mut zip, b"signed bytes").unwrap();
            zip.finish().unwrap();
        }
        assert!(
            android(&cfg, &path, &test_binding("0".repeat(64)), &cancel)
                .await
                .is_err()
        );
        native(
            &cfg,
            "jarsigner",
            &[
                "-keystore",
                store,
                "-storepass",
                "signing-test",
                "-keypass",
                "signing-test",
                path.to_str().unwrap(),
                "release",
            ],
            &cancel,
        )
        .await
        .unwrap();
        let certificate = native(
            &cfg,
            "keytool",
            &[
                "-exportcert",
                "-alias",
                "release",
                "-keystore",
                store,
                "-storepass",
                "signing-test",
            ],
            &cancel,
        )
        .await
        .unwrap();
        let binding = test_binding(hex::encode(Sha256::digest(certificate)));
        android(&cfg, &path, &binding, &cancel).await.unwrap();
        assert!(
            android(&cfg, &path, &test_binding("0".repeat(64)), &cancel)
                .await
                .is_err()
        );
        {
            let mut zip = zip::ZipWriter::new_append(
                fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&path)
                    .unwrap(),
            )
            .unwrap();
            zip.start_file("unsigned.txt", zip::write::SimpleFileOptions::default())
                .unwrap();
            std::io::Write::write_all(&mut zip, b"untrusted").unwrap();
            zip.finish().unwrap();
        }
        assert!(android(&cfg, &path, &binding, &cancel).await.is_err());
    }
    fn test_binding(fingerprint: String) -> Binding {
        Binding {
            profile: "managed:test".into(),
            platform: "android".into(),
            credential_id: "test".into(),
            version_id: "test".into(),
            notarize: false,
            metadata: pb::signing::Metadata {
                subject: "test".into(),
                fingerprint_sha256: fingerprint,
                not_before: 0,
                not_after: i64::MAX,
                identity: "test".into(),
                team_id: None,
                alias: Some("release".into()),
                purpose: Some(pb::signing::AndroidPurpose::Upload),
                notarization_available: false,
            },
        }
    }

    #[tokio::test]
    #[ignore = "requires ARTIFACTORY_TEST_ANDROID_BUILD_TOOLS, ARTIFACTORY_TEST_ANDROID_PLATFORM and keytool"]
    async fn native_apk_verification_checks_a_packaged_android_application() {
        let tools = PathBuf::from(std::env::var("ARTIFACTORY_TEST_ANDROID_BUILD_TOOLS").unwrap());
        let platform = std::env::var("ARTIFACTORY_TEST_ANDROID_PLATFORM").unwrap();
        let directory = tempfile::tempdir().unwrap();
        let mut cfg = Config {
            root: directory.path().into(),
            ..Default::default()
        };
        for tool in ["apksigner", "aapt2"] {
            cfg.tools
                .insert(tool.into(), tools.join(tool).to_str().unwrap().into());
        }
        let cancel = CancellationToken::new();
        let store = directory.path().join("app.p12");
        let store = store.to_str().unwrap();
        native(
            &cfg,
            "keytool",
            &[
                "-genkeypair",
                "-alias",
                "release",
                "-keyalg",
                "RSA",
                "-keysize",
                "2048",
                "-validity",
                "365",
                "-dname",
                "CN=APK Test",
                "-storetype",
                "PKCS12",
                "-keystore",
                store,
                "-storepass",
                "signing-test",
                "-keypass",
                "signing-test",
            ],
            &cancel,
        )
        .await
        .unwrap();
        let manifest = directory.path().join("AndroidManifest.xml");
        fs::write(&manifest,r#"<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="com.example.signing"><uses-sdk android:minSdkVersion="21" android:targetSdkVersion="35"/><application android:hasCode="false" android:label="Signing test"/></manifest>"#).unwrap();
        let path = directory.path().join("app.apk");
        let name = path.to_str().unwrap();
        native(
            &cfg,
            "aapt2",
            &[
                "link",
                "--manifest",
                manifest.to_str().unwrap(),
                "-I",
                &platform,
                "-o",
                name,
            ],
            &cancel,
        )
        .await
        .unwrap();
        let mut binding = test_binding("0".repeat(64));
        binding.metadata.purpose = Some(pb::signing::AndroidPurpose::Application);
        assert!(android(&cfg, &path, &binding, &cancel).await.is_err());
        native(
            &cfg,
            "apksigner",
            &[
                "sign",
                "--ks",
                store,
                "--ks-key-alias",
                "release",
                "--ks-pass",
                "pass:signing-test",
                "--key-pass",
                "pass:signing-test",
                name,
            ],
            &cancel,
        )
        .await
        .unwrap();
        let cert = native(
            &cfg,
            "keytool",
            &[
                "-exportcert",
                "-alias",
                "release",
                "-keystore",
                store,
                "-storepass",
                "signing-test",
            ],
            &cancel,
        )
        .await
        .unwrap();
        binding.metadata.fingerprint_sha256 = hex::encode(Sha256::digest(cert));
        android(&cfg, &path, &binding, &cancel).await.unwrap();
        binding.metadata.fingerprint_sha256 = "0".repeat(64);
        assert!(android(&cfg, &path, &binding, &cancel).await.is_err());
    }
}

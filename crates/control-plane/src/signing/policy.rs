//! [INPUT]: 依赖执行目标、构建固定的签名绑定与安全路径规则。
//! [OUTPUT]: 校验默认签名不可省略，输出覆盖及应用平台匹配。
//! [POS]: Poll 和 Kubernetes 提交计划共用的签名授权约束。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{
    recipe::{Target, safe_relative_path},
    signing::Binding,
};
use anyhow::{Result, ensure};

pub fn validate(target: &Target, bindings: &[Binding]) -> Result<()> {
    if !bindings.is_empty() {
        ensure!(
            target.application_platform.is_some(),
            "application_platform is required when project signing defaults exist"
        );
    }
    if bindings.iter().any(|b| b.platform == "macos")
        && target.os == "darwin"
        && !(target.application_platform.as_deref() == Some("android")
            && target
                .artifacts
                .iter()
                .all(|p| p.ends_with(".apk") || p.ends_with(".aab")))
    {
        ensure!(
            target.application_platform.as_deref() == Some("macos"),
            "darwin target must use its macOS signing default"
        );
    }
    if bindings.iter().any(|b| b.platform == "android")
        && target
            .artifacts
            .iter()
            .any(|p| p.ends_with(".apk") || p.ends_with(".aab"))
    {
        ensure!(
            target.application_platform.as_deref() == Some("android"),
            "Android artifact must use its Android signing default"
        );
    }
    let binding = bindings
        .iter()
        .find(|b| Some(b.platform.as_str()) == target.application_platform.as_deref());
    if let Some(binding) = binding {
        ensure!(
            target.signing_profiles == [binding.profile.clone()],
            "project default signing profile is required"
        );
        ensure!(
            !target.signing_outputs.is_empty(),
            "managed signing requires final signing_outputs"
        );
        ensure!(
            target.steps.iter().any(|s| s.phase == "sign")
                && target.steps.iter().any(|s| s.phase == "verify"),
            "managed signing requires sign and verify steps"
        );
        let mut paths = std::collections::HashSet::new();
        for output in &target.signing_outputs {
            ensure!(paths.insert(&output.path), "duplicate signing output");
            ensure!(
                safe_relative_path(&output.path) && !output.path.contains(['*', '?', '[', ']']),
                "signing output must be a literal relative path"
            );
            ensure!(
                target.artifacts.contains(&output.path),
                "signing output must be an explicit artifact"
            );
            if binding.platform == "macos" {
                ensure!(
                    target.os == "darwin"
                        && (output.path.ends_with(".zip") || output.path.ends_with(".dmg")),
                    "macOS signing outputs require ZIP or DMG on darwin"
                );
                ensure!(
                    output
                        .bundle
                        .as_deref()
                        .is_some_and(|p| safe_relative_path(p) && p.ends_with(".app")),
                    "macOS output requires relative .app bundle path"
                );
            } else {
                let extension =
                    if binding.metadata.purpose == Some(crate::signing::AndroidPurpose::Upload) {
                        ".aab"
                    } else {
                        ".apk"
                    };
                ensure!(
                    output.path.ends_with(extension) && output.bundle.is_none(),
                    "Android signing purpose does not match output format"
                );
            }
        }
        ensure!(
            target
                .artifacts
                .iter()
                .all(|p| target.signing_outputs.iter().any(|o| &o.path == p)),
            "managed signing artifacts must all have verification evidence"
        );
    } else {
        ensure!(
            target.signing_outputs.is_empty(),
            "signing_outputs require a configured project default"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        recipe::Recipe,
        signing::{AndroidPurpose, Metadata},
    };
    #[test]
    fn default_signing_cannot_be_omitted_or_disguised_as_a_host_platform() {
        let recipe=Recipe::parse("version: 1\ntargets:\n  android:\n    os: linux\n    arch: amd64\n    steps: [{name: build, run: ./gradlew assembleRelease}]\n    artifacts: [dist/app.apk]\n").unwrap();
        let mut target = recipe.targets["android"].clone();
        let bindings = vec![Binding {
            profile: "managed:android".into(),
            platform: "android".into(),
            credential_id: "c".into(),
            version_id: "v".into(),
            notarize: false,
            metadata: Metadata {
                subject: "test".into(),
                fingerprint_sha256: "0".repeat(64),
                not_before: 0,
                not_after: i64::MAX,
                identity: "test".into(),
                team_id: None,
                alias: Some("release".into()),
                purpose: Some(AndroidPurpose::Application),
                notarization_available: false,
            },
        }];
        assert!(validate(&target, &bindings).is_err());
        target.application_platform = Some("android".into());
        assert!(validate(&target, &bindings).is_err());
        target.signing_profiles = vec!["managed:android".into()];
        target.signing_outputs = vec![crate::signing::Output {
            path: "dist/app.apk".into(),
            bundle: None,
        }];
        target.steps.push(crate::recipe::Step {
            name: "sign".into(),
            phase: "sign".into(),
            run: "apksigner".into(),
            shell: None,
            timeout_seconds: 10,
        });
        target.steps.push(crate::recipe::Step {
            name: "verify".into(),
            phase: "verify".into(),
            run: "apksigner verify".into(),
            shell: None,
            timeout_seconds: 10,
        });
        validate(&target, &bindings).unwrap();
        target.signing_outputs[0].path = "dist/*.apk".into();
        assert!(validate(&target, &bindings).is_err());
    }
}

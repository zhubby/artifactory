//! [INPUT]: 依赖固定源码元数据、节点能力和执行计划。
//! [OUTPUT]: 校验本次构建的完整目标合同并读取构建日志。
//! [POS]: Coordinator 提案进入业务调度的校验边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{execution_plan::ExecutionPlan, recipe::Recipe, state::AppState};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use uuid::Uuid;

pub async fn nodes(state: &AppState, build: Uuid) -> Result<Vec<Value>> {
    state.store.planning_nodes(build).await
}

pub(crate) async fn read_logs(state: &AppState, build: Uuid, args: &Value) -> Result<Value> {
    let target = args.get("target").and_then(Value::as_str).unwrap_or("");
    let rows = state.store.planning_logs(build, target).await?;
    let mut result = Vec::new();
    let mut size = 0;
    for row in rows.into_iter().rev() {
        let bytes = state.storage.get_log(&row.object_key).await?;
        let text = String::from_utf8_lossy(&bytes);
        let text = text
            .chars()
            .take((128 * 1024usize).saturating_sub(size))
            .collect::<String>();
        size += text.len();
        result.push(json!({"step":row.step,"content":text}));
        if size >= 128 * 1024 {
            break;
        }
    }
    Ok(json!({"logs":result}))
}

fn target_requirements(recipe: &Recipe, requirements: &Value) -> Result<()> {
    let requirements = requirements
        .as_array()
        .context("target requirements must be an array")?;
    ensure!(
        requirements.len() == recipe.targets.len(),
        "plan must implement every build target exactly once"
    );
    for requirement in requirements {
        let key = requirement["key"]
            .as_str()
            .context("target key is required")?;
        let target = recipe
            .targets
            .get(key)
            .context("plan omits a build target")?;
        match requirement["kind"].as_str() {
            Some("image") => {
                let image = target
                    .image
                    .as_ref()
                    .context("build image requires BuildKit")?;
                ensure!(
                    target.executor == "buildkit" && image.name == key,
                    "image component identity changed"
                );
                let expected: Vec<String> =
                    serde_json::from_value(requirement["platforms"].clone())?;
                let mut expected = expected;
                let mut actual = image.platforms.clone();
                expected.sort();
                actual.sort();
                ensure!(
                    expected == actual,
                    "image platforms differ from build targets"
                );
                ensure!(
                    image.tags.is_empty(),
                    "image tags are controlled by Release publication"
                );
                ensure!(
                    !image
                        .build_args
                        .keys()
                        .any(|key| key.starts_with("ARTIFACTORY_RELEASE_")),
                    "Release build arguments are reserved"
                );
            }
            Some("native") => {
                ensure!(
                    target.executor == "native"
                        && requirement["os"].as_str() == Some(target.os.as_str())
                        && requirement["arch"].as_str() == Some(target.arch.as_str())
                        && requirement["application_platform"].as_str()
                            == target.application_platform.as_deref(),
                    "native platform differs from build target"
                );
            }
            _ => anyhow::bail!("invalid build target kind"),
        }
    }
    Ok(())
}

pub(crate) async fn prepare_plan(
    state: &AppState,
    context: crate::store::SubmissionContext,
    value: Value,
) -> Result<ExecutionPlan> {
    let mut plan = ExecutionPlan::parse(value)?;
    crate::images::ensure_targets_enabled(plan.recipe.targets.values(), crate::images::enabled())?;
    let source = state
        .store
        .source_manifest(context.source_manifest_id)
        .await?;
    plan.recipe.environment(&context.parameters)?;
    target_requirements(&plan.recipe, &context.target_requirements)?;
    let planning_context = context.planning_context;
    if planning_context["placement_version"] == 1 {
        ensure!(
            plan.recipe
                .targets
                .iter()
                .filter(|(_, target)| target.executor == "native")
                .map(|(key, _)| key)
                .eq(plan.placements.keys()),
            "AI placement is required for every native target"
        );
    } else {
        // A newer worker must preserve the scheduler selected when this build began.
        plan.placements.clear();
    }
    let permitted: Vec<String> = serde_json::from_value(
        planning_context
            .get("signing_profiles")
            .cloned()
            .unwrap_or(json!([])),
    )?;
    let selected: Vec<String> = context.selected_targets;
    ensure!(
        selected.is_empty()
            || plan.recipe.targets.keys().all(|key| selected.contains(key))
                && selected
                    .iter()
                    .all(|key| plan.recipe.targets.contains_key(key)),
        "plan target names differ from requested targets"
    );
    let signing_defaults: Vec<crate::signing::Binding> = serde_json::from_value(
        planning_context
            .get("signing_defaults")
            .cloned()
            .unwrap_or(json!([])),
    )?;
    for target in plan.recipe.targets.values_mut() {
        crate::signing::policy::validate(target, &signing_defaults)?;
        if !target.signing_outputs.is_empty() {
            let binding = signing_defaults
                .iter()
                .find(|b| Some(b.platform.as_str()) == target.application_platform.as_deref())
                .context("missing signing binding")?;
            let mut tools = vec!["managed-signing-v1"];
            if binding.platform == "macos" {
                tools.extend(["codesign", "security"]);
                if target
                    .signing_outputs
                    .iter()
                    .any(|o| o.path.ends_with(".zip"))
                {
                    tools.push("ditto");
                }
                if target
                    .signing_outputs
                    .iter()
                    .any(|o| o.path.ends_with(".dmg"))
                {
                    tools.push("hdiutil");
                }
                if binding.notarize {
                    tools.push("xcrun");
                }
            } else if binding.metadata.purpose == Some(crate::signing::AndroidPurpose::Upload) {
                tools.extend(["jarsigner", "keytool"]);
            } else {
                tools.push("apksigner");
            }
            for tool in tools {
                if !target.capabilities.iter().any(|c| c == tool) {
                    target.capabilities.push(tool.into());
                }
            }
        }
        ensure!(
            target
                .signing_profiles
                .iter()
                .all(|name| permitted.contains(name)),
            "plan exceeds project signing authorization"
        );
    }
    source_capabilities(&mut plan.recipe, &source);
    let current: Option<Uuid> = context.current_plan_id;
    let failed: Vec<String> = context.failed_targets;
    if current.is_some() {
        let previous = context.previous.context("previous plan missing")?;
        let previous = ExecutionPlan::parse(previous)?;
        ensure!(
            plan.recipe
                .targets
                .keys()
                .eq(previous.recipe.targets.keys()),
            "revision cannot change target set"
        );
        for (name, target) in &plan.recipe.targets {
            let old = &previous.recipe.targets[name];
            ensure!(
                target.os == old.os
                    && target.arch == old.arch
                    && target.application_platform == old.application_platform
                    && target
                        .signing_profiles
                        .iter()
                        .all(|p| old.signing_profiles.contains(p)),
                "revision cannot change platform or expand signing authorization"
            );
            if !failed.contains(name) {
                ensure!(
                    serde_json::to_value(target)? == serde_json::to_value(old)?,
                    "revision cannot change a successful target"
                );
            }
        }
    }
    // Publication is controlled by explicit API requests and release authorization.
    plan.recipe.release.enabled = true;
    ExecutionPlan::parse(serde_json::to_value(plan)?)
}

fn source_capabilities(recipe: &mut Recipe, source: &artifactory_source_checkout::SourceManifest) {
    if source
        .repositories
        .iter()
        .any(|repo| !repo.lfs_objects.is_empty())
    {
        for target in recipe
            .targets
            .values_mut()
            .filter(|target| target.executor == "native")
        {
            if !target.capabilities.iter().any(|c| c == "git-lfs") {
                target.capabilities.push("git-lfs".into());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lfs_capabilities_keep_mixed_plans_valid_after_persistence() {
        let value = json!({"summary":"mixed plan","evidence":[{"path":"Dockerfile","reason":"build context"}],"recipe":{"version":1,"targets":{"image":{"os":"linux","arch":"amd64","application_platform":null,"executor":"buildkit","image":{"name":"app"}},"native":{"os":"linux","arch":"amd64","application_platform":null,"artifacts":["dist/*"],"steps":[{"name":"build","run":"echo ok"}]}}}});
        let mut plan = ExecutionPlan::parse(value).unwrap();
        let source = artifactory_source_checkout::SourceManifest::new(
            Uuid::new_v4(),
            vec![artifactory_source_checkout::SourceRepository {
                path: String::new(),
                upstream_url: "https://example.com/repo.git".into(),
                commit_sha: "a".repeat(40),
                tags: Default::default(),
                lfs_objects: vec![artifactory_source_checkout::LfsObject {
                    path: "data.bin".into(),
                    oid: "b".repeat(64),
                    size: 42,
                }],
            }],
        )
        .unwrap();
        source_capabilities(&mut plan.recipe, &source);
        let persisted = serde_json::to_value(&plan).unwrap();
        let mut revision = ExecutionPlan::parse(persisted.clone())
            .expect("saved plans must reparse for assignment and revisions");
        assert_eq!(plan.recipe.targets["native"].capabilities, vec!["git-lfs"]);
        assert!(plan.recipe.targets["image"].capabilities.is_empty());
        source_capabilities(&mut revision.recipe, &source);
        assert_eq!(serde_json::to_value(revision).unwrap(), persisted);
    }
    #[test]
    fn image_platform_requirements_cover_the_index_platforms() {
        let recipe=Recipe::parse("version: 1\ntargets:\n  image:\n    os: linux\n    arch: amd64\n    executor: buildkit\n    image:\n      name: app\n      platforms: [linux/amd64, linux/arm64]\n").unwrap();
        assert!(
            target_requirements(
                &recipe,
                &json!([{"kind":"image","key":"image","platforms":["linux/amd64","linux/arm64"]}])
            )
            .is_err(),
            "component key must match image.name"
        );
        let mut recipe = recipe;
        recipe
            .targets
            .get_mut("image")
            .unwrap()
            .image
            .as_mut()
            .unwrap()
            .name = "image".into();
        assert!(
            target_requirements(
                &recipe,
                &json!([{"kind":"image","key":"image","platforms":["linux/arm64","linux/amd64"]}])
            )
            .is_ok()
        );
        assert!(
            target_requirements(
                &recipe,
                &json!([{"kind":"image","key":"image","platforms":["linux/amd64"]}])
            )
            .is_err()
        );
        assert!(target_requirements(&recipe, &json!([])).is_err());
    }
}

/**
 * [INPUT]: 依赖构建目标合同和构建摘要类型。
 * [OUTPUT]: 提供构建目标校验及源码、平台摘要。
 * [POS]: 构建引导与发布候选共用的边界；服务端执行最终校验。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import type { Build, BuildTarget } from "@/lib/types";

const safeKey = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;
export function validBuildTargets(targets: BuildTarget[]) {
  return targets.length > 0 && targets.length <= 32 && new Set(targets.map(target => target.key)).size === targets.length && targets.every(target => {
    if (target.key.length > 63 || !safeKey.test(target.key)) return false;
    if (target.kind === "image") return target.platforms.length > 0;
    if (target.artifact_key && (target.artifact_key.length > 63 || !safeKey.test(target.artifact_key))) return false;
    if (target.format_suffix && (target.format_suffix.length > 32 || !/^\.[A-Za-z0-9]+(?:\.[A-Za-z0-9]+)*$/.test(target.format_suffix))) return false;
    const app = target.application_platform;
    return !app || ((app === "macos" || app === "ios") ? target.os === "darwin" : app === "android" ? target.os !== "windows" : app === target.os);
  });
}

export function buildTargetSummary(build: Pick<Build, "selected_targets" | "target_requirements">) {
  return build.target_requirements.map(target => target.kind === "image"
    ? `${target.key} (${target.platforms.join(", ")})`
    : `${target.key} (${target.os}/${target.arch})`).join(" · ") || build.selected_targets.join(" · ");
}

export function buildDeliveryLabel(build: Build) {
  return `#${build.number} · ${build.requested_ref} · ${build.commit_sha || "—"} · ${buildTargetSummary(build)}`;
}

/**
 * [INPUT]: 依赖 Release 版本字符串与目标合同。
 * [OUTPUT]: 提供严格版本校验、SemVer 排序与目标合同校验。
 * [POS]: Release 表单和列表共用的客户端边界；服务端执行最终校验。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import type { ReleaseTarget } from "@/lib/types";

const numeric = "(?:0|[1-9][0-9]*)";
const identifier = "(?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)";
const semver = new RegExp(`^${numeric}\\.${numeric}\\.${numeric}(?:-${identifier}(?:\\.${identifier})*)?$`);
export function isReleaseVersion(value: string) {
  return value.length <= 100 && semver.test(value);
}
function compareNumeric(a: string, b: string) {
  return a.length - b.length || (a < b ? -1 : a > b ? 1 : 0);
}
export function compareReleaseVersions(a: string, b: string): number {
  const [aCore, aPre] = a.split(/-(.*)/s);
  const [bCore, bPre] = b.split(/-(.*)/s);
  const aParts = aCore.split(".");
  const bParts = bCore.split(".");
  for (let index = 0; index < 3; index++) {
    const difference = compareNumeric(aParts[index], bParts[index]);
    if (difference) return difference;
  }
  if (aPre === undefined || bPre === undefined) return aPre === bPre ? 0 : aPre === undefined ? 1 : -1;
  const aIdentifiers = aPre.split(".");
  const bIdentifiers = bPre.split(".");
  for (let index = 0; index < Math.min(aIdentifiers.length, bIdentifiers.length); index++) {
    const x = aIdentifiers[index], y = bIdentifiers[index];
    if (x === y) continue;
    const xNumeric = /^[0-9]+$/.test(x), yNumeric = /^[0-9]+$/.test(y);
    return xNumeric && yNumeric ? compareNumeric(x, y) : xNumeric !== yNumeric ? xNumeric ? -1 : 1 : x < y ? -1 : 1;
  }
  return aIdentifiers.length - bIdentifiers.length;
}
const safeKey = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;
export function validReleaseTargets(targets: ReleaseTarget[]) {
  return targets.length > 0 && targets.length <= 32 && new Set(targets.map(target => target.key)).size === targets.length && targets.every(target => {
    if (target.key.length > 63 || !safeKey.test(target.key)) return false;
    if (target.kind === "image") return target.platforms.length > 0;
    if (target.artifact_key && (target.artifact_key.length > 63 || !safeKey.test(target.artifact_key))) return false;
    if (target.format_suffix && (target.format_suffix.length > 32 || !/^\.[A-Za-z0-9]+(?:\.[A-Za-z0-9]+)*$/.test(target.format_suffix))) return false;
    const app = target.application_platform;
    return !app || ((app === "macos" || app === "ios") ? target.os === "darwin" : app === "android" ? target.os !== "windows" : app === target.os);
  });
}

/**
 * [INPUT]: 依赖构建准入等本地错误词条键和原始异常。
 * [OUTPUT]: 提供 LocalizedError、asError 与轮询错误等价判断。
 * [POS]: 将请求异常转换为可随语言切换的错误。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
type LocalErrorKey =
  | "redesign.errorBody"
  | "common.networkFailed"
  | "common.httpFailed"
  | "common.invalidJson"
  | "common.operationFailed"
  | "common.requestFailed"
  | "projects.nameTooLong"
  | "wizard.invalidProject"
  | "wizard.invalidRef"
  | "wizard.invalidGoal"
  | "wizard.invalidPlatforms"
  | "wizard.invalidVersion"
  | "wizard.invalidParameters"
  | "releaseFlow.invalidVersion"
  | "releaseFlow.invalidTargets"
  | "releaseFlow.invalidSource"
  | "buildGuide.devicesLoading"
  | "buildGuide.devicesLoadFailed"
  | "buildGuide.devicesInvalid"
  | "login.failed"
  | "register.failed"
  | "register.invalidUsername"
  | "register.invalidPassword"
  | "register.invalidFields"
  | "register.passwordMismatch"
  | "register.usernameTaken"
  | "register.forbidden"
  | "register.rateLimited"
  | "register.configFailed";

// Retain translation keys until rendering so existing errors follow the locale.
export class LocalizedError extends Error {
  constructor(
    readonly key: LocalErrorKey,
    readonly values: Record<string, string | number> = {},
  ) {
    super(key);
  }
}

export function asError(
  error: unknown,
  fallback: LocalErrorKey = "common.operationFailed",
) {
  return error instanceof Error ? error : new LocalizedError(fallback);
}

export function sameError(left: Error | undefined, right: Error) {
  if (!left || left.constructor !== right.constructor) return false;
  if (left instanceof LocalizedError && right instanceof LocalizedError)
    return left.key === right.key && JSON.stringify(left.values) === JSON.stringify(right.values);
  return left.name === right.name && left.message === right.message;
}

/**
 * [INPUT]: 依赖本地错误词条键和原始异常。
 * [OUTPUT]: 提供 LocalizedError 与 asError。
 * [POS]: 将请求异常转换为可随语言切换的错误。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
type LocalErrorKey =
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

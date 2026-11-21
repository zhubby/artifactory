/**
 * [INPUT]: 依赖平台模型协议、显式接口能力和 Pi 模型适配器。
 * [OUTPUT]: 构造单任务模型、独立消息角色及缓存边界。
 * [POS]: 规划请求的协议边界；模型名称只作为供应商标识。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { createModels, createProvider, type Api, type Model } from "@earendil-works/pi-ai";
import { openAICompletionsApi } from "@earendil-works/pi-ai/api/openai-completions.lazy";
import { openAIResponsesApi } from "@earendil-works/pi-ai/api/openai-responses.lazy";
import { anthropicMessagesApi } from "@earendil-works/pi-ai/api/anthropic-messages.lazy";

export interface ModelConfiguration {
  api: "openai-completions" | "openai-responses" | "anthropic-messages";
  base_url: string; model: string; api_key: string;
  parameters: {
    thinking_level?: string; max_tokens?: number; context_window?: number;
    temperature?: number; requests_per_minute?: number;
    compat?: {
      system_role?: "system" | "developer";
      thinking_format?: "none" | "reasoning_effort" | "enable_thinking" | "thinking";
      max_tokens_field?: "max_tokens" | "max_completion_tokens";
    };
  };
}

export function modelConfiguration(config: ModelConfiguration) {
  const provider = "artifactory-task";
  const parameters = config.parameters || {};
  const completions = config.api === "openai-completions";
  const compat = parameters.compat || {};
  const thinking = compat.thinking_format || "none";
  const model: Model<Api> = {
    id: config.model, name: config.model, api: config.api, provider,
    baseUrl: config.base_url,
    reasoning: completions ? thinking !== "none" : !!parameters.thinking_level && parameters.thinking_level !== "off",
    ...(completions ? { compat: {
      supportsDeveloperRole: false,
      supportsReasoningEffort: thinking === "reasoning_effort",
      thinkingFormat: thinking === "enable_thinking" ? "qwen" as const : thinking === "thinking" ? "deepseek" as const : "openai" as const,
      maxTokensField: compat.max_tokens_field || "max_tokens" as const,
      supportsStore: false,
      requiresReasoningContentOnAssistantMessages: false,
      sendSessionAffinityHeaders: false,
      supportsLongCacheRetention: false,
      promptCacheKeyMode: "disabled" as const,
    } } : {}),
    input: ["text"], cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
    contextWindow: parameters.context_window || 128000, maxTokens: parameters.max_tokens || 8192,
  };
  // 消息角色由配置独立决定。Pi 的 reasoning 开关不承担此职责。
  const onPayload = completions ? (payload: unknown) => {
    const messages = (payload as { messages?: { role: string }[] }).messages;
    for (const message of messages || []) {
      if (message.role === "system" || message.role === "developer") message.role = compat.system_role || "system";
    }
  } : undefined;
  const api = config.api === "anthropic-messages" ? anthropicMessagesApi() : config.api === "openai-responses" ? openAIResponsesApi() : openAICompletionsApi();
  const models = createModels();
  models.setProvider(createProvider({ id: provider, baseUrl: config.base_url, models: [model], auth: { apiKey: { name: "Platform task credential", resolve: async () => ({ auth: { apiKey: config.api_key } }) } }, api }));
  return { models, model, onPayload, cacheRetention: completions ? "none" as const : undefined };
}

/**
 * [INPUT]: 依赖通知 API 的公开数据合同。
 * [OUTPUT]: 提供收件箱、修订、偏好和 Webhook 类型。
 * [POS]: 账户收件箱与外部投递配置共享的类型边界。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
export const notificationCategories = [
  "build",
  "release",
  "permission",
  "device",
  "credential",
  "account",
  "workspace",
  "delivery",
] as const;
export type NotificationCategory = (typeof notificationCategories)[number];
export type LocalizedText = { zh: string; en: string };
export type NotificationItem = {
  id: string;
  category: NotificationCategory;
  event_type: string;
  severity: string;
  title: LocalizedText;
  summary: LocalizedText;
  tenant_id: string | null;
  tenant_name: string | null;
  tenant_slug: string | null;
  project_id: string | null;
  project_name: string | null;
  project_slug: string | null;
  resource_id: string | null;
  resource_type: string;
  url: string | null;
  occurred_at: string;
  updated_at: string;
  content_revision: number;
  attention_revision: number;
  read_revision: number;
  needs_action: boolean;
};
export type NotificationSummary = {
  unread_count: number;
  needs_action_count: number;
  watermark: number;
};
export type NotificationList = NotificationSummary & {
  items: NotificationItem[];
  next_cursor: string | null;
};
export type NotificationDetail = {
  item: NotificationItem;
  events: {
    id: string;
    event_type: string;
    title: LocalizedText;
    summary: LocalizedText;
    occurred_at: string;
  }[];
  next_cursor: string | null;
};
export type NotificationPreferences = {
  in_app: Record<string, boolean>;
  email: Record<string, boolean>;
  locale: "zh-CN" | "en";
  email_address: string | null;
  email_verified: boolean;
  email_available: boolean;
};
export type WebhookEndpoint = {
  id: string;
  name: string;
  url_display: string;
  enabled: boolean;
  event_types: string[];
  generation: number;
  last_status: string | null;
  created_at: string;
};
export type WebhookDelivery = {
  id: string;
  event_type: string;
  status: string;
  attempts: number;
  next_attempt_at: string | null;
  last_error: string | null;
  created_at: string;
};

export type NotificationScopes = {
  tenants: { id: string; name: string; slug: string }[];
  projects: { id: string; name: string; slug: string; tenant_id: string }[];
};

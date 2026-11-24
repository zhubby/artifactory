/**
 * [INPUT]: 依赖当前界面语言与通知中心中英文文案。
 * [OUTPUT]: 提供按语言选择的通知与项目星标操作文案。
 * [POS]: 收件箱与投递设置共享的语言边界。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";
import { useLocale } from "next-intl";
export const notificationCopy = {
  unsubscribeTitle: ["退订邮件通知", "Unsubscribe from email notifications"],
  unsubscribeHint: [
    "确认后将关闭此邮箱的邮件通知。",
    "Confirm to turn off notification emails for this address.",
  ],
  unsubscribe: ["确认退订", "Confirm unsubscribe"],
  unsubscribed: ["已退订邮件通知", "Unsubscribed from email notifications"],
  unsaved: [
    "有未保存的修改，确定离开？",
    "You have unsaved changes. Leave anyway?",
  ],
  title: ["通知中心", "Notifications"],
  personal: ["跨工作区的个人收件箱", "Your inbox across workspaces"],
  all: ["全部", "All"],
  unread: ["未读", "Unread"],
  action: ["需要处理", "Needs action"],
  category: ["通知类别", "Category"],
  tenant: ["工作区", "Workspace"],
  project: ["项目", "Project"],
  allTenants: ["所有工作区", "All workspaces"],
  allProjects: ["所有项目", "All projects"],
  allCategories: ["所有类别", "All categories"],
  read: ["标为已读", "Mark as read"],
  readAll: ["全部标为已读", "Mark all as read"],
  empty: ["暂无通知", "No notifications"],
  emptyHint: [
    "构建进展和需要你处理的事项会显示在这里。",
    "Build updates and items that need your attention appear here.",
  ],
  more: ["加载更多", "Load more"],
  open: ["打开资源", "Open resource"],
  history: ["事件记录", "Event history"],
  back: ["返回收件箱", "Back to inbox"],
  full: ["查看全部通知", "View all notifications"],
  settings: ["通知设置", "Notification settings"],
  follow: ["关注项目", "Follow project"],
  unfollow: ["取消关注项目", "Unfollow project"],
  retryFollow: ["重试读取项目关注状态", "Retry loading project follow status"],
  following: ["已关注", "Following"],
  followHint: [
    "接收此项目的构建和发布通知",
    "Receive build and release notifications for this project",
  ],
  email: ["通知邮箱", "Notification email"],
  verified: ["邮箱已验证", "Email verified"],
  pending: ["等待验证", "Awaiting verification"],
  emailHint: [
    "验证邮箱后，可主动开启各类邮件通知。",
    "Verify your email, then opt in to the categories you want.",
  ],
  unavailable: [
    "平台尚未配置邮件发送服务",
    "Email delivery is not configured on this platform",
  ],
  address: ["邮箱地址", "Email address"],
  saveEmail: ["发送验证邮件", "Send verification email"],
  resend: ["重新发送", "Resend verification"],
  remove: ["移除邮箱", "Remove email"],
  inApp: ["站内通知", "In-app"],
  emailChannel: ["邮件", "Email"],
  inAppOnly: ["仅站内", "In-app only"],
  language: ["邮件语言", "Email language"],
  verifyTitle: ["验证通知邮箱", "Verify notification email"],
  verifyHint: [
    "确认后将验证此链接对应的邮箱。",
    "Confirm to verify the email address associated with this link.",
  ],
  verify: ["确认验证邮箱", "Confirm email verification"],
  invalidToken: [
    "验证链接缺少令牌，请重新发送验证邮件。",
    "This link has no token. Request a new verification email.",
  ],
  webhooks: ["Webhook 集成", "Webhook integrations"],
  webhookHint: [
    "仅向你明确启用的地址投递选定事件。",
    "Deliver selected events only to endpoints you explicitly enable.",
  ],
  name: ["名称", "Name"],
  url: ["HTTPS 地址", "HTTPS URL"],
  eventTypes: ["事件类型（逗号分隔）", "Event types (comma separated)"],
  disabled: ["已停用", "Disabled"],
  enabled: ["启用投递", "Enable delivery"],
  create: ["添加 Webhook", "Add webhook"],
  test: ["发送测试", "Send test"],
  rotate: ["轮换签名密钥", "Rotate signing secret"],
  deliveries: ["投递记录", "Deliveries"],
  retry: ["重试", "Retry"],
  secret: [
    "签名密钥仅显示一次，请安全保存。",
    "This signing secret is shown once. Store it securely.",
  ],
  deleteConfirm: ["删除此 Webhook？", "Delete this webhook?"],
  build: ["构建", "Builds"],
  release: ["发布", "Releases"],
  permission: ["权限", "Permissions"],
  device: ["设备", "Devices"],
  credential: ["凭据", "Credentials"],
  account: ["账户", "Account"],
  workspace: ["工作区", "Workspace"],
  delivery: ["投递", "Delivery"],
  close: ["关闭", "Close"],
  save: ["保存", "Save"],
  delete: ["删除", "Delete"],
  noEndpoints: ["尚未配置 Webhook", "No webhook endpoints"],
  noDeliveries: ["暂无投递记录", "No deliveries yet"],
  confirm: ["确认", "Confirm"],
  saved: ["已保存", "Saved"],
  failed: ["请求失败，请重试", "Request failed. Try again"],
  return: ["返回工作区大厅", "Back to workspaces"],
  notificationLocale: ["通知语言", "Notification language"],
} as const;
export type NotificationCopyKey = keyof typeof notificationCopy;
export function useNotificationCopy() {
  const locale = useLocale();
  return (key: NotificationCopyKey) =>
    notificationCopy[key][locale === "en" ? 1 : 0];
}

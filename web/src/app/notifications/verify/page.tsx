/**
 * [INPUT]: 依赖一次性邮箱令牌与显式确认组件。
 * [OUTPUT]: 提供邮箱验证或退订页面。
 * [POS]: 邮件链接的安全确认边界；GET 不变更订阅。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import {NotificationEmailVerify} from "@/components/notification-settings";
export default function Page(){return <NotificationEmailVerify/>}

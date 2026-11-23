//! [INPUT]: 依赖白名单事件类型、状态与安全上下文。
//! [OUTPUT]: 提供中英文通知文案与受限资源链接。
//! [POS]: 收件箱和外部通道共用的安全展示边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalizedText {
    pub zh: String,
    pub en: String,
}
fn text(zh: &str, en: &str) -> LocalizedText {
    LocalizedText {
        zh: zh.into(),
        en: en.into(),
    }
}

/// Catalog registration is explicit; newer schemas must ship a matching renderer.
pub fn is_supported(event_type: &str, version: i32) -> bool {
    version == 1
        && matches!(
            event_type,
            "build.status.changed"
                | "build.phase.changed"
                | "release.publication.changed"
                | "permission.member.changed"
                | "permission.resources.changed"
                | "device.status.changed"
                | "device.update.changed"
                | "credential.status.changed"
                | "account.security.changed"
                | "workspace.lifecycle.changed"
                | "delivery.channel.changed"
        )
}

/// Links use current ACL-filtered names from the store, never payload URLs.
pub(crate) fn resource_url(context: &Value) -> Option<String> {
    let kind = context["resource_type"].as_str()?;
    if kind == "account_safe" {
        return None;
    }
    let slug = |value: &Value| -> Option<String> {
        let text = value.as_str()?;
        (text.len() <= 63
            && text.split('-').all(|part| {
                !part.is_empty()
                    && part
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            }))
        .then(|| text.to_owned())
    };
    let tenant = slug(&context["tenant_slug"])?;
    let tenant_base = format!("/tenants/{tenant}");
    let event = context["event_type"].as_str().unwrap_or_default();
    let aggregate = context["aggregate_key"].as_str().unwrap_or_default();
    if let Some(project) = slug(&context["project_slug"]) {
        let base = format!("{tenant_base}/projects/{project}");
        return match kind {
            "build" => context["payload"]["build_number"]
                .as_i64()
                .filter(|n| *n > 0)
                .map(|n| format!("{base}/builds/{n}")),
            "release" => context["resource_id"]
                .as_str()
                .and_then(|id| uuid::Uuid::parse_str(id).ok())
                .map(|id| format!("{base}/releases/{id}")),
            "project" => Some(base),
            "project_admin" => {
                let section = match event {
                    "permission.member.changed" => "members",
                    "permission.resources.changed" => "resources",
                    "credential.status.changed" if aggregate.starts_with("signing-project:") => {
                        "signing"
                    }
                    "credential.status.changed" => "resources",
                    "workspace.lifecycle.changed" => "lifecycle",
                    "delivery.channel.changed" => "integrations",
                    _ => "general",
                };
                Some(format!("{base}/settings/{section}"))
            }
            _ => None,
        };
    }
    match kind {
        "device" | "node" | "nodes" => Some(format!("{tenant_base}/nodes")),
        "credential" | "credentials" if aggregate.starts_with("signing:") => {
            Some(format!("{tenant_base}/settings/signing"))
        }
        "credential" | "credentials" => Some(format!("{tenant_base}/settings")),
        "tenant_admin" => {
            let section = match event {
                "permission.member.changed" => "members",
                "workspace.lifecycle.changed" => "lifecycle",
                "delivery.channel.changed" => "integrations",
                _ => "general",
            };
            Some(format!("{tenant_base}/settings/{section}"))
        }
        _ => None,
    }
}

/// Never render arbitrary diagnostics, URLs, names or exception strings from payloads.
pub fn render(event_type: &str, payload: &Value) -> (LocalizedText, LocalizedText) {
    if event_type == "delivery.channel.changed" && payload["status"] == "resolved" {
        return (
            text("通知投递已恢复", "Notification delivery restored"),
            text(
                "外部通知投递已恢复。",
                "External notification delivery has been restored.",
            ),
        );
    }
    if event_type == "build.phase.changed"
        && matches!(
            payload["status"].as_str(),
            Some("succeeded" | "failed" | "canceled" | "cancelled" | "lost")
        )
    {
        return render("build.status.changed", payload);
    }
    let title = match event_type {
        "build.status.changed" | "build.completed" => text("构建状态更新", "Build status updated"),
        "build.phase.changed" => text("构建阶段更新", "Build phase updated"),
        "release.publication.changed" => text("版本交付更新", "Release publication updated"),
        "permission.member.changed" | "permission.resources.changed" => {
            text("访问权限更新", "Access permissions updated")
        }
        "device.status.changed" => text("设备状态更新", "Device status updated"),
        "device.update.changed" => text("设备升级更新", "Device update changed"),
        "credential.status.changed" => text("凭据状态更新", "Credential status updated"),
        "account.security.changed" => text("账户安全更新", "Account security updated"),
        "workspace.lifecycle.changed" => text("工作区状态更新", "Workspace status updated"),
        "delivery.channel.changed" => text("通知投递异常", "Notification delivery failed"),
        _ => text("通知更新", "Notification updated"),
    };
    let mut summary = match payload.get("status").and_then(Value::as_str) {
        Some("succeeded" | "published" | "completed") => {
            text("操作已完成。", "The operation completed.")
        }
        Some("failed" | "error") => text(
            "操作未完成，请查看当前状态。",
            "The operation failed. Review its current status.",
        ),
        Some(
            "needs_input" | "awaiting_input" | "needs_clarification" | "awaiting_clarification",
        ) => text(
            "需要你的输入才能继续。",
            "Your input is needed to continue.",
        ),
        Some("canceled" | "cancelled") => text("操作已取消。", "The operation was canceled."),
        Some("offline") => text("设备已离线。", "The device is offline."),
        Some("online") => text("设备已上线。", "The device is online."),
        Some("revoked" | "removed") => {
            text("你的访问权限已变更。", "Your access permissions changed.")
        }
        Some("expired" | "expiring") => text("凭据需要更新。", "The credential needs renewal."),
        _ => text(
            "状态已更新，打开通知查看详情。",
            "The status changed. Open the notification for details.",
        ),
    };
    let status = payload
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if event_type == "release.publication.changed" {
        summary = match status {
            "published" | "succeeded" => text("版本交付已发布。", "Release publication succeeded."),
            "failed" => text("版本交付失败。", "Release publication failed."),
            "publishing" | "pending" => {
                text("版本交付正在发布。", "Release publication is in progress.")
            }
            _ => text(
                "版本交付状态已更新。",
                "Release publication status changed.",
            ),
        };
        if payload["build_status"] == "succeeded" {
            summary.zh.push_str(" 构建已成功。");
            summary.en.push_str(" The build succeeded.");
        }
    } else if event_type == "build.status.changed" {
        summary = match status {
            "succeeded" => text("构建已成功。", "The build succeeded."),
            "failed" => text(
                "构建失败，请查看当前状态。",
                "The build failed. Review its current status.",
            ),
            "lost" => text(
                "构建执行连接已中断，请查看当前状态。",
                "The build lost its execution connection. Review its current status.",
            ),
            "queued" => text(
                "构建已排队，正在等待执行资源。",
                "The build is queued and waiting for execution resources.",
            ),
            "running" | "planning" => text("构建正在执行。", "The build is in progress."),
            _ => summary,
        };
    } else if event_type == "build.phase.changed"
        && !matches!(
            status,
            "needs_input" | "awaiting_input" | "needs_clarification" | "awaiting_clarification"
        )
    {
        summary = match payload.get("phase").and_then(Value::as_str) {
            Some("planning" | "plan") => text("正在规划构建。", "Planning the build."),
            Some("source" | "checkout") => text("正在准备源码。", "Preparing source code."),
            Some("build" | "building" | "execution" | "executing") => {
                text("正在执行构建。", "Executing the build.")
            }
            Some("upload" | "uploading" | "artifact") => {
                text("正在保存构建制品。", "Saving build artifacts.")
            }
            Some("publish" | "publishing") => {
                text("正在准备版本交付。", "Preparing release publication.")
            }
            _ => summary,
        };
    } else if event_type == "credential.status.changed" {
        summary = match status {
            "resolved" => text(
                "凭据异常状态已恢复。",
                "The credential alert has been resolved.",
            ),
            "disabled" => text(
                "凭据已停用，请检查使用此凭据的项目。",
                "The credential was disabled. Review projects that use it.",
            ),
            "removed" => text("凭据已删除。", "The credential was deleted."),
            "expiring_7d" => text(
                "凭据将在 7 天内到期，请安排更新。",
                "The credential expires within 7 days. Plan its renewal.",
            ),
            "expiring_1d" => text(
                "凭据将在 24 小时内到期，请及时更新。",
                "The credential expires within 24 hours. Renew it soon.",
            ),
            "expired" => text(
                "凭据已过期，请更新后再使用。",
                "The credential expired. Renew it before using it.",
            ),
            _ => summary,
        };
    } else if event_type == "device.update.changed" {
        summary = match status {
            "resolved" => text(
                "后续设备升级已成功，升级故障已恢复。",
                "The device issue was resolved by a later successful update.",
            ),
            "failed" => text(
                "设备升级失败，请检查设备状态。",
                "The device update failed. Review the device status.",
            ),
            "rolled_back" => text(
                "设备升级已回滚，设备已恢复先前版本。",
                "The device update was rolled back. The previous version was restored.",
            ),
            _ => summary,
        };
    } else if event_type == "device.status.changed" {
        summary = match status {
            "certificate_expired" => text(
                "设备证书已过期，请续期设备证书。",
                "The device certificate expired. Renew the certificate.",
            ),
            "certificate_renewed" | "renewed" => {
                text("设备证书已更新。", "The device certificate was renewed.")
            }
            _ => summary,
        };
    } else if event_type == "delivery.channel.changed" {
        summary = text(
            "外部通知未送达，请检查投递设置。",
            "An external notification could not be delivered. Review the delivery settings.",
        );
    } else if event_type == "account.security.changed" {
        summary = match status {
            "password_changed" => text("账户密码已更改。", "Your account password changed."),
            "token_created" => text(
                "已创建账户访问令牌。",
                "An account access token was created.",
            ),
            "token_revoked" => text(
                "账户访问令牌已撤销。",
                "An account access token was revoked.",
            ),
            _ => text(
                "账户安全设置已更新。",
                "Your account security settings changed.",
            ),
        };
    }
    (title, summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arbitrary_diagnostics_are_never_rendered() {
        let (a, b) = render(
            "build.status.changed",
            &serde_json::json!({"status":"secret-token","error":"password","url":"javascript:alert(1)"}),
        );
        assert!(!format!("{a:?}{b:?}").contains("secret-token"));
        assert!(!format!("{a:?}{b:?}").contains("password"));
    }
    #[test]
    fn publication_failure_does_not_claim_the_build_failed() {
        let (_, summary) = render(
            "release.publication.changed",
            &serde_json::json!({"status":"failed","build_status":"succeeded"}),
        );
        assert!(summary.en.contains("publication failed"));
        assert!(summary.en.contains("build succeeded"));
        assert!(!summary.en.contains("build failed"));
    }
    #[test]
    fn build_phase_uses_catalog_labels() {
        let (_, summary) = render(
            "build.phase.changed",
            &serde_json::json!({"status":"running","phase":"planning"}),
        );
        assert!(summary.en.contains("Planning"));
        let (_, unsafe_summary) = render(
            "build.phase.changed",
            &serde_json::json!({"status":"running","phase":"password-secret"}),
        );
        assert!(!unsafe_summary.en.contains("password-secret"));
    }
    #[test]
    fn late_phase_preserves_terminal_build_result() {
        for phase in ["building", "planning", "uploading"] {
            let (title, summary) = render(
                "build.phase.changed",
                &serde_json::json!({"status":"succeeded","phase":phase}),
            );
            assert!(summary.en.contains("build succeeded"));
            assert_eq!(title.en, "Build status updated");
        }
        let (_, summary) = render(
            "build.phase.changed",
            &serde_json::json!({"status":"failed","phase":"building"}),
        );
        assert!(summary.en.contains("build failed"));
    }
    #[test]
    fn delivery_recovery_uses_success_copy() {
        let (title, summary) = render(
            "delivery.channel.changed",
            &serde_json::json!({"status":"resolved"}),
        );
        assert_eq!(title.en, "Notification delivery restored");
        assert!(!summary.en.contains("could not"));
        assert!(summary.en.contains("restored"));
    }
    #[test]
    fn credential_statuses_have_specific_bilingual_copy() {
        for (status, zh, en) in [
            ("resolved", "恢复", "resolved"),
            ("disabled", "停用", "disabled"),
            ("removed", "删除", "deleted"),
            ("expiring_7d", "7 天", "7 days"),
            ("expiring_1d", "24 小时", "24 hours"),
            ("expired", "过期", "expired"),
        ] {
            let (_, summary) = render(
                "credential.status.changed",
                &serde_json::json!({"status":status}),
            );
            assert!(
                summary.zh.contains(zh),
                "Chinese copy for {status}: {}",
                summary.zh
            );
            assert!(
                summary.en.contains(en),
                "English copy for {status}: {}",
                summary.en
            );
        }
    }
    #[test]
    fn device_recovery_rollback_and_certificate_copy_are_specific() {
        for (event, status, zh, en) in [
            (
                "device.update.changed",
                "resolved",
                "后续设备升级已成功",
                "later successful update",
            ),
            (
                "device.update.changed",
                "failed",
                "升级失败",
                "update failed",
            ),
            (
                "device.update.changed",
                "rolled_back",
                "回滚",
                "rolled back",
            ),
            (
                "device.status.changed",
                "certificate_expired",
                "证书已过期",
                "certificate expired",
            ),
            (
                "device.status.changed",
                "certificate_renewed",
                "证书已更新",
                "certificate was renewed",
            ),
            (
                "device.status.changed",
                "renewed",
                "证书已更新",
                "certificate was renewed",
            ),
        ] {
            let (_, summary) = render(event, &serde_json::json!({"status":status}));
            assert!(
                summary.zh.contains(zh),
                "Chinese copy for {status}: {}",
                summary.zh
            );
            assert!(
                summary.en.contains(en),
                "English copy for {status}: {}",
                summary.en
            );
        }
    }
}

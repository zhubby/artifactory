/**
 * [INPUT]: 依赖系统安装配置 API、服务器权限投影及共享表单。
 * [OUTPUT]: 提供设备公开地址配置与管理员只读说明。
 * [POS]: Emissary 设置的安装入口；权限由服务端真实账户角色决定。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { useUI } from "@/components/providers";
import { ErrorBox, Field, Loading, SectionTitle } from "@/components/common";
import { SubmitForm } from "@/components/forms";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { mutation, useRemote } from "@/lib/api";
import type { NodeInstallationSettings as InstallationSettings } from "@/lib/node-installation";

export function NodeInstallationSettings() {
  const { t } = useUI();
  const settings = useRemote<InstallationSettings>("/system/node-installation", false);
  return <section className="panel" data-testid="node-installation-settings">
    <SectionTitle title={t("installation.settingsTitle")} detail={t("installation.settingsDescription")} />
    <div className="space-y-4 p-6 pt-0">
      <ErrorBox message={settings.error} />
      {!settings.data ? settings.error ? <Button variant="secondary" onClick={() => void settings.reload()}>{t("common.retry")}</Button> : <Loading variant="form" /> : settings.data.can_edit ?
        <SubmitForm key={settings.data.device_endpoint} onSubmit={async form => {
          await mutation("/system/node-installation", { device_endpoint: String(form.get("device_endpoint") || "").trim() }, "PATCH");
          await settings.reload();
        }}><Field label={t("installation.endpoint")} hint={t("installation.endpointHint")}>
          <Input name="device_endpoint" defaultValue={settings.data.device_endpoint} placeholder="agents.example.com:8443" maxLength={512} spellCheck={false} autoComplete="off" />
        </Field></SubmitForm> : <>
          <Field label={t("installation.endpoint")}><Input value={settings.data.device_endpoint} readOnly placeholder={t("installation.notConfigured")} /></Field>
          <p className="text-xs leading-5 text-muted-foreground">{t("installation.systemAdminOnly")}</p>
        </>}
    </div>
  </section>;
}

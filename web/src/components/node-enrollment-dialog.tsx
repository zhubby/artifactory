/**
 * [INPUT]: 依赖租户安装材料 API、共享弹窗、Select、命令生成器和通知。
 * [OUTPUT]: 提供同弹窗安装命令、一次性凭证和过期状态。
 * [POS]: 节点首次加入入口；关闭即销毁材料和待完成请求。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { useEffect, useRef, useState, type ReactNode } from "react";
import { Copy } from "lucide-react";
import { useUI } from "@/components/providers";
import { ErrorBox, Field, Loading } from "@/components/common";
import { Button } from "@/components/ui/button";
import { Modal } from "@/components/ui/dialog";
import { Select } from "@/components/ui/select";
import { api, useRemote } from "@/lib/api";
import { Link } from "@/lib/tenant";
import { asError } from "@/lib/errors";
import { notify } from "@/lib/notify";
import { installationCommand, registrationCommand, type InstallationArch, type InstallationOS,
  type NodeInstallationMaterial, type NodeInstallationOptions } from "@/lib/node-installation";
import type { Pool } from "@/lib/types";

function CopyCommand({ value, disabled, label }: { value: string; disabled: boolean; label: string }) {
  const { t } = useUI();
  return <Button type="button" variant="secondary" disabled={disabled} onClick={async () => {
    try { await navigator.clipboard.writeText(value); notify.success(t("common.copied")); }
    catch (error) { notify.error(asError(error)); }
  }}><Copy />{label}</Button>;
}

function InstallationTarget({ summary, children }: { summary?: string; children: ReactNode }) {
  const { t } = useUI();
  return summary ? <details className="min-w-0 rounded-lg border border-border p-3 sm:p-4">
    <summary className="cursor-pointer text-sm"><span className="font-medium">{t("installation.target")}</span><span className="ml-2 break-words text-xs text-muted-foreground">{summary}</span></summary>
    <div className="mt-4">{children}</div>
  </details> : <>{children}</>;
}

export function NodeEnrollmentDialog({ close }: { close: () => void }) {
  const { t } = useUI();
  const options = useRemote<NodeInstallationOptions>("/node-installation/options", false);
  const pools = useRemote<Pool[]>("/pools", false);
  const error = options.error || pools.error;
  return <Modal open onOpenChange={open => { if (!open) close(); }}
    title={t("installation.title")} description={t("installation.description")}>
    <ErrorBox message={error} />
    {error && <Button type="button" variant="secondary" onClick={() => void Promise.all([options.reload(), pools.reload()])}>{t("common.retry")}</Button>}
    {!options.data || !pools.data ? !error && <Loading variant="form" /> :
      <EnrollmentForm options={options.data} pools={pools.data} close={close} />}
  </Modal>;
}

function EnrollmentForm({ options, pools, close }: { options: NodeInstallationOptions; pools: Pool[]; close: () => void }) {
  const { t } = useUI();
  const [pool, setPool] = useState(pools.find(pool => pool.is_default)?.id || pools[0]?.id || "");
  const [release, setRelease] = useState(options.releases[0]?.id || "");
  const [os, setOS] = useState<InstallationOS>("linux");
  const [arch, setArch] = useState<InstallationArch>("amd64");
  const [material, setMaterial] = useState<NodeInstallationMaterial>();
  const [busy, setBusy] = useState(false);
  const [now, setNow] = useState(Date.now);
  const request = useRef<AbortController | undefined>(undefined);
  useEffect(() => () => request.current?.abort(), []);
  useEffect(() => {
    if (!material) return;
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [material]);
  const seconds = material ? Math.max(0, Math.ceil((Date.parse(material.expires_at) - now) / 1000)) : 0;
  const expired = !Number.isFinite(seconds) || seconds === 0;
  const selected = options.releases.find(item => item.id === release);
  const supported = selected?.platforms.includes(`${os}/${arch}`);
  const ready = !!options.device_endpoint && !!pool && !!selected && !!supported;
  const locked = busy || !!material;
  const targetSummary = material ? `${pools.find(item => item.id === pool)?.name || pool} · ${material.os === "darwin" ? "macOS" : material.os === "windows" ? "Windows" : "Linux"} / ${material.arch.toUpperCase()} · ${material.version}` : undefined;
  async function generate() {
    if (!ready || busy) return;
    const controller = new AbortController();
    request.current = controller;
    setBusy(true);
    try {
      const result = await api<NodeInstallationMaterial>("/node-installation", {
        method: "POST", body: JSON.stringify({ pool_id: pool, release_id: release, os, arch }), signal: controller.signal,
      });
      if (controller.signal.aborted) return;
      setNow(Date.now()); setMaterial(result);
      notify.success(t("installation.generated"));
    } catch (error) {
      if (!controller.signal.aborted) notify.error(asError(error));
    } finally { if (!controller.signal.aborted) setBusy(false); }
  }
  return <div className="space-y-5">
    <InstallationTarget summary={targetSummary}>
    <div className="grid min-w-0 gap-4 sm:grid-cols-2">
      <Field label={t("nodes.joinPool")}><Select aria-label={t("nodes.joinPool")} className="input" value={pool} disabled={locked} onChange={event => setPool(event.target.value)}>
        {pools.map(pool => <option key={pool.id} value={pool.id}>{pool.name}</option>)}
      </Select></Field>
      <Field label={t("installation.version")}><Select aria-label={t("installation.version")} className="input" value={release} disabled={locked} onChange={event => setRelease(event.target.value)}>
        {options.releases.map(release => <option key={release.id} value={release.id}>{release.version}</option>)}
      </Select></Field>
      <Field label={t("installation.os")}><Select aria-label={t("installation.os")} className="input" value={os} disabled={locked} onChange={event => setOS(event.target.value as InstallationOS)}>
        <option value="linux">Linux</option><option value="darwin">macOS</option><option value="windows">Windows</option>
      </Select></Field>
      <Field label={t("installation.arch")}><Select aria-label={t("installation.arch")} className="input" value={arch} disabled={locked} onChange={event => setArch(event.target.value as InstallationArch)}>
        <option value="amd64">AMD64 / x86-64</option><option value="arm64">ARM64</option>
      </Select></Field>
    </div>
    </InstallationTarget>
    {!options.device_endpoint && <p className="text-sm leading-6 text-muted-foreground">{t("installation.missingEndpoint")}</p>}
    {!options.releases.length && <p className="text-sm leading-6 text-muted-foreground">{t("installation.missingRelease")}</p>}
    {!pools.length && <p className="text-sm leading-6 text-muted-foreground">{t("installation.missingPool")}</p>}
    {selected && !supported && <p className="text-sm text-muted-foreground">{t("installation.unsupported")}</p>}
    {(!options.device_endpoint || !options.releases.length) && <Button asChild variant="secondary"><Link href="/settings/emissary" onClick={close}>{t("installation.openSettings")}</Link></Button>}
    {!material ? <Button className="w-full" disabled={!ready || busy} onClick={() => void generate()}>{t(busy ? "common.processing" : "installation.generate")}</Button> : <>
      <div className="space-y-3 rounded-lg border border-border p-3 sm:p-4">
        <div className="flex flex-wrap items-center justify-between gap-3"><h3 className="text-sm font-medium">{t("installation.command")}</h3>
          <CopyCommand value={installationCommand(material)} disabled={expired || busy} label={t("installation.copyCommand")} /></div>
        <p className="text-xs leading-5 text-muted-foreground">{t(os === "windows" ? "installation.windowsHint" : "installation.unixHint")}</p>
        <pre data-testid="node-install-command" tabIndex={0} className="max-h-56 overflow-auto rounded-md bg-muted p-3 font-mono text-xs leading-5 whitespace-pre-wrap break-all">{installationCommand(material)}</pre>
        <p role="status" className="text-xs leading-5 text-muted-foreground">{expired ? t("installation.expired") : t("installation.remaining", { time: `${String(Math.floor(seconds / 60)).padStart(2, "0")}:${String(seconds % 60).padStart(2, "0")}` })}</p>
        <p className="text-xs leading-5 text-muted-foreground">{t("installation.historyHint")}</p>
      </div>
      <details className="min-w-0 rounded-lg border border-border p-3 sm:p-4"><summary className="cursor-pointer text-sm">{t("installation.details")}</summary>
        <div className="mt-4 min-w-0 space-y-4 text-xs">
          <div className="flex flex-wrap items-center justify-between gap-2"><span>{t("installation.token")}</span><CopyCommand value={material.token} disabled={expired || busy} label={t("installation.copyToken")} /></div>
          <code data-testid="node-install-token" className="block break-all rounded bg-muted p-3">{material.token}</code>
          <dl className="space-y-2"><div><dt className="text-muted-foreground">{t("installation.endpoint")}</dt><dd className="break-all font-mono">{material.device_endpoint}</dd></div>
            <div><dt className="text-muted-foreground">{t("installation.caHash")}</dt><dd className="break-all font-mono">{material.ca_sha256}</dd></div></dl>
          <pre className="max-h-32 overflow-auto whitespace-pre-wrap break-all rounded bg-muted p-3">{material.ca_pem}</pre>
        </div>
      </details>
      <details className="min-w-0 rounded-lg border border-border p-3 sm:p-4"><summary className="cursor-pointer text-sm">{t("installation.existing")}</summary>
        <div className="mt-4 space-y-3"><p className="text-xs leading-5 text-muted-foreground">{t("installation.existingHint")}</p>
          <CopyCommand value={registrationCommand(material)} disabled={expired || busy} label={t("installation.copyRegistration")} />
          <pre className="max-h-48 overflow-auto whitespace-pre-wrap break-all rounded bg-muted p-3 font-mono text-xs leading-5">{registrationCommand(material)}</pre>
        </div>
      </details>
      <p className="text-xs leading-5 text-muted-foreground">{t("installation.afterRun")}</p>
      <div className="flex flex-wrap justify-end gap-2"><Button variant="ghost" disabled={busy} onClick={() => setMaterial(undefined)}>{t("installation.changeOptions")}</Button><Button variant="secondary" disabled={busy} onClick={() => void generate()}>{t("installation.regenerate")}</Button><Button onClick={close}>{t("installation.done")}</Button></div>
    </>}
  </div>;
}

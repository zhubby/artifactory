/**
 * [INPUT]: 依赖产品统一版本、OTA API、系统安装配置与设备详情。
 * [OUTPUT]: 提供固定产品版本的手动升级、并发策略与执行历史。
 * [POS]: 节点确认后发起升级；首读展示骨架，轮询保留策略草稿。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { useRef, useState, type ReactNode } from "react";
import { NodeInstallationSettings } from "@/components/node-installation-settings";
import { useUI } from "@/components/providers";
import { ErrorBox, Field, SectionTitle, Loading } from "@/components/common";
import { SubmitForm } from "@/components/forms";
import { NodeFacts } from "@/components/node-detail-parts";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Modal } from "@/components/ui/dialog";
import { mutation, useRemote } from "@/lib/api";
import { asError } from "@/lib/errors";
import { matchesProductVersion } from "@/lib/emissary-version";
import { notify } from "@/lib/notify";
import type { EmissaryRelease, EmissaryUpdate, EmissaryVersion, Node, Pool, User } from "@/lib/types";

const retryable = (u: EmissaryUpdate, version?: EmissaryVersion) => version?.release_status === "ready" && u.release_id === version.release_id && ["failed", "rolled_back", "canceled"].includes(u.status);
const cancelable = (u: EmissaryUpdate) => !u.finished_at && ["queued", "offered", "downloading", "draining"].includes(u.status);

function UpdateActions({ update, version, reload, allowRetry = true }: { update: EmissaryUpdate; version?: EmissaryVersion; reload: () => Promise<unknown>; allowRetry?: boolean }) {
  const { t } = useUI();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<Error>();
  async function act(action: string) {
    setBusy(true); setError(undefined);
    try { await mutation(`/emissary-updates/${update.id}/${action}`, {}); notify.success(t("common.operationSucceeded")); await reload(); }
    catch (e) { setError(asError(e)); } finally { setBusy(false); }
  }
  return <div className="space-y-2"><div className="flex gap-2">
    {allowRetry && retryable(update, version) && <Button size="sm" variant="secondary" disabled={busy} onClick={() => void act("retry")}>{t("ota.retry")}</Button>}
    {cancelable(update) && <Button size="sm" variant="secondary" disabled={busy || update.cancel_requested} onClick={() => void act("cancel")}>{t(update.cancel_requested ? "ota.cancelRequested" : "ota.cancel")}</Button>}
  </div><ErrorBox message={error} /></div>;
}

function ReleaseStatus({ version }: { version: EmissaryVersion }) {
  const { t } = useUI();
  return <p className="text-xs leading-5 text-muted-foreground" data-testid="emissary-release-status">{t(`ota.releaseStatus.${version.release_status}`, { version: version.product_version })}</p>;
}

function RequestUpdate({ node, version, allowed, reload }: { node: Node; version: EmissaryVersion; allowed: boolean; reload: () => Promise<unknown> }) {
  const { t } = useUI();
  const [target, setTarget] = useState<{ product: string; release: string | null }>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<Error>();
  const submitting = useRef(false);
  const unchanged = target?.product === version.product_version && target?.release === version.release_id;
  async function submit() {
    if (!allowed || !unchanged || submitting.current) return;
    submitting.current = true; setBusy(true); setError(undefined);
    try {
      await mutation(`/nodes/${node.id}/emissary-update`, { product_version: target.product, release_id: target.release });
      setTarget(undefined); notify.success(t("ota.requested")); await reload();
    } catch (cause) { setError(asError(cause)); }
    finally { submitting.current = false; setBusy(false); }
  }
  return <>
    <Button variant="secondary" disabled={!allowed || busy} onClick={() => {
      setError(undefined); setTarget({ product: version.product_version, release: version.release_id });
    }}>{t("ota.upgradeTo", { version: version.product_version })}</Button>
    <Modal open={!!target} onOpenChange={open => { if (!open && !submitting.current) setTarget(undefined); }} dismissible={!busy}
      title={t("ota.confirmTitle")} description={t("ota.confirmDescription", { name: node.name, version: target?.product || "" })}>
      <p className="text-sm leading-6 text-muted-foreground">{t("ota.confirmRecovery")}</p>
      {(!allowed || !unchanged) && <p role="status" className="mt-3 text-sm text-warning">{t("ota.conditionsChanged")}</p>}
      <ErrorBox message={error} />
      <div className="form-footer"><Button variant="secondary" disabled={busy} onClick={() => setTarget(undefined)}>{t("common.cancel")}</Button>
        <Button disabled={busy || !allowed || !unchanged} onClick={() => void submit()}>{t(busy ? "common.processing" : "ota.confirmUpgrade")}</Button></div>
    </Modal>
  </>;
}

export function NodeOTA({ node, user, version, updates, reload, actionsReady = true }: { node: Node; user: User; version?: EmissaryVersion; updates: EmissaryUpdate[]; reload: () => Promise<unknown>; actionsReady?: boolean }) {
  const { t } = useUI();
  const history = updates.filter(u => u.node_id === node.id);
  const latest = history[0];
  const active = history.find(u => !u.finished_at);
  const supported = node.capabilities.includes("emissary-ota-v1");
  const current = node.emissary_current_version || node.diagnostics.emissary_version || "";
  const aligned = !!version && matchesProductVersion(current, version.product_version);
  const certificateValid = Date.parse(node.certificate_expires_at) > Date.now();
  const phase = active ? `ota.stages.${active.status}` : !supported ? "ota.bootstrap" : aligned ? "ota.aligned" : node.revoked ? "ota.revoked" : !node.online ? "ota.offline" : !certificateValid ? "ota.certificateExpired" : node.emissary_pool_paused ? "ota.paused" : version?.release_status === "ready" ? "ota.awaitingUpdate" : "ota.waitingRelease";
  const canRequest = actionsReady && user.role === "admin" && supported && node.online && !node.revoked && certificateValid && !aligned && !active && version?.release_status === "ready" && !!version.release_id && !node.emissary_pool_paused;
  return <div className="space-y-4 text-xs" data-testid="node-ota">
    <NodeFacts entries={[
      [t("ota.current"), <span className="font-mono" key="current">{current || "—"}</span>],
      [t("ota.productVersion"), <span className="font-mono" key="target">{version?.product_version || "—"}</span>],
      ...(active ? [[t("ota.activeVersion"), <span className="font-mono" key="active">{active.version}</span>] as [string, ReactNode]] : []),
      [t("ota.launcher"), <span className="font-mono" key="launcher">{node.emissary_launcher_version || "—"}</span>],
      [t("ota.phase"), version || !supported || active ? t(phase) : "—"],
    ]} />
    {version && <><p className="text-xs leading-5 text-muted-foreground">{t("ota.followsProduct")}</p><ReleaseStatus version={version} /></>}
    {!supported && <p className="text-xs leading-5 text-muted-foreground">{t("ota.bootstrapHint")}</p>}
    {node.ota_draining && <p className="notice">{t(active?.status === "activating" ? "ota.activatingHint" : "ota.drainHint")}</p>}
    {node.emissary_pool_paused && <p className="text-warning">{t("ota.poolPaused")}</p>}
    {latest?.error && <p className="break-words text-destructive" role="status">{latest.error}</p>}
    {latest?.status === "rolled_back" && <p>{t("ota.restored", { version: node.emissary_current_version || "—" })}</p>}
    {user.role === "admin" && actionsReady && <div className="flex flex-wrap items-start gap-2">
      {version && supported && !aligned && !active && !node.revoked && <RequestUpdate node={node} version={version} allowed={canRequest} reload={reload} />}
      {latest && !node.revoked && <UpdateActions update={latest} version={version} reload={reload} allowRetry={!aligned} />}
    </div>}
  </div>;
}

export function EmissarySettings({ user }: { user: User }) {
  const { t, date } = useUI();
  const version = useRemote<EmissaryVersion>("/emissary-version", 30000);
  const releases = useRemote<EmissaryRelease[]>("/emissary-releases");
  const pools = useRemote<Pool[]>("/pools");
  const updates = useRemote<EmissaryUpdate[]>("/emissary-updates", 3000);
  const admin = user.role === "admin";
  async function reload() { await Promise.all([version.reload(), releases.reload(), pools.reload(), updates.reload()]); }
  return <div className="space-y-6" data-testid="emissary-settings">
    <NodeInstallationSettings />
    <ErrorBox message={version.error || releases.error || pools.error || updates.error} />
    {((!version.data && version.error) || (!releases.data && releases.error) || (!pools.data && pools.error) || (!updates.data && updates.error)) && <Button variant="secondary" onClick={() => void reload()}>{t("common.retry")}</Button>}
    <section className="panel"><SectionTitle title={t("ota.productVersion")} detail={t("ota.followsProduct")} />
      <div className="space-y-3 p-6 pt-0">{!version.data ? !version.error && <Loading /> : <><p className="font-mono text-sm" data-testid="product-version">{version.data.product_version}</p><ReleaseStatus version={version.data} /></>}</div>
    </section>
    <section className="panel"><SectionTitle title={t("ota.releases")} detail={t("ota.importHint")} />
      <div className="space-y-4 p-6 pt-0">
        {admin && <SubmitForm label={t("ota.import")} onSubmit={async form => { await mutation("/emissary-releases", { tag: form.get("tag") }); await reload(); }}>
          <Field label={t("ota.tag")}><Input name="tag" required maxLength={100} placeholder={version.data ? `emissary-v${version.data.product_version}` : "emissary-v0.2.0"} /></Field>
        </SubmitForm>}
        {!releases.data ? !releases.error && <Loading /> : <div className="flex flex-wrap gap-2">{releases.data.map(r => <span className="tag" key={r.id}>{r.version}</span>)}{!releases.data.length && <p className="text-xs text-muted-foreground">{t("ota.noReleases")}</p>}</div>}
      </div>
    </section>
    <section className="panel"><SectionTitle title={t("ota.pools")} detail={t("ota.poolHint")} />
      {!pools.data || !version.data ? !(pools.error || version.error) && <Loading variant="form" /> : <div className="grid gap-6 p-6 pt-0 md:grid-cols-2">{pools.data.map(pool => <div className="space-y-3 rounded-lg border border-border p-4" key={pool.id}>
        <h3 className="text-sm font-medium">{pool.name}</h3>
        <p className="text-xs">{t("ota.productVersion")} <span className="font-mono">{version.data?.product_version}</span></p>
        {admin ? <SubmitForm onSubmit={async form => {
          await mutation(`/pools/${pool.id}/emissary-update`, { release_id: null, concurrency: Number(form.get("concurrency")), paused: form.get("paused") === "on" }, "PATCH"); await reload();
        }}>
          <Field label={t("ota.concurrency")}><Input name="concurrency" type="number" required min={1} max={32} defaultValue={pool.emissary_update_concurrency || 1} /></Field>
          <label className="flex gap-2 text-xs"><input type="checkbox" name="paused" defaultChecked={pool.emissary_updates_paused} />{t("ota.pause")}</label>
        </SubmitForm> : <p className="text-xs">{pool.emissary_updates_paused ? t("ota.pause") : t("ota.stages.idle")}</p>}
      </div>)}</div>}
    </section>
    <section className="panel"><SectionTitle title={t("ota.history")} />
      {!updates.data ? !updates.error && <Loading variant="table" /> : <div className="table-scroll"><table><thead><tr>{[t("common.name"),t("ota.target"),t("ota.phase"),t("common.time"),t("ota.reason"),""].map((s,i)=><th key={i}>{s}</th>)}</tr></thead><tbody>
        {updates.data.map(u => <tr key={u.id}><td>{u.node_name}</td><td className="font-mono">{u.previous_version} → {u.version}</td><td>{t(`ota.stages.${u.status}`)}</td><td className="text-xs">{date(u.updated_at)}</td><td className="max-w-sm break-words text-xs">{u.error || "—"}</td><td>{admin && <UpdateActions update={u} version={version.data} reload={reload} />}</td></tr>)}
      </tbody></table></div>}
    </section>
  </div>;
}
